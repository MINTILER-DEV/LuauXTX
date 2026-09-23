local response = {}
response.__index = response

function response.new()
    return setmetatable({ _status = 200, _headers = {}, _body = "" }, response)
end

function response:status(code)
    self._status = assert(tonumber(code), "status must be a number")
    return self
end

function response:header(name, value)
    self._headers[tostring(name)] = tostring(value)
    return self
end

function response:send(body)
    self._body = tostring(body or "")
    return self
end

function response:json(value)
    self:header("Content-Type", "application/json")
    self:send(require("serde/json").encode(value))
    return self
end

function response:result()
    return self._status, self._headers, self._body
end

return response
