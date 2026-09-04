//! The gravity example in a single native window, with an egui interface.
//!
//! The app crate is the same one the web and Tauri hosts run — this binary only chooses
//! a different interface for it.

fn main() -> eframe::Result {
    env_logger::init();
    fathom_shell::run_native::<gravity::Gravity>("Gravity - fathom")
}
