local native = ...
local Request = require("net/http/request")
local Response = require("net/http/response")

local http = {}

function http.createServer(handler)
    assert(type(handler) == "function" or (type(handler) == "table" and type(handler.handle) == "function"), "createServer expects a handler function or router")

    local server = {}

    function server:listen(port)
        return native.http_listen(assert(tonumber(port), "port must be a number"), function(method, target, body)
            local req = Request.new(method, target, {}, body)
            local res = Response.new()
            if type(handler) == "function" then
                handler(req, res)
            else
                handler:handle(req, res)
            end
            return res:result()
        end)
    end

    return server
end

return http
