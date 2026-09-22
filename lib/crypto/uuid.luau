local native = ...
local uuid = {}

function uuid.v4()
    return native.uuid_v4()
end

return uuid
