local base64 = {}
local alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"

function base64.encode(input)
    input = tostring(input)
    local output = {}
    for index = 1, #input, 3 do
        local first, second, third = input:byte(index, index + 2)
        table.insert(output, alphabet:sub(math.floor(first / 4) + 1, math.floor(first / 4) + 1))
        table.insert(output, alphabet:sub((first % 4) * 16 + math.floor((second or 0) / 16) + 1, (first % 4) * 16 + math.floor((second or 0) / 16) + 1))
        table.insert(output, second and alphabet:sub((second % 16) * 4 + math.floor((third or 0) / 64) + 1, (second % 16) * 4 + math.floor((third or 0) / 64) + 1) or "=")
        table.insert(output, third and alphabet:sub(third % 64 + 1, third % 64 + 1) or "=")
    end
    return table.concat(output)
end

function base64.decode(input)
    input = tostring(input):gsub("%s", "")
    if #input % 4 ~= 0 or input:find("[^A-Za-z0-9+/=]") then error("invalid base64 input", 2) end
    local output = {}
    for index = 1, #input, 4 do
        local a, b, c, d = input:sub(index, index), input:sub(index + 1, index + 1), input:sub(index + 2, index + 2), input:sub(index + 3, index + 3)
        local first, second = alphabet:find(a, 1, true), alphabet:find(b, 1, true)
        local third = c == "=" and nil or alphabet:find(c, 1, true)
        local fourth = d == "=" and nil or alphabet:find(d, 1, true)
        if not first or not second or (c ~= "=" and not third) or (d ~= "=" and not fourth) or (c == "=" and d ~= "=") then error("invalid base64 input", 2) end
        first, second = first - 1, second - 1
        table.insert(output, string.char(first * 4 + math.floor(second / 16)))
        if third then
            third -= 1
            table.insert(output, string.char((second % 16) * 16 + math.floor(third / 4)))
            if fourth then table.insert(output, string.char((third % 4) * 64 + fourth - 1)) end
        end
    end
    return table.concat(output)
end

return base64
