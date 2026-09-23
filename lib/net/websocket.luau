local native = ...
local Promise = require("async/promise")

local websocket = {}
local Socket = {}
Socket.__index = Socket

function Socket:send(message)
    return Promise.new(function(resolve, reject)
        native.websocket_send_text(self._socket, tostring(message), resolve, reject)
    end)
end

function Socket:sendBinary(bytes)
    return Promise.new(function(resolve, reject)
        native.websocket_send_binary(self._socket, bytes, resolve, reject)
    end)
end

function Socket:receive()
    return Promise.new(function(resolve, reject)
        native.websocket_receive(self._socket, function(kind, data)
            if kind == nil then resolve(nil) else resolve({ type = kind, data = data }) end
        end, reject)
    end)
end

function Socket:close()
    return Promise.new(function(resolve)
        native.websocket_close(self._socket, resolve)
    end)
end

function websocket.connect(url)
    return Promise.new(function(resolve, reject)
        native.websocket_connect(url, function(socket)
            resolve(setmetatable({ _socket = socket }, Socket))
        end, reject)
    end)
end

function websocket.listen(port, handler)
    assert(type(handler) == "function", "websocket.listen expects a connection handler")
    return native.websocket_listen(port, function(socket)
        handler(setmetatable({ _socket = socket }, Socket))
    end)
end

return websocket
