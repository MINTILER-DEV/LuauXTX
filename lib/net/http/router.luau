local Router = {}
Router.__index = Router

local function match(pattern, path)
    local pattern_parts, path_parts = {}, {}
    for part in pattern:gmatch("[^/]+") do table.insert(pattern_parts, part) end
    for part in path:gmatch("[^/]+") do table.insert(path_parts, part) end
    if #pattern_parts ~= #path_parts then return nil end

    local params = {}
    for index, part in ipairs(pattern_parts) do
        local value = path_parts[index]
        if part:sub(1, 1) == ":" then
            params[part:sub(2)] = value
        elseif part ~= value then
            return nil
        end
    end
    return params
end

function Router.new()
    return setmetatable({ routes = {} }, Router)
end

function Router:route(method, pattern, handler)
    assert(type(handler) == "function", "route handler must be a function")
    table.insert(self.routes, { method = method:upper(), pattern = pattern, handler = handler })
    return self
end

function Router:get(pattern, handler) return self:route("GET", pattern, handler) end
function Router:post(pattern, handler) return self:route("POST", pattern, handler) end
function Router:put(pattern, handler) return self:route("PUT", pattern, handler) end
function Router:delete(pattern, handler) return self:route("DELETE", pattern, handler) end

function Router:handle(req, res)
    for _, route in ipairs(self.routes) do
        if route.method == req.method then
            local params = match(route.pattern, req.path)
            if params then
                req.params = params
                return route.handler(req, res)
            end
        end
    end
    res:status(404):send("Not Found")
end

return Router
