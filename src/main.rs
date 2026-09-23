use std::{
    env,
    error::Error,
    fs,
    net::TcpListener as StdTcpListener,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use futures_util::{SinkExt, StreamExt};
use mlua::{
    AnyUserData, Error as LuaError, Function, Lua, LuaString, MultiValue, Result as LuaResult,
    Table, Thread, UserData, Value,
};
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::{Mutex, mpsc},
    task::LocalSet,
};
use tokio_tungstenite::{
    WebSocketStream, accept_async, connect_async, tungstenite::protocol::Message,
};
use uuid::Uuid;

mod standard_library {
    include!(concat!(env!("OUT_DIR"), "/standard_library.rs"));
}

type AppResult<T> = Result<T, Box<dyn Error>>;

const USAGE: &str = "Usage: luauxtx <script.luau> [-- <script arguments...>]\n\nRuns a trusted Luau script with async system, net, and Promise APIs.";

#[derive(Clone)]
struct ModuleLoader {
    native: Table,
    file_cache: Table,
    file_loading: Table,
    standard_cache: Table,
    standard_loading: Table,
}

enum WebSocketOutgoing {
    Text(String),
    Binary(Vec<u8>),
    Close,
}

struct WebSocketIncoming {
    kind: &'static str,
    data: Vec<u8>,
}

struct WebSocketHandle {
    sender: mpsc::UnboundedSender<WebSocketOutgoing>,
    receiver: Arc<Mutex<mpsc::UnboundedReceiver<Result<WebSocketIncoming, String>>>>,
}

impl UserData for WebSocketHandle {}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("luauxtx: {error}");
        std::process::exit(1);
    }
}

async fn run() -> AppResult<()> {
    let mut arguments = env::args_os().skip(1);
    let Some(script) = arguments.next() else {
        println!("{USAGE}");
        return Ok(());
    };
    if script == "--help" || script == "-h" {
        println!("{USAGE}");
        return Ok(());
    }

    let script = absolute_path(PathBuf::from(script))?;
    let script_arguments: Vec<String> = arguments
        .filter(|argument| argument != "--")
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect();

    let local = LocalSet::new();
    let result = local
        .run_until(async {
            let lua = Lua::new();
            install_host_apis(&lua, &script, &script_arguments)?;
            execute_entry(&lua, &script, &script_arguments).await
        })
        .await;
    local.await;
    result.map_err(Into::into)
}

async fn execute_entry(lua: &Lua, path: &Path, arguments: &[String]) -> LuaResult<()> {
    let source = fs::read_to_string(path).map_err(|error| {
        LuaError::RuntimeError(format!("cannot read script {}: {error}", path.display()))
    })?;
    let arguments = MultiValue::from_vec(
        arguments
            .iter()
            .map(|argument| lua.create_string(argument).map(Value::String))
            .collect::<LuaResult<Vec<_>>>()?,
    );
    lua.load(&source)
        .set_name(path.to_string_lossy().as_ref())
        .call_async(arguments)
        .await
}

fn install_host_apis(lua: &Lua, script: &Path, script_arguments: &[String]) -> LuaResult<()> {
    let globals = lua.globals();
    let native = create_native_api(lua)?;
    let process = lua.create_table()?;
    let executable = env::args().next().unwrap_or_else(|| "luauxtx".to_owned());
    let argv = lua.create_table()?;
    argv.set(1, executable)?;
    argv.set(2, script.to_string_lossy().as_ref())?;
    for (index, argument) in script_arguments.iter().enumerate() {
        argv.set(index + 3, argument.as_str())?;
    }
    process.set("argv", argv)?;
    process.set(
        "cwd",
        lua.create_function(|_, ()| {
            env::current_dir()
                .map(|path| path.to_string_lossy().into_owned())
                .map_err(LuaError::external)
        })?,
    )?;
    process.set(
        "getenv",
        lua.create_function(|_, name: String| Ok(env::var(name).ok()))?,
    )?;
    globals.set("process", process)?;

    let loader = ModuleLoader {
        native,
        file_cache: lua.create_table()?,
        file_loading: lua.create_table()?,
        standard_cache: lua.create_table()?,
        standard_loading: lua.create_table()?,
    };
    let entry_directory = script.parent().unwrap_or_else(|| Path::new("."));
    globals.set(
        "require",
        create_require(lua, entry_directory.to_path_buf(), loader.clone())?,
    )?;

    let promise = standard_table(lua, "async/promise", entry_directory, loader.clone())?;
    let task = standard_table(lua, "async/task", entry_directory, loader.clone())?;
    let fs_api = standard_table(lua, "system/fs", entry_directory, loader.clone())?;
    let http = standard_table(lua, "net/http", entry_directory, loader)?;
    globals.set("Promise", promise)?;
    globals.set("task", task)?;
    globals.set("fs", fs_api)?;
    globals.set("http", http)?;
    Ok(())
}

