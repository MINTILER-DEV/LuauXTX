local native = ...
local Promise = require("async/promise")
local udp, Socket = {}, {}
Socket.__index = Socket
function Socket:send(data, host, port) return Promise.new(function(resolve, reject) native.udp_send(self._socket, data, host, port, resolve, reject) end) end
function Socket:receive()
    return Promise.new(function(resolve, reject) native.udp_receive(self._socket, function(data, host, port) resolve({ data = data, host = host, port = port }) end, reject) end)
end
function udp.bind(port, host)
    local socket, actual_port = native.udp_bind(host or "127.0.0.1", port or 0)
    return setmetatable({ _socket = socket, port = actual_port }, Socket)
end
return udp
