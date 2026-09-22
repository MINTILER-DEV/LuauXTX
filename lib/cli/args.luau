local args = {}

function args.parse(values)
    local result = { _ = {} }
    values = values or process.argv
    for index = 1, #values do
        local value = values[index]
        if value:sub(1, 2) == "--" then
            local key, assigned = value:sub(3):match("^([^=]+)=(.*)$")
            key = key or value:sub(3)
            result[key] = assigned ~= nil and assigned or true
        elseif value:sub(1, 1) == "-" and #value > 1 then
            for key in value:sub(2):gmatch(".") do result[key] = true end
        else
            table.insert(result._, value)
        end
    end
    return result
end

return args