fn create_native_api(lua: &Lua) -> LuaResult<Table> {
    let native = lua.create_table()?;
    native.set(
        "sleep",
        lua.create_async_function(|_, seconds: Option<f64>| async move {
            let seconds = seconds.unwrap_or(0.0);
            if !seconds.is_finite() || seconds < 0.0 {
                return Err(LuaError::RuntimeError(
                    "task.wait expects a non-negative number of seconds".to_owned(),
                ));
            }
            let started = Instant::now();
            tokio::time::sleep(Duration::from_secs_f64(seconds)).await;
            Ok(started.elapsed().as_secs_f64())
        })?,
    )?;
    native.set(
        "spawn",
        lua.create_function(|lua, (function, arguments): (Function, MultiValue)| {
            schedule_thread(lua, Duration::ZERO, function, arguments)
        })?,
    )?;
    native.set(
        "defer",
        lua.create_function(|lua, (function, arguments): (Function, MultiValue)| {
            schedule_thread(lua, Duration::ZERO, function, arguments)
        })?,
    )?;
    native.set(
        "delay",
        lua.create_function(
            |lua, (seconds, function, arguments): (f64, Function, MultiValue)| {
                if !seconds.is_finite() || seconds < 0.0 {
                    return Err(LuaError::RuntimeError(
                        "task.delay expects a non-negative number of seconds".to_owned(),
                    ));
                }
                schedule_thread(lua, Duration::from_secs_f64(seconds), function, arguments)
            },
        )?,
    )?;
    native.set(
        "fs_read",
        lua.create_function(|_, (path, resolve, reject): (String, Function, Function)| {
            tokio::task::spawn_local(async move {
                match tokio::fs::read_to_string(&path).await {
                    Ok(contents) => settle(resolve.call::<()>(contents)),
                    Err(error) => {
                        settle(reject.call::<()>(format!("fs.read({path:?}) failed: {error}")))
                    }
                }
            });
            Ok(())
        })?,
    )?;
    native.set(
        "fs_write",
        lua.create_function(
            |_, (path, contents, resolve, reject): (String, String, Function, Function)| {
                tokio::task::spawn_local(async move {
                    match tokio::fs::write(&path, contents).await {
                        Ok(()) => settle(resolve.call::<()>(())),
                        Err(error) => {
                            settle(reject.call::<()>(format!("fs.write({path:?}) failed: {error}")))
                        }
                    }
                });
                Ok(())
            },
        )?,
    )?;
    native.set(
        "fs_read_sync",
        lua.create_function(|_, path: String| {
            fs::read_to_string(&path).map_err(|error| {
                LuaError::RuntimeError(format!("fs.readSync({path:?}) failed: {error}"))
            })
        })?,
    )?;
    native.set(
        "fs_write_sync",
        lua.create_function(|_, (path, contents): (String, String)| {
            fs::write(&path, contents).map_err(|error| {
                LuaError::RuntimeError(format!("fs.writeSync({path:?}) failed: {error}"))
            })
        })?,
    )?;
    native.set(
        "fs_exists",
        lua.create_function(|_, path: String| Ok(Path::new(&path).exists()))?,
    )?;
    native.set(
        "http_get",
        lua.create_function(|_, (url, resolve, reject): (String, Function, Function)| {
            tokio::task::spawn_local(async move {
                let response = async {
                    let response = reqwest::get(&url)
                        .await
                        .map_err(|error| format!("http.fetch({url:?}) failed: {error}"))?;
                    let status = i64::from(response.status().as_u16());
                    let body = response.text().await.map_err(|error| {
                        format!("http.fetch({url:?}) failed while reading body: {error}")
                    })?;
                    Ok::<_, String>((status, body))
                }
                .await;

                match response {
                    Ok((status, body)) => settle(resolve.call::<()>((status, body))),
                    Err(error) => settle(reject.call::<()>(error)),
                }
            });
            Ok(())
        })?,
    )?;
    native.set(
        "http_listen",
        lua.create_function(|_, (port, handler): (u16, Function)| {
            let listener = StdTcpListener::bind(("127.0.0.1", port)).map_err(|error| {
                LuaError::RuntimeError(format!("http server cannot listen on {port}: {error}"))
            })?;
            listener.set_nonblocking(true).map_err(LuaError::external)?;
            let port = listener.local_addr().map_err(LuaError::external)?.port();
            let listener = TcpListener::from_std(listener).map_err(LuaError::external)?;
            tokio::task::spawn_local(async move {
                loop {
                    match listener.accept().await {
                        Ok((socket, _)) => {
                            let handler = handler.clone();
                            tokio::task::spawn_local(async move {
                                if let Err(error) = serve_http_connection(socket, handler).await {
                                    eprintln!("luauxtx: HTTP server request failed: {error}");
                                }
                            });
                        }
                        Err(error) => {
                            eprintln!("luauxtx: HTTP server accept failed: {error}");
                            break;
                        }
                    }
                }
            });
            Ok(port)
        })?,
    )?;
    native.set(
        "websocket_connect",
        lua.create_function(
            move |lua, (url, resolve, reject): (String, Function, Function)| {
                let lua = lua.clone();
                tokio::task::spawn_local(async move {
                    match connect_async(&url).await {
                        Ok((stream, _)) => match lua.create_userdata(websocket_handle(stream)) {
                            Ok(socket) => settle(resolve.call::<()>(socket)),
                            Err(error) => settle(reject.call::<()>(error.to_string())),
                        },
                        Err(error) => settle(
                            reject
                                .call::<()>(format!("websocket.connect({url:?}) failed: {error}")),
                        ),
                    }
                });
                Ok(())
            },
        )?,
    )?;
    native.set(
        "websocket_receive",
        lua.create_function(
            move |lua, (socket, resolve, reject): (AnyUserData, Function, Function)| {
                let receiver = socket.borrow::<WebSocketHandle>()?.receiver.clone();
                let lua = lua.clone();
                tokio::task::spawn_local(async move {
                    match receiver.lock().await.recv().await {
                        Some(Ok(message)) => match lua.create_string(&message.data) {
                            Ok(data) => settle(resolve.call::<()>((message.kind, data))),
                            Err(error) => settle(reject.call::<()>(error.to_string())),
                        },
                        Some(Err(error)) => settle(reject.call::<()>(error)),
                        None => settle(resolve.call::<()>(())),
                    }
                });
                Ok(())
            },
        )?,
    )?;
    native.set(
        "websocket_send_text",
        lua.create_function(
            |_, (socket, message, resolve, reject): (AnyUserData, String, Function, Function)| {
                match socket
                    .borrow::<WebSocketHandle>()?
                    .sender
                    .send(WebSocketOutgoing::Text(message))
                {
                    Ok(()) => settle(resolve.call::<()>(())),
                    Err(_) => settle(reject.call::<()>("websocket is closed")),
                }
                Ok(())
            },
        )?,
    )?;
    native.set(
        "websocket_send_binary",
        lua.create_function(|_, (socket, message, resolve, reject): (AnyUserData, LuaString, Function, Function)| {
            let message = message.as_bytes().to_vec();
            match socket.borrow::<WebSocketHandle>()?.sender.send(WebSocketOutgoing::Binary(message)) {
                Ok(()) => settle(resolve.call::<()>(())),
                Err(_) => settle(reject.call::<()>("websocket is closed")),
            }
            Ok(())
        })?,
    )?;
    native.set(
        "websocket_close",
        lua.create_function(|_, (socket, resolve): (AnyUserData, Function)| {
            let _ = socket
                .borrow::<WebSocketHandle>()?
                .sender
                .send(WebSocketOutgoing::Close);
            settle(resolve.call::<()>(()));
            Ok(())
        })?,
    )?;
    native.set(
        "websocket_listen",
        lua.create_function(move |lua, (port, handler): (u16, Function)| {
            let listener = StdTcpListener::bind(("127.0.0.1", port)).map_err(|error| {
                LuaError::RuntimeError(format!("websocket server cannot listen on {port}: {error}"))
            })?;
            listener.set_nonblocking(true).map_err(LuaError::external)?;
            let port = listener.local_addr().map_err(LuaError::external)?.port();
            let listener = TcpListener::from_std(listener).map_err(LuaError::external)?;
            let lua = lua.clone();
            tokio::task::spawn_local(async move {
                while let Ok((stream, _)) = listener.accept().await {
                    let lua = lua.clone();
                    let handler = handler.clone();
                    tokio::task::spawn_local(async move {
                        match accept_async(stream).await {
                            Ok(stream) => match lua.create_userdata(websocket_handle(stream)) {
                                Ok(socket) => settle(handler.call::<()>(socket)),
                                Err(error) => eprintln!(
                                    "luauxtx: WebSocket server connection failed: {error}"
                                ),
                            },
                            Err(error) => {
                                eprintln!("luauxtx: WebSocket server connection failed: {error}")
                            }
                        }
                    });
                }
            });
            Ok(port)
        })?,
    )?;
    native.set(
        "sha256",
        lua.create_function(|_, input: String| {
            Ok(format!("{:x}", Sha256::digest(input.as_bytes())))
        })?,
    )?;
    native.set(
        "uuid_v4",
        lua.create_function(|_, ()| Ok(Uuid::new_v4().to_string()))?,
    )?;
    Ok(native)
}

