local toml = {}

-- ==========================================
-- HELPER FUNCTIONS
-- ==========================================
local function trim(s: string): string
	return s:match("^%s*(.-)%s*$")
end

local function parsePrimitive(valStr: string): any
	valStr = trim(valStr)
	if valStr == "true" then return true end
	if valStr == "false" then return false end
	
	-- String matching
	if valStr:sub(1, 1) == '"' and valStr:sub(-1, -1) == '"' then
		return valStr:sub(2, -2):gsub("\\\"", '"'):gsub("\\\\", "\\")
	end
	if valStr:sub(1, 1) == "'" and valStr:sub(-1, -1) == "'" then
		return valStr:sub(2, -2)
	end
	
	-- Number matching
	local num = tonumber(valStr)
	if num then return num end

	return valStr -- Fallback raw string if unquoted/malformed
end

local function navigateToSection(root: table, sectionPath: string): table
	local current = root
	for part in sectionPath:gmatch("[^%.]+") do
		part = trim(part)
		if not current[part] then
			current[part] = {}
		end
		current = current[part]
	end
	return current
end

-- ==========================================
-- DECODER (TOML -> Luau Table)
-- ==========================================
function toml.decode(str: string): table
	local root = {}
	local currentSection = root

	for line in str:gmatch("[^\r\n]+") do
		line = trim(line)
		
		-- Skip empty lines and comments
		if line == "" or line:sub(1, 1) == "#" then
			continue
		end

		-- Parse Section Header: [section.name]
		if line:sub(1, 1) == "[" and line:sub(-1, -1) == "]" then
			local sectionPath = line:sub(2, -2)
			currentSection = navigateToSection(root, sectionPath)
			continue
		end

		-- Parse Key-Value pair: key = value
		local key, valStr = line:match("^([^=]+)=(.-)$")
		if key and valStr then
			key = trim(key)
			-- Strip quotes from keys if present
			if (key:sub(1,1) == '"' and key:sub(-1,-1) == '"') or (key:sub(1,1) == "'" and key:sub(-1,-1) == "'") then
				key = key:sub(2, -2)
			end
			currentSection[key] = parsePrimitive(valStr)
		end
	end

	return root
end

-- ==========================================
-- ENCODER (Luau Table -> TOML)
-- ==========================================
local function serializeValue(val: any): string
	local t = typeof(val)
	if t == "boolean" or t == "number" then
		return tostring(val)
	elseif t == "string" then
		return '"' .. val:gsub('\\', '\\\\'):gsub('"', '\\"') .. '"'
	else
		error("toml.encode error: Unsupported type '" .. t .. "'")
	end
end

function toml.encode(tbl: table, prefix: string?): string
	local lines = {}
	local subTables = {}
	local currentPrefix = prefix or ""

	-- Process primitives first, postpone sub-tables/dictionaries to the bottom of the section
	for k, v in pairs(tbl) do
		if typeof(v) == "table" then
			subTables[k] = v
		else
			table.insert(lines, string.format("%s = %s", tostring(k), serializeValue(v)))
		end
	end

	-- Process sub-tables natively nested under section blocks
	for k, v in pairs(subTables) do
		local sectionName = currentPrefix == "" and tostring(k) or currentPrefix .. "." .. tostring(k)
		table.insert(lines, string.format("\n[%s]", sectionName))
		table.insert(lines, toml.encode(v, sectionName))
	end

	return table.concat(lines, "\n")
end

return toml
