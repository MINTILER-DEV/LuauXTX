use std::{
    env,
    error::Error,
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use mlua::{
    Error as LuaError, Function, Lua, MultiValue, Result as LuaResult, Table, Thread, Value,
};
use tokio::task::LocalSet;

mod standard_library {
    include!(concat!(env!("OUT_DIR"), "/standard_library.rs"));
}

type AppResult<T> = Result<T, Box<dyn Error>>;

const USAGE: &str = "Usage: luauxtx <script.luau> [-- <script arguments...>]\n\nRuns a trusted Luau script with async fs, http, task, and Promise APIs.";

#[derive(Clone)]
struct ModuleLoader {
    native: Table,
    file_cache: Table,
    file_loading: Table,
    standard_cache: Table,
    standard_loading: Table,
}

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

    let promise = standard_table(lua, "promise", entry_directory, loader.clone())?;
    let task = standard_table(lua, "task", entry_directory, loader.clone())?;
    let fs_api = standard_table(lua, "fs", entry_directory, loader.clone())?;
    let http = standard_table(lua, "http", entry_directory, loader)?;
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
    Ok(native)
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
        net::TcpListener,
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
            .load("return require('promise') == Promise and require('fs') == fs")
            .eval()
            .unwrap();

        assert!(uses_embedded_modules);
        assert!(standard_library_source("http").is_some());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn promises_timers_and_async_fs_work_together() {
        let root = env::temp_dir().join(format!("luauxtx-async-test-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let input = root.join("input.txt");
        fs::write(&input, "async content").unwrap();
        let script = root.join("app.luau");
        let source = format!(
            "assert(__xtx == nil)\nlocal value = fs.read({:?}):andThen(function(text) return text .. '!' end):await()\nlocal finished = false\ntask.delay(0.001, function() finished = true end)\ntask.wait(0.01)\nassert(finished)\nreturn value",
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
}