fn websocket_handle<S>(stream: WebSocketStream<S>) -> WebSocketHandle
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + 'static,
{
    let (mut writer, mut reader) = stream.split();
    let (sender, mut outgoing) = mpsc::unbounded_channel();
    let (incoming_sender, incoming) = mpsc::unbounded_channel();
    let writer_errors = incoming_sender.clone();
    tokio::task::spawn_local(async move {
        while let Some(message) = outgoing.recv().await {
            let message = match message {
                WebSocketOutgoing::Text(text) => Message::Text(text.into()),
                WebSocketOutgoing::Binary(data) => Message::Binary(data.into()),
                WebSocketOutgoing::Close => Message::Close(None),
            };
            let closing = matches!(message, Message::Close(_));
            if let Err(error) = writer.send(message).await {
                let _ = writer_errors.send(Err(format!("websocket write failed: {error}")));
                break;
            }
            if closing {
                break;
            }
        }
    });
    tokio::task::spawn_local(async move {
        while let Some(message) = reader.next().await {
            match message {
                Ok(Message::Text(text)) => {
                    let _ = incoming_sender.send(Ok(WebSocketIncoming {
                        kind: "text",
                        data: text.as_bytes().to_vec(),
                    }));
                }
                Ok(Message::Binary(data)) => {
                    let _ = incoming_sender.send(Ok(WebSocketIncoming {
                        kind: "binary",
                        data: data.to_vec(),
                    }));
                }
                Ok(Message::Close(_)) => {
                    let _ = incoming_sender.send(Ok(WebSocketIncoming {
                        kind: "close",
                        data: Vec::new(),
                    }));
                    break;
                }
                Ok(Message::Ping(_)) | Ok(Message::Pong(_)) | Ok(Message::Frame(_)) => {}
                Err(error) => {
                    let _ = incoming_sender.send(Err(format!("websocket read failed: {error}")));
                    break;
                }
            }
        }
    });
    WebSocketHandle {
        sender,
        receiver: Arc::new(Mutex::new(incoming)),
    }
}

