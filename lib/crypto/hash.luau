local native = ...
local hash = {}

function hash.sha256(input)
    return native.sha256(tostring(input))
end

return hash
