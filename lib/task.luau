local native = ...

local task = {}

function task.wait(seconds)
    return native.sleep(seconds)
end

function task.spawn(callback, ...)
    return native.spawn(callback, ...)
end

function task.defer(callback, ...)
    return native.defer(callback, ...)
end

function task.delay(seconds, callback, ...)
    return native.delay(seconds, callback, ...)
end

return task