async fn serve_http_connection(
    mut socket: tokio::net::TcpStream,
    handler: Function,
) -> LuaResult<()> {
    let (method, target, body) = read_http_request(&mut socket).await?;
    let (status, headers, body): (i64, Table, String) =
        handler.call_async((method, target, body)).await?;
    let status = u16::try_from(status)
        .ok()
        .filter(|status| (100..=599).contains(status))
        .unwrap_or(500);
    let mut response_headers = String::new();
    for pair in headers.pairs::<String, String>() {
        let (name, value) = pair?;
        if name.contains(['\r', '\n']) || value.contains(['\r', '\n']) {
            return Err(LuaError::RuntimeError(
                "HTTP response headers cannot contain newlines".to_owned(),
            ));
        }
        response_headers.push_str(&format!("{name}: {value}\r\n"));
    }
    if !response_headers
        .to_ascii_lowercase()
        .contains("content-type:")
    {
        response_headers.push_str("Content-Type: text/plain; charset=utf-8\r\n");
    }
    response_headers.push_str(&format!(
        "Content-Length: {}\r\nConnection: close\r\n",
        body.len()
    ));
    let response = format!(
        "HTTP/1.1 {status} {}\r\n{response_headers}\r\n",
        reason_phrase(status)
    );
    socket
        .write_all(response.as_bytes())
        .await
        .map_err(LuaError::external)?;
    socket
        .write_all(body.as_bytes())
        .await
        .map_err(LuaError::external)?;
    Ok(())
}

