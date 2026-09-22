local native = ...
local Promise = require("async/promise")

local http = {}

function http.fetch(url)
    return Promise.new(function(resolve, reject)
        native.http_get(url, function(status, body)
            resolve({
                status = status,
                ok = status >= 200 and status < 300,
                body = body,
                text = function(self) return self.body end,
            })
        end, reject)
    end)
end

return http
