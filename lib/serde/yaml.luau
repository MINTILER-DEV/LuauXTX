local yaml = {}

-- ==========================================
-- HELPER FUNCTIONS
-- ==========================================
local function trim(s: string): string
	return s:match("^%s*(.-)%s*$")
end

local function countIndent(s: string): number
	local _, count = s:find("^%s*")
	return count or 0
end

local function parsePrimitive(valStr: string): any
	valStr = trim(valStr)
	if valStr == "" then return nil end
	if valStr == "true" then return true end
	if valStr == "false" then return false end
	if valStr == "null" or valStr == "~" then return nil end
	
	-- Strings with quotes
	if (valStr:sub(1, 1) == '"' and valStr:sub(-1, -1) == '"') or
	   (valStr:sub(1, 1) == "'" and valStr:sub(-1, -1) == "'") then
		return valStr:sub(2, -2)
	end
	
	-- Numeric checking
	local num = tonumber(valStr)
	if num then return num end
	
	return valStr
end

-- ==========================================
-- DECODER (YAML -> Luau Table)
-- ==========================================
function yaml.decode(str: string): table
	local root = {}
	local stack = { { indent = -1, node = root, isArray = false } }
	
	-- Clear comments and clean up lines
	local lines = {}
	for line in str:gmatch("[^\r\n]+") do
		local cleaned = line:gsub("%s*#.*$", "") -- strip inline comments
		if trim(cleaned) ~= "" then
			table.insert(lines, { raw = line, cleaned = cleaned, indent = countIndent(line) })
		end
	end

	for _, lineData in ipairs(lines) do
		local line = trim(lineData.cleaned)
		local indent = lineData.indent

		-- Pop stack until we find the parent context matching this indentation level
		while #stack > 1 and indent <= stack[#stack].indent do
			table.remove(stack)
		end
		
		local parent = stack[#stack].node

		-- Case 1: Sequence Item (Array entry starts with "- ")
		if line:sub(1, 2) == "- " or line == "-" then
			local remaining = trim(line:sub(3))
			
			-- If parent is not flagged as an array yet, transform its design context
			if #stack > 1 and not stack[#stack].isArray then
				stack[#stack].isArray = true
			end
			
			if remaining:find(":") then
				-- Inline dictionary inside an array item: "- key: value"
				local k, v = remaining:match("^([^:]+):%s*(.*)$")
				local subNode = {}
				table.insert(parent, subNode)
				subNode[trim(k)] = parsePrimitive(v)
				table.insert(stack, { indent = indent + 2, node = subNode, isArray = false })
			elseif remaining ~= "" then
				table.insert(parent, parsePrimitive(remaining))
			else
				-- Nested object follows on next lines
				local subNode = {}
				table.insert(parent, subNode)
				table.insert(stack, { indent = indent + 2, node = subNode, isArray = false })
			end

		-- Case 2: Key-Value Mapping Pair ("key: value")
		elseif line:find(":") then
			local k, v = line:match("^([^:]+):%s*(.*)$")
			k = trim(k)
			v = trim(v)
			
			if v ~= "" then
				parent[k] = parsePrimitive(v)
			else
				-- Nesting a structured block underneath this key
				local subNode = {}
				parent[k] = subNode
				table.insert(stack, { indent = indent, node = subNode, isArray = false })
			end
		end
	end

	return root
end

-- ==========================================
-- ENCODER (Luau Table -> YAML)
-- ==========================================
local function isArray(tbl: table): boolean
	local maxIdx = 0
	local count = 0
	for k, _ in pairs(tbl) do
		if typeof(k) ~= "number" or k <= 0 or math.floor(k) ~= k then return false end
		if k > maxIdx then maxIdx = k end
		count += 1
	end
	return maxIdx == count
end

function yaml.encode(tbl: table, indentLevel: number?): string
	local level = indentLevel or 0
	local indentStr = string.rep("  ", level)
	local lines = {}

	if isArray(tbl) then
		for _, v in ipairs(tbl) do
			if typeof(v) == "table" then
				table.insert(lines, indentStr .. "-")
				table.insert(lines, yaml.encode(v, level + 1))
			else
				local valStr = typeof(v) == "string" and '"' .. v .. '"' or tostring(v)
				table.insert(lines, indentStr .. "- " .. valStr)
			end
		end
	else
		for k, v in pairs(tbl) do
			if typeof(v) == "table" then
				table.insert(lines, indentStr .. tostring(k) .. ":")
				table.insert(lines, yaml.encode(v, level + 1))
			else
				local valStr = typeof(v) == "string" and '"' .. v .. '"' or tostring(v)
				table.insert(lines, indentStr .. tostring(k) .. ": " .. valStr)
			end
		end
	end

	return table.concat(lines, "\n")
end

return yaml
