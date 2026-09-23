use wasm_bindgen::prelude::*;

/// Run a standalone Luau source string in the shared LuauXTX VM.
#[wasm_bindgen]
pub fn run(source: &str) -> Result<String, JsValue> {
    luauxtx::run(source).map_err(|error| JsValue::from_str(&error))
}

#[wasm_bindgen]
pub fn document_title() -> String {
    web_sys::window()
        .and_then(|window| window.document())
        .map(|document| document.title())
        .unwrap_or_default()
}

#[wasm_bindgen]
pub fn set_text(selector: &str, text: &str) -> Result<(), JsValue> {
    let document = web_sys::window()
        .and_then(|window| window.document())
        .ok_or_else(|| JsValue::from_str("document is unavailable"))?;
    let element = document
        .query_selector(selector)?
        .ok_or_else(|| JsValue::from_str("element was not found"))?;
    element.set_text_content(Some(text));
    Ok(())
}
