//! The native host: a fathom app behind a transparent Tauri webview.
//!
//! The interface is the same React panel the web target runs. The difference is only
//! where the pixels come from: a wgpu child window owned by the Tauri window, positioned
//! by the rects the interface reports. See [`child`] for why that shape was chosen.
//!
//! Wiring an app up takes two lines in its Tauri binary:
//!
//! ```ignore
//! tauri::Builder::default()
//!     .setup(fathom_native::setup::<Gravity>)
//!     .invoke_handler(fathom_native::handlers())
//! ```

mod child;
mod render;

use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use fathom_core::{App, FrameStats};
use serde::Serialize;
use tauri::Manager;

use render::{Boot, BootResult, Message};

/// How long the interface waits for the GPU to come up before giving up on it.
const BOOT_TIMEOUT: Duration = Duration::from_secs(10);

/// The handle the commands talk to. Deliberately not generic: Tauri commands cannot be,
/// so the app type is erased here and lives only inside the render thread.
pub struct FathomState {
    sender: Sender<Message>,
    stats: Arc<Mutex<FrameStats>>,
    boot: BootResult,
}

impl FathomState {
    fn send(&self, message: Message) {
        // A closed channel means the render thread is gone; there is nothing useful to
        // do about it from a command, and panicking would take the window with it.
        let _ = self.sender.send(message);
    }

    /// Block until the render thread reports success or failure.
    fn await_boot(&self) -> Result<Boot, String> {
        let deadline = Instant::now() + BOOT_TIMEOUT;
        loop {
            if let Some(result) = self.boot.lock().unwrap().clone() {
                return result;
            }
            if Instant::now() > deadline {
                return Err("the GPU did not start within ten seconds".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InitReply {
    /// The app's parameter and command schema, as JSON.
    descriptor: String,
    param_byte_length: usize,
    adapter_info: String,
}

/// Start the render thread and create the child window. Pass to `Builder::setup`.
pub fn setup<A: App>(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let window = app
        .get_webview_window("main")
        .ok_or("fathom needs a window labelled \"main\"")?;

    // Must happen here: a child window belongs to the thread that creates it, and setup
    // runs on the thread that pumps the parent window's messages.
    let parent = window_handle(&window)?;
    let surface = child::ChildSurface::create(parent)?;

    let (sender, receiver) = mpsc::channel();
    let stats = Arc::new(Mutex::new(FrameStats::default()));
    let boot: BootResult = Arc::new(Mutex::new(None));

    let thread_stats = Arc::clone(&stats);
    let thread_boot = Arc::clone(&boot);
    std::thread::Builder::new()
        .name("fathom-render".into())
        .spawn(move || render::run::<A>(surface, receiver, thread_stats, thread_boot))?;

    app.manage(FathomState { sender, stats, boot });
    Ok(())
}

#[cfg(target_os = "windows")]
fn window_handle(window: &tauri::WebviewWindow) -> Result<isize, Box<dyn std::error::Error>> {
    Ok(window.hwnd()?.0 as isize)
}

#[cfg(not(target_os = "windows"))]
fn window_handle(_window: &tauri::WebviewWindow) -> Result<isize, Box<dyn std::error::Error>> {
    Err("fathom's native host is implemented on Windows only so far".into())
}

/// The commands the interface invokes.
///
/// They live in a module rather than at the crate root because `#[tauri::command]`
/// re-exports a macro under each command's name, which collides with the function at
/// the root of a library crate.
pub mod commands {
    use tauri::State;

    use fathom_core::{FrameStats, ViewportRect};

    use super::{FathomState, InitReply, Message};

    #[tauri::command]
    pub fn fathom_init(state: State<'_, FathomState>) -> Result<InitReply, String> {
        let boot = state.await_boot()?;
        Ok(InitReply {
            descriptor: boot.descriptor,
            param_byte_length: boot.param_byte_length,
            adapter_info: boot.adapter_info,
        })
    }

    #[tauri::command]
    pub fn fathom_set_viewport(rect: ViewportRect, state: State<'_, FathomState>) {
        state.send(Message::Viewport(rect));
    }

    #[tauri::command]
    pub fn fathom_write_params(bytes: Vec<u8>, state: State<'_, FathomState>) {
        state.send(Message::Params(bytes));
    }

    #[tauri::command]
    pub fn fathom_input(json: String, state: State<'_, FathomState>) {
        state.send(Message::Input(json));
    }

    #[tauri::command]
    pub fn fathom_command(name: String, args: String, state: State<'_, FathomState>) {
        state.send(Message::Command { name, args });
    }

    #[tauri::command]
    pub fn fathom_stats(state: State<'_, FathomState>) -> FrameStats {
        *state.stats.lock().unwrap()
    }

    #[tauri::command]
    pub fn fathom_destroy(state: State<'_, FathomState>) {
        state.send(Message::Shutdown);
    }
}

/// Every command the interface calls, ready for `Builder::invoke_handler`.
#[macro_export]
macro_rules! handlers {
    () => {
        ::tauri::generate_handler![
            $crate::commands::fathom_init,
            $crate::commands::fathom_set_viewport,
            $crate::commands::fathom_write_params,
            $crate::commands::fathom_input,
            $crate::commands::fathom_command,
            $crate::commands::fathom_stats,
            $crate::commands::fathom_destroy,
        ]
    };
}
