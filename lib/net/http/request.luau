local request = {}

function request.new(method, target, headers, body)
    local path, query = target:match("^([^?]*)(.*)$")
    return {
        method = method,
        path = path ~= "" and path or "/",
        url = target,
        query = query or "",
        headers = headers or {},
        body = body or "",
    }
end

return request
