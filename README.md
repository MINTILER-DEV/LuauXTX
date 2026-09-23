# LuauXTX

LuauXTX is an early Node-style runtime for trusted Luau scripts. Rust hosts the
official Luau VM; Tokio provides its event loop and asynchronous system APIs.

## Standard library

Public runtime APIs are implemented in Luau under [`lib/`](lib/) and bundled
into the executable at compile time. Rust exposes private callback-based host
primitives to those modules only; user scripts receive the public `Promise`,
`task`, `fs`, and `http` APIs instead.

- [`lib/async/promise.luau`](lib/async/promise.luau) implements promise state and chaining.
- [`lib/async/task.luau`](lib/async/task.luau) wraps coroutine scheduling and timers.
- [`lib/system/fs.luau`](lib/system/fs.luau) turns filesystem completions into promises.
- [`lib/net/http/client.luau`](lib/net/http/client.luau) provides HTTP client requests.
- [`lib/net/http/server.luau`](lib/net/http/server.luau) provides a small HTTP server.

To add a built-in module, create a `.luau` file in `lib/` and rebuild. Module
names follow their path, so `lib/system/path.luau` is loaded with
`require("system/path")` and `lib/crypto/base64.luau` is loaded with
`require("crypto/base64")`.
Directory modules can use `init.luau`, such as `lib/json/init.luau` for
`require("json")`. The build rejects duplicate module names.

```luau
-- lib/system/path.luau
local path = {}

function path.join(left, right)
    return left .. "/" .. right
end

return path
```

```luau
local path = require("system/path")
print(path.join("src", "main.luau"))
```

## Run

```bash
cargo run -- examples/hello.luau world
```

The executable accepts a script followed by script arguments. A standalone
separator is optional:

```bash
luauxtx app.luau -- production 3000
```

## Available APIs

```lua
print(process.argv[3])
print(process.cwd())
print(process.getenv("HOME"))

fs.write("message.txt", "hello"):await()
print(fs.read("message.txt"):await())

local config = require("./config") -- .luau, .lua, or directory/init.luau
```

`fs.read(path)` and `fs.write(path, contents)` return `Promise` objects and use
Tokio's asynchronous filesystem APIs. `fs.readSync` and `fs.writeSync` are
available when blocking behavior is intentional.

```lua
task.delay(0.5, function()
    print("later")
end)

local response = http.fetch("https://example.com"):await()
print(response.status, response.body)
```

`task.wait`, `task.spawn`, `task.defer`, and `task.delay` use the runtime event
loop. `Promise` supports `resolve`, `reject`, `all`, `andThen`, `catch`, and
`await`. HTTP currently provides an asynchronous `http.fetch(url)` GET client.

For a small HTTP server, use `require("net/http/server")`. `createServer` takes
a function (or a `net/http/router` router), and `listen(port)` begins accepting
connections on `127.0.0.1`. Request bodies are strings; `res:send`,
`res:json`, `res:status`, and `res:header` build the response.

`require("net/websocket")` supports both client and standalone server sockets.
`websocket.connect("ws://...")` (and `wss://`) returns a Promise for a socket;
use `socket:send`, `socket:sendBinary`, `socket:receive`, and `socket:close`.
`websocket.listen(port, handler)` accepts WebSocket connections on `127.0.0.1`.

`require` caches module return values. Modules must return the value they
export. Built-in modules are namespaced: `require("system/fs")`,
`require("system/path")`, `require("net/http")`, `require("net/url")`,
`require("async/promise")`, and `require("async/task")`. `process`, `fs`,
`http`, `task`, and `Promise` remain available as globals.

The entry script may use `:await()`. Modules loaded by `require` execute
synchronously while loading, so asynchronous work belongs in exported
functions or in a promise returned by the module.

This runtime runs trusted local code. Its file APIs are not sandboxed.

## Next steps

- HTTP server and TCP APIs
- package resolution and a package manager
- permissions and isolated script execution
