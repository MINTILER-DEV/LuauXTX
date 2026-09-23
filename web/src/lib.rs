use wasm_bindgen::prelude::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;

/// Run a standalone Luau source string in the shared LuauXTX VM.
#[wasm_bindgen]
pub fn run(source: &str) -> Result<String, JsValue> {
    let lua = Box::leak(Box::new(mlua::Lua::new()));
    let native = lua.create_table().map_err(|error| JsValue::from_str(&error.to_string()))?;
    native.set("web_document_get", lua.create_function(|_, selector: String| {
        Ok(web_sys::window().and_then(|window| window.document()).and_then(|document| document.query_selector(&selector).ok().flatten()).is_some())
    }).map_err(|error| JsValue::from_str(&error.to_string()))?).map_err(|error| JsValue::from_str(&error.to_string()))?;
    native.set("web_element_set_text", lua.create_function(|_, (selector, text): (String, String)| {
        let document = web_sys::window().and_then(|window| window.document()).ok_or_else(|| mlua::Error::RuntimeError("document is unavailable".into()))?;
        let element = document.query_selector(&selector).map_err(|_| mlua::Error::RuntimeError("invalid selector".into()))?.ok_or_else(|| mlua::Error::RuntimeError("element was not found".into()))?;
        element.set_text_content(Some(&text)); Ok(())
    }).map_err(|error| JsValue::from_str(&error.to_string()))?).map_err(|error| JsValue::from_str(&error.to_string()))?;
    native.set("web_element_on", lua.create_function(|_, (selector, event, callback): (String, String, mlua::Function)| {
        let document = web_sys::window().and_then(|window| window.document()).ok_or_else(|| mlua::Error::RuntimeError("document is unavailable".into()))?;
        let element = document.query_selector(&selector).map_err(|_| mlua::Error::RuntimeError("invalid selector".into()))?.ok_or_else(|| mlua::Error::RuntimeError("element was not found".into()))?;
        let listener = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| { let _ = callback.call::<()>(()); });
        element.add_event_listener_with_callback(&event, listener.as_ref().unchecked_ref()).map_err(|_| mlua::Error::RuntimeError("could not add event listener".into()))?;
        listener.forget(); Ok(())
    }).map_err(|error| JsValue::from_str(&error.to_string()))?).map_err(|error| JsValue::from_str(&error.to_string()))?;
    luauxtx::run_with_host(lua, source, native).map_err(|error| JsValue::from_str(&error))
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
