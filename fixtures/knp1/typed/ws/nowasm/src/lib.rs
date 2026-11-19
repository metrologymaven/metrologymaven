//! AC-4: does not compile for wasm32.

#[cfg(target_arch = "wasm32")]
compile_error!("typed-nowasm does not build for wasm32");

pub fn ok() {}