async fn read_http_request(
    socket: &mut tokio::net::TcpStream,
) -> LuaResult<(String, String, String)> {
    const MAX_REQUEST_SIZE: usize = 1024 * 1024;
    let mut bytes = Vec::with_capacity(4096);
    let header_end = loop {
        if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
        if bytes.len() >= MAX_REQUEST_SIZE {
            return Err(LuaError::RuntimeError(
                "HTTP request is too large".to_owned(),
            ));
        }
        let mut chunk = [0; 4096];
        let read = socket.read(&mut chunk).await.map_err(LuaError::external)?;
        if read == 0 {
            return Err(LuaError::RuntimeError(
                "HTTP client closed request early".to_owned(),
            ));
        }
        bytes.extend_from_slice(&chunk[..read]);
    };
    let header_text = std::str::from_utf8(&bytes[..header_end]).map_err(LuaError::external)?;
    let mut lines = header_text.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next().unwrap_or_default().to_owned();
    let target = request_parts.next().unwrap_or_default().to_owned();
    if method.is_empty() || target.is_empty() || request_parts.next().is_none() {
        return Err(LuaError::RuntimeError(
            "malformed HTTP request line".to_owned(),
        ));
    }
    let content_length = lines
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .map(|(_, value)| value.trim().parse::<usize>())
        .transpose()
        .map_err(LuaError::external)?
        .unwrap_or(0);
    if content_length > MAX_REQUEST_SIZE || header_end + content_length > MAX_REQUEST_SIZE {
        return Err(LuaError::RuntimeError(
            "HTTP request is too large".to_owned(),
        ));
    }
    while bytes.len() < header_end + content_length {
        let mut chunk = [0; 4096];
        let read = socket.read(&mut chunk).await.map_err(LuaError::external)?;
        if read == 0 {
            return Err(LuaError::RuntimeError(
                "HTTP client closed request body early".to_owned(),
            ));
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
    let body = String::from_utf8(bytes[header_end..header_end + content_length].to_vec())
        .map_err(LuaError::external)?;
    Ok((method, target, body))
}

fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        500 => "Internal Server Error",
        _ => "OK",
    }
}

