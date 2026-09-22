local path = {}

local function normalize(value)
    local absolute = value:sub(1, 1) == "/"
    local parts = {}
    for part in value:gmatch("[^/\\]+") do
        if part == ".." then
            if #parts > 0 and parts[#parts] ~= ".." then table.remove(parts) elseif not absolute then table.insert(parts, part) end
        elseif part ~= "." and part ~= "" then table.insert(parts, part) end
    end
    local result = table.concat(parts, "/")
    if absolute then result = "/" .. result end
    return result ~= "" and result or (absolute and "/" or ".")
end

function path.join(...)
    return normalize(table.concat({ ... }, "/"))
end

function path.normalize(value) return normalize(value) end
function path.isAbsolute(value) return tostring(value):sub(1, 1) == "/" or tostring(value):match("^%a:[/\\]") ~= nil end
function path.basename(value) return tostring(value):gsub("[/\\]+$", ""):match("([^/\\]+)$") or "" end
function path.dirname(value)
    local directory = tostring(value):gsub("[/\\]+$", ""):match("^(.*)[/\\][^/\\]+$")
    return directory and (directory ~= "" and directory or "/") or "."
end
function path.extname(value)
    local name = path.basename(value)
    return name:match("(%.[^%.]+)$") or ""
end

return path
