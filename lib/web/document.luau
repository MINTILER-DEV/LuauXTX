-- Browser-only module surface. The wasm host exposes document operations.
local document = {}
function document:get(selector)
    error("web/document requires the browser wasm host", 2)
end
return document
