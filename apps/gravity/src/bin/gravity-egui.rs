//! The gravity example in a single native window, with an egui interface.
//!
//! The app crate is the same one the browser runs — this binary only picks the desktop
//! backend for it. The browser entry point is `gravity::start`, in the library.

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    env_logger::init();
    fathom_shell::run_native::<gravity::Gravity>("Gravity - fathom")
}

/// The desktop binary is not built for the browser; `gravity::start` is the entry there.
#[cfg(target_arch = "wasm32")]
fn main() {}