fn schedule_thread(
    lua: &Lua,
    delay: Duration,
    function: Function,
    arguments: MultiValue,
) -> LuaResult<Thread> {
    let thread = lua.create_thread(function)?;
    let runnable = thread.clone().into_async::<()>(arguments)?;
    tokio::task::spawn_local(async move {
        if delay.is_zero() {
            tokio::task::yield_now().await;
        } else {
            tokio::time::sleep(delay).await;
        }
        if let Err(error) = runnable.await {
            eprintln!("luauxtx: unhandled task error: {error}");
        }
    });
    Ok(thread)
}

fn settle(result: LuaResult<()>) {
    if let Err(error) = result {
        eprintln!("luauxtx: failed to settle promise: {error}");
    }
}

fn create_require(lua: &Lua, base_directory: PathBuf, loader: ModuleLoader) -> LuaResult<Function> {
    lua.create_function(move |lua, specifier: String| {
        if specifier == "process" {
            return lua.globals().get("process");
        }
        if standard_library_source(&specifier).is_some() {
            return execute_standard_module(lua, &specifier, &base_directory, loader.clone());
        }

        let module_path = resolve_module(&base_directory, &specifier)
            .map_err(|error| LuaError::RuntimeError(error.to_string()))?;
        execute_module(lua, &module_path, loader.clone())
    })
}

fn standard_table(
    lua: &Lua,
    name: &str,
    base_directory: &Path,
    loader: ModuleLoader,
) -> LuaResult<Table> {
    match execute_standard_module(lua, name, base_directory, loader)? {
        Value::Table(table) => Ok(table),
        _ => Err(LuaError::RuntimeError(format!(
            "standard library module {name:?} must return a table"
        ))),
    }
}

fn execute_standard_module(
    lua: &Lua,
    name: &str,
    base_directory: &Path,
    loader: ModuleLoader,
) -> LuaResult<Value> {
    let source = standard_library_source(name).ok_or_else(|| {
        LuaError::RuntimeError(format!("unknown standard library module {name:?}"))
    })?;
    let cached: Value = loader.standard_cache.raw_get(name)?;
    if !matches!(cached, Value::Nil) {
        return Ok(cached);
    }
    if loader
        .standard_loading
        .raw_get::<bool>(name)
        .unwrap_or(false)
    {
        return Err(LuaError::RuntimeError(format!(
            "circular require detected while loading standard library module {name:?}"
        )));
    }

    loader.standard_loading.raw_set(name, true)?;
    let globals = lua.globals();
    let previous_require: Value = globals.get("require")?;
    globals.set(
        "require",
        create_require(lua, base_directory.to_path_buf(), loader.clone())?,
    )?;
    let chunk_name = format!("@xtx/{name}");
    let result = lua
        .load(source)
        .set_name(&chunk_name)
        .call::<Value>(loader.native.clone());
    globals.set("require", previous_require)?;
    loader.standard_loading.raw_set(name, Value::Nil)?;

    let value = result?;
    loader.standard_cache.raw_set(name, value.clone())?;
    Ok(value)
}

fn standard_library_source(name: &str) -> Option<&'static str> {
    standard_library::STANDARD_LIBRARY
        .iter()
        .find_map(|(module_name, source)| (*module_name == name).then_some(*source))
}

fn execute_module(lua: &Lua, path: &Path, loader: ModuleLoader) -> LuaResult<Value> {
    let path = absolute_path(path.to_path_buf()).map_err(LuaError::external)?;
    let cache_key = path.to_string_lossy();
    let cached: Value = loader.file_cache.raw_get(cache_key.as_ref())?;
    if !matches!(cached, Value::Nil) {
        return Ok(cached);
    }
    if loader
        .file_loading
        .raw_get::<bool>(cache_key.as_ref())
        .unwrap_or(false)
    {
        return Err(LuaError::RuntimeError(format!(
            "circular require detected while loading {}",
            path.display()
        )));
    }

    let source = fs::read_to_string(&path).map_err(|error| {
        LuaError::RuntimeError(format!("cannot read module {}: {error}", path.display()))
    })?;
    loader.file_loading.raw_set(cache_key.as_ref(), true)?;

    let globals = lua.globals();
    let previous_require: Value = globals.get("require")?;
    let directory = path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    globals.set("require", create_require(lua, directory, loader.clone())?)?;

    let result = lua
        .load(&source)
        .set_name(path.to_string_lossy().as_ref())
        .eval::<Value>();
    globals.set("require", previous_require)?;
    loader
        .file_loading
        .raw_set(cache_key.as_ref(), Value::Nil)?;

    let value = result?;
    loader
        .file_cache
        .raw_set(cache_key.as_ref(), value.clone())?;
    Ok(value)
}

