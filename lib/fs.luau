local native = ...
local Promise = require("promise")

local fs = {}

function fs.read(path)
    return Promise.new(function(resolve, reject)
        native.fs_read(path, resolve, reject)
    end)
end

function fs.write(path, contents)
    return Promise.new(function(resolve, reject)
        native.fs_write(path, contents, resolve, reject)
    end)
end

function fs.readSync(path)
    return native.fs_read_sync(path)
end

function fs.writeSync(path, contents)
    return native.fs_write_sync(path, contents)
end

function fs.exists(path)
    return native.fs_exists(path)
end

return fs
