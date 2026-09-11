//! ECP-Life in a single native window, with an egui interface.
//!
//! The app crate is the same one the browser runs — this binary only picks the desktop
//! backend for it. The browser entry point is `ecp_life2::start`, in the library.

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    env_logger::init();
    fathom_shell::run_native::<ecp_life2::EcpLife>("ECP-Life - fathom")
}

/// The desktop binary is not built for the browser; `ecp_life2::start` is the entry there.
#[cfg(target_arch = "wasm32")]
fn main() {}
