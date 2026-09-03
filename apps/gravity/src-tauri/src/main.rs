//! The desktop shell for the gravity example.
//!
//! Everything here is boilerplate on purpose: the app is the same `gravity` crate the
//! web target compiles to wasm, and the interface is the same React panel. This binary
//! only wires the two together.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    tauri::Builder::default()
        .setup(fathom_native::setup::<gravity::Gravity>)
        .invoke_handler(fathom_native::handlers!())
        .run(tauri::generate_context!())
        .expect("gravity failed to start");
}