fn resolve_module(base_directory: &Path, specifier: &str) -> AppResult<PathBuf> {
    let requested = Path::new(specifier);
    let candidate = if requested.is_absolute() {
        requested.to_path_buf()
    } else if specifier.starts_with("./") || specifier.starts_with("../") {
        base_directory.join(requested)
    } else {
        return Err(format!(
            "unsupported module {specifier:?}; use a relative path or a built-in module"
        )
        .into());
    };

    let options = [
        candidate.clone(),
        candidate.with_extension("luau"),
        candidate.with_extension("lua"),
        candidate.join("init.luau"),
        candidate.join("init.lua"),
    ];
    options
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| {
            format!(
                "cannot resolve module {specifier:?} from {}",
                base_directory.display()
            )
            .into()
        })
}

fn absolute_path(path: PathBuf) -> AppResult<PathBuf> {
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(env::current_dir()?.join(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
    };

    #[test]
    fn resolves_luau_and_index_modules() {
        let root = env::temp_dir().join(format!("luauxtx-test-{}", std::process::id()));
        fs::create_dir_all(root.join("nested")).unwrap();
        fs::write(root.join("value.luau"), "return 42").unwrap();
        fs::write(root.join("nested/init.luau"), "return 'nested'").unwrap();

        assert_eq!(
            resolve_module(&root, "./value").unwrap(),
            root.join("value.luau")
        );
        assert_eq!(
            resolve_module(&root, "./nested").unwrap(),
            root.join("nested/init.luau")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn executes_nested_relative_modules() {
        let root = env::temp_dir().join(format!("luauxtx-runtime-test-{}", std::process::id()));
        let modules = root.join("modules");
        fs::create_dir_all(&modules).unwrap();
        let entry = root.join("app.luau");
        fs::write(&entry, "return require('./modules/value')").unwrap();
        fs::write(modules.join("value.luau"), "return require('../answer')").unwrap();
        fs::write(root.join("answer.luau"), "return 42").unwrap();

        let lua = Lua::new();
        install_host_apis(&lua, &entry, &[]).unwrap();
        let require: Function = lua.globals().get("require").unwrap();
        let value: Value = require.call("./modules/value").unwrap();

        assert!(matches!(value, Value::Integer(42)));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn require_loads_embedded_standard_library_modules() {
        let lua = Lua::new();
        let script = PathBuf::from("standard-library-test.luau");
        install_host_apis(&lua, &script, &[]).unwrap();

        let uses_embedded_modules: bool = lua
            .load("return require('async/promise') == Promise and require('system/fs') == fs")
            .eval()
            .unwrap();

        assert!(uses_embedded_modules);
        assert!(standard_library_source("net/http").is_some());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn promises_timers_and_async_fs_work_together() {
        let root = env::temp_dir().join(format!("luauxtx-async-test-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let input = root.join("input.txt");
        fs::write(&input, "async content").unwrap();
        let script = root.join("app.luau");
        let source = format!(
            "assert(__xtx == nil)\nlocal value = fs.read({:?}):andThen(function(text) return text .. '!' end):await()\nlocal finished = false\ntask.delay(0.001, function() finished = true end)\ntask.wait(0.1)\nassert(finished)\nreturn value",
            input.to_string_lossy()
        );
        fs::write(&script, &source).unwrap();

        let local = LocalSet::new();
        let value: String = local
            .run_until(async {
                let lua = Lua::new();
                install_host_apis(&lua, &script, &[])?;
                lua.load(&source).eval_async().await
            })
            .await
            .unwrap();
        local.await;

        assert_eq!(value, "async content!");
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn async_http_fetch_returns_a_response() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 1024];
            socket.read(&mut request).await.unwrap();
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello",
                )
                .await
                .unwrap();
        });

        let local = LocalSet::new();
        let body: String = local
            .run_until(async {
                let lua = Lua::new();
                let script = PathBuf::from("http-test.luau");
                install_host_apis(&lua, &script, &[])?;
                lua.load(format!(
                    "local response = http.fetch('http://{address}/'):await()\nassert(response.ok)\nreturn response:text()"
                ))
                .eval_async()
                .await
            })
            .await
            .unwrap();
        local.await;
        server.await.unwrap();

        assert_eq!(body, "hello");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn http_server_routes_requests_and_serializes_json() {
        let local = LocalSet::new();
        let response = local
            .run_until(async {
                let lua = Lua::new();
                let script = PathBuf::from("http-server-test.luau");
                install_host_apis(&lua, &script, &[])?;
                let port: u16 = lua
                    .load(
                        "local http = require('net/http/server')\nlocal Router = require('net/http/router')\nlocal app = Router.new()\napp:get('/users/:id', function(req, res) res:json({ id = req.params.id }) end)\nreturn http.createServer(app):listen(0)",
                    )
                    .eval_async()
                    .await?;
                let mut socket = TcpStream::connect(("127.0.0.1", port))
                    .await
                    .map_err(LuaError::external)?;
                socket
                    .write_all(b"GET /users/42 HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                    .await
                    .map_err(LuaError::external)?;
                let mut response = String::new();
                socket
                    .read_to_string(&mut response)
                    .await
                    .map_err(LuaError::external)?;
                Ok::<_, LuaError>(response)
            })
            .await
            .unwrap();

        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(response.contains("Content-Type: application/json\r\n"));
        assert!(response.ends_with("\r\n\r\n{\"id\":\"42\"}"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn websocket_client_and_server_exchange_messages() {
        let local = LocalSet::new();
        let message: String = local
            .run_until(async {
                let lua = Lua::new();
                let script = PathBuf::from("websocket-test.luau");
                install_host_apis(&lua, &script, &[])?;
                lua.load(
                    "local websocket = require('net/websocket')\nlocal port = websocket.listen(0, function(socket)\n    socket:receive():andThen(function(message) socket:send('echo:' .. message.data) end)\nend)\nlocal client = websocket.connect('ws://127.0.0.1:' .. port):await()\nclient:send('hello'):await()\nreturn client:receive():await().data",
                )
                .eval_async()
                .await
            })
            .await
            .unwrap();

        assert_eq!(message, "echo:hello");
    }

    #[test]
    fn loads_new_namespaced_library_modules() {
        let lua = Lua::new();
        let script = PathBuf::from("namespaced-library-test.luau");
        install_host_apis(&lua, &script, &[]).unwrap();

        let valid: bool = lua
            .load(
                "local base64 = require('crypto/base64')\nlocal hash = require('crypto/hash')\nlocal uuid = require('crypto/uuid')\nlocal url = require('net/url')\nlocal path = require('system/path')\nlocal args = require('cli/args')\nlocal encoded = base64.encode('hello')\nassert(base64.decode(encoded) == 'hello', encoded .. ' / ' .. base64.decode(encoded))\nassert(hash.sha256('hello') == '2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824', 'hash')\nassert(uuid.v4():match('^[0-9a-f%-]+$'), 'uuid')\nassert(url.parse('https://example.com/a').hostname == 'example.com', 'url')\nassert(path.join('a', '..', 'b') == 'b', 'path')\nreturn args.parse({'--port=3000'}).port == '3000'",
            )
            .eval()
            .unwrap();

        assert!(valid);
    }
}
