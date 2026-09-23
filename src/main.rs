#[cfg(not(target_arch = "wasm32"))]
fn main() {
    luauxtx::cli_main();
}

#[cfg(target_arch = "wasm32")]
fn main() {}
