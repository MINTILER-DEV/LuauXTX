local url = {}

local function percent_encode(value, keep_slash)
    return (tostring(value):gsub("[^%w%-_%.~]", function(character)
        if keep_slash and character == "/" then
            return character
        end
        return string.format("%%%02X", string.byte(character))
    end))
end

function url.encode(value)
    return percent_encode(value, false)
end

function url.decode(value)
    return (tostring(value):gsub("%%(%x%x)", function(hex)
        return string.char(tonumber(hex, 16))
    end))
end

function url.parse(value)
    value = tostring(value)
    local scheme, remainder = value:match("^(%a[%w+.-]*):%/%/(.*)$")
    if not scheme or not remainder then
        return nil, "URL must include a scheme and authority"
    end
    local authority, tail = remainder:match("^([^/%?#]*)(.*)$")
    local path, query, fragment = tail, "", ""
    local fragment_start = path:find("#", 1, true)
    if fragment_start then
        fragment = path:sub(fragment_start)
        path = path:sub(1, fragment_start - 1)
    end
    local query_start = path:find("?", 1, true)
    if query_start then
        query = path:sub(query_start)
        path = path:sub(1, query_start - 1)
    end
    local host, port = authority:match("^%[([^%]]+)%]:(%d+)$")
    if not host then
        host, port = authority:match("^([^:]+):(%d+)$")
    end
    host = host or authority
    return {
        href = value,
        protocol = scheme .. ":",
        scheme = scheme,
        host = authority,
        hostname = host,
        port = port and tonumber(port) or nil,
        pathname = path ~= "" and path or "/",
        search = query,
        hash = fragment,
    }
end

function url.format(parts)
    local scheme = assert(parts.scheme or (parts.protocol and parts.protocol:gsub(":$", "")), "URL scheme is required")
    local authority = parts.host or parts.hostname
    assert(authority, "URL host is required")
    if parts.port and not parts.host then authority ..= ":" .. tostring(parts.port) end
    local path = parts.pathname or "/"
    return scheme .. "://" .. authority .. path .. (parts.search or "") .. (parts.hash or "")
end

function url.resolve(base, relative)
    if relative:match("^%a[%w+.-]*://") then return relative end
    local parsed, error_message = url.parse(base)
    if not parsed then return nil, error_message end
    if relative:sub(1, 1) == "/" then
        return parsed.protocol .. "//" .. parsed.host .. relative
    end
    local directory = parsed.pathname:match("^(.*)/") or "/"
    if directory:sub(-1) ~= "/" then directory ..= "/" end
    return parsed.protocol .. "//" .. parsed.host .. directory .. relative
end

return url
