//! Shared LuauXTX runtime entrypoints.
//!
//! Native host integrations live in `native`; browser hosts use `run` through
//! the `web` crate. This keeps the VM dependency and bundled Luau sources in
//! one package while platform bindings remain target-specific.

#[cfg(not(target_arch = "wasm32"))]
pub mod native;

#[cfg(not(target_arch = "wasm32"))]
pub use native::cli_main;

/// Execute a standalone Luau chunk using the embedded Luau VM.
///
/// Browser bindings install their own host APIs around this same VM. Values are
/// rendered as strings so this minimal portable entrypoint has no JS-specific
/// types in its public API.
pub fn run(source: &str) -> Result<String, String> {
    let lua = mlua::Lua::new();
    let value: mlua::Value = lua.load(source).eval().map_err(|error| error.to_string())?;
    match value {
        mlua::Value::Nil => Ok(String::new()),
        mlua::Value::String(value) => value.to_str().map(|value| value.to_owned()).map_err(|error| error.to_string()),
        value => Ok(format!("{value:?}")),
    }
}
