# LuauXTX

LuauXTX is an early Node-style runtime for trusted Luau scripts. Rust hosts the
official Luau VM; Tokio provides its event loop and asynchronous system APIs.

## Standard library

Public runtime APIs are implemented in Luau under [`lib/`](lib/) and bundled
into the executable at compile time. Rust exposes private callback-based host
primitives to those modules only; user scripts receive the public `Promise`,
`task`, `fs`, and `http` APIs instead.

- [`lib/promise.luau`](lib/promise.luau) implements promise state and chaining.
- [`lib/task.luau`](lib/task.luau) wraps coroutine scheduling and timers.
- [`lib/fs.luau`](lib/fs.luau) turns filesystem completions into promises.
- [`lib/http.luau`](lib/http.luau) shapes HTTP responses as Luau values.

To add a built-in module, create a `.luau` file in `lib/` and rebuild. Module
names follow their path, so `lib/path.luau` is loaded with `require("path")`
and `lib/encoding/base64.luau` is loaded with `require("encoding/base64")`.
Directory modules can use `init.luau`, such as `lib/json/init.luau` for
`require("json")`. The build rejects duplicate module names.

```luau
-- lib/path.luau
local path = {}

function path.join(left, right)
    return left .. "/" .. right
end

return path
```

```luau
local path = require("path")
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

`require` caches module return values. Modules must return the value they
export. Built-in modules are available as `require("fs")`, `require("http")`,
`require("process")`, and `require("task")`.

The entry script may use `:await()`. Modules loaded by `require` execute
synchronously while loading, so asynchronous work belongs in exported
functions or in a promise returned by the module.

This runtime runs trusted local code. Its file APIs are not sandboxed.

## Next steps

- HTTP server and TCP APIs
- package resolution and a package manager
- permissions and isolated script execution
