local Promise = {}
Promise.__index = Promise

function Promise._new()
    return setmetatable({ _state = "pending", _values = nil, _handlers = {} }, Promise)
end

function Promise:_settle(state, ...)
    if self._state ~= "pending" then
        return
    end

    self._state = state
    self._values = table.pack(...)
    for _, handler in ipairs(self._handlers) do
        task.defer(handler)
    end
    self._handlers = {}
end

function Promise:_resolve(...)
    self:_settle("fulfilled", ...)
end

function Promise:_reject(reason)
    self:_settle("rejected", reason)
end

function Promise.new(executor)
    local promise = Promise._new()
    local ok, reason = pcall(executor, function(...) promise:_resolve(...) end, function(error) promise:_reject(error) end)
    if not ok then
        promise:_reject(reason)
    end
    return promise
end

function Promise.resolve(...)
    local promise = Promise._new()
    promise:_resolve(...)
    return promise
end

function Promise.reject(reason)
    local promise = Promise._new()
    promise:_reject(reason)
    return promise
end

function Promise:andThen(on_fulfilled, on_rejected)
    local next_promise = Promise._new()

    local function handle()
        local values = self._values
        local callback = self._state == "fulfilled" and on_fulfilled or on_rejected
        if type(callback) ~= "function" then
            next_promise:_settle(self._state, table.unpack(values, 1, values.n))
            return
        end

        local result = table.pack(pcall(callback, table.unpack(values, 1, values.n)))
        if not result[1] then
            next_promise:_reject(result[2])
            return
        end

        local first = result[2]
        if type(first) == "table" and getmetatable(first) == Promise then
            first:andThen(
                function(...) next_promise:_resolve(...) end,
                function(reason) next_promise:_reject(reason) end
            )
        else
            next_promise:_resolve(table.unpack(result, 2, result.n))
        end
    end

    if self._state == "pending" then
        table.insert(self._handlers, handle)
    else
        task.defer(handle)
    end
    return next_promise
end

function Promise:catch(on_rejected)
    return self:andThen(nil, on_rejected)
end

function Promise:await()
    while self._state == "pending" do
        task.wait(0.001)
    end
    if self._state == "rejected" then
        error(self._values[1], 2)
    end
    return table.unpack(self._values, 1, self._values.n)
end

function Promise.all(values)
    local result = Promise._new()
    local remaining = #values
    local resolved = {}
    if remaining == 0 then
        result:_resolve(resolved)
        return result
    end

    for index, value in ipairs(values) do
        if type(value) == "table" and getmetatable(value) == Promise then
            value:andThen(
                function(resolved_value)
                    resolved[index] = resolved_value
                    remaining -= 1
                    if remaining == 0 then result:_resolve(resolved) end
                end,
                function(reason) result:_reject(reason) end
            )
        else
            resolved[index] = value
            remaining -= 1
            if remaining == 0 then result:_resolve(resolved) end
        end
    end
    return result
end

return Promise
