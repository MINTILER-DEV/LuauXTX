local native = ...
local Promise = require("async/promise")
local tcp, Socket = {}, {}
Socket.__index = Socket
function Socket:send(data) return Promise.new(function(resolve, reject) native.tcp_send(self._socket, data, resolve, reject) end) end
function Socket:receive() return Promise.new(function(resolve, reject) native.tcp_receive(self._socket, resolve, reject) end) end
function tcp.connect(host, port)
    return Promise.new(function(resolve, reject) native.tcp_connect(host, port, function(socket) resolve(setmetatable({ _socket = socket }, Socket)) end, reject) end)
end
function tcp.listen(port, handler)
    return native.tcp_listen(port, function(socket, host, remote_port) handler(setmetatable({ _socket = socket }, Socket), host, remote_port) end)
end
return tcp
