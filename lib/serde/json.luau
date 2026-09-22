local json = {}

-- Helper to check if a table is acting as an array (sequential integer keys)
local function isArray(tbl)
	local maxIdx = 0
	local count = 0
	for k, _ in pairs(tbl) do
		if typeof(k) ~= "number" or k <= 0 or math.floor(k) ~= k then
			return false
		end
		if k > maxIdx then maxIdx = k end
		count += 1
	end
	return maxIdx == count
end

-- Escape string characters for JSON compliance
local function escapeString(str: string): string
	local escapeMap = {
		['"'] = '\\"', ['\\'] = '\\\\', ['\b'] = '\\b',
		['\f'] = '\\f', ['\n'] = '\\n', ['\r'] = '\\r', ['\t'] = '\\t'
	}
	return '"' .. str:gsub('["\\\b\f\n\r\t]', escapeMap) .. '"'
end

-- ==========================================
-- ENCODER
-- ==========================================
function json.encode(val: any): string
	local t = typeof(val)
	if t == "string" then
		return escapeString(val)
	elseif t == "number" or t == "boolean" then
		return tostring(val)
	elseif t == "nil" then
		return "null"
	elseif t == "table" then
		if isArray(val) then
			local parts = {}
			for i, v in ipairs(val) do
				table.insert(parts, json.encode(v))
			end
			return "[" .. table.concat(parts, ",") .. "]"
		else
			local parts = {}
			for k, v in pairs(val) do
				if typeof(k) ~= "string" then
					error("json.encode error: Object keys must be strings")
				end
				table.insert(parts, escapeString(k) .. ":" .. json.encode(v))
			end
			return "{" .. table.concat(parts, ",") .. "}"
		end
	else
		error("json.encode error: Unsupported type '" .. t .. "'")
	end
end

-- ==========================================
-- DECODER
-- ==========================================
local function skipWhitespace(str: string, pos: number): number
	return str:find("%S", pos) or #str + 1
end

local parseValue -- Forward declaration

local function parseObject(str: string, pos: number): (table, number)
	local obj = {}
	pos = skipWhitespace(str, pos + 1) -- Skip open brace '{'
	
	if str:sub(pos, pos) == "}" then
		return obj, pos + 1
	end

	while true do
		if str:sub(pos, pos) ~= '"' then
			error("json.decode error: Expected string key at position " .. pos)
		end
		
		local key, nextPos = parseValue(str, pos)
		pos = skipWhitespace(str, nextPos)
		
		if str:sub(pos, pos) ~= ":" then
			error("json.decode error: Expected ':' at position " .. pos)
		end
		
		local val
		val, pos = parseValue(str, skipWhitespace(str, pos + 1))
		obj[key] = val
		pos = skipWhitespace(str, pos)
		
		local chr = str:sub(pos, pos)
		if chr == "}" then
			return obj, pos + 1
		elseif chr == "," then
			pos = skipWhitespace(str, pos + 1)
		else
			error("json.decode error: Expected ',' or '}' at position " .. pos)
		end
	end
end

local function parseArray(str: string, pos: number): (table, number)
	local arr = {}
	pos = skipWhitespace(str, pos + 1) -- Skip open bracket '['
	
	if str:sub(pos, pos) == "]" then
		return arr, pos + 1
	end

	while true do
		local val
		val, pos = parseValue(str, pos)
		table.insert(arr, val)
		pos = skipWhitespace(str, pos)
		
		local chr = str:sub(pos, pos)
		if chr == "]" then
			return arr, pos + 1
		elseif chr == "," then
			pos = skipWhitespace(str, pos + 1)
		else
			error("json.decode error: Expected ',' or ']' at position " .. pos)
		end
	end
end

local function parseString(str: string, pos: number): (string, number)
	local endPos = pos
	while true do
		endPos = str:find('"', endPos + 1)
		if not endPos then error("json.decode error: Unterminated string") end
		-- Check for an unescaped quote
		local backslashes = 0
		for i = endPos - 1, pos, -1 do
			if str:sub(i, i) == "\\" then backslashes += 1 else break end
		end
		if backslashes % 2 == 0 then break end
	end
	
	local rawStr = str:sub(pos + 1, endPos - 1)
	-- Simple unescaping
	local unescapeMap = { ['\\"'] = '"', ['\\\\'] = '\\', ['\\/'] = '/', ['\\b'] = '\b', ['\\f'] = '\f', ['\\n'] = '\n', ['\\r'] = '\r', ['\\t'] = '\t' }
	local processedStr = rawStr:gsub("\\[\"\\/bfnrt]", unescapeMap)
	return processedStr, endPos + 1
end

function parseValue(str: string, pos: number): (any, number)
	local chr = str:sub(pos, pos)
	
	if chr == '"' then
		return parseString(str, pos)
	elseif chr == "{" then
		return parseObject(str, pos)
	elseif chr == "[" then
		return parseArray(str, pos)
	elseif str:sub(pos, pos + 3) == "true" then
		return true, pos + 4
	elseif str:sub(pos, pos + 4) == "false" then
		return false, pos + 5
	elseif str:sub(pos, pos + 3) == "null" then
		return nil, pos + 4
	else
		-- Treat as a number
		local numStr = str:match("^([%-?%d%.eE%+%-]+)", pos)
		local num = tonumber(numStr)
		if not num then
			error("json.decode error: Unexpected token at position " .. pos)
		end
		return num, pos + #numStr
	end
end

function json.decode(str: string): any
	local pos = skipWhitespace(str, 1)
	local val, endPos = parseValue(str, pos)
	if skipWhitespace(str, endPos) <= #str then
		error("json.decode error: Trailing garbage bytes")
	end
	return val
end

return json
