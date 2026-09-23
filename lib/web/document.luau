local native = ...
local document = {}
function document:get(selector)
    assert(native.web_document_get(selector), "element was not found")
    local element = {}
    function element:setText(text) native.web_element_set_text(selector, tostring(text)) return self end
    function element:on(event, callback) native.web_element_on(selector, event, callback) return self end
    return element
end
return document
