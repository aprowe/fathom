//! The native host: a fathom app behind a transparent Tauri webview.
//!
//! The interface is the same React panel the web target runs. The difference is only
//! where the pixels come from: a borderless wgpu window that the Tauri window owns and
//! floats above, positioned by the rects the interface reports. See [`surface`] for why
//! it is a sibling window rather than a child of the Tauri window.
//!
//! Wiring an app up takes two lines in its Tauri binary:
//!
//! ```ignore
//! tauri::Builder::default()
//!     .setup(fathom_native::setup::<Gravity>)
//!     .invoke_handler(fathom_native::handlers!())
//! ```

mod render;
mod surface;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Sender};
use std::time::{Duration, Instant};

use fathom_core::App;
use serde::Serialize;
use tauri::Manager;

use render::{Boot, Message, Shared};

/// How long the interface waits for the GPU to come up before giving up on it.
const BOOT_TIMEOUT: Duration = Duration::from_secs(10);

/// The handle the commands talk to. Deliberately not generic: Tauri commands cannot be,
/// so the app type is erased here and lives only inside the render thread.
pub struct FathomState {
    sender: Sender<Message>,
    shared: Arc<Shared>,
    /// Moving the window is a layout concern, so it happens on the thread that owns it
    /// rather than on the render thread. That also means a stalled frame can never
    /// leave the window in the wrong place.
    surface: surface::OverlaySurface,
}

impl FathomState {
    fn send(&self, message: Message) {
        // A closed channel means the render thread is gone; there is nothing useful a
        // command could do about that, and panicking would take the window down.
        let _ = self.sender.send(message);
    }

    /// Put the render window back where the interface last asked for it, and directly
    /// beneath the interface in the z-order.
    fn reposition(&self) {
        if let Some(rect) = *self.shared.placement.lock().unwrap() {
            self.surface.set_rect(rect.x, rect.y, rect.width, rect.height);
        }
        self.surface.restack();
    }

    /// Block until the render thread reports success or failure.
    fn await_boot(&self) -> Result<Boot, String> {
        let deadline = Instant::now() + BOOT_TIMEOUT;
        loop {
            if let Some(result) = self.shared.boot.lock().unwrap().clone() {
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

/// Start the render thread and create the render window. Pass to `Builder::setup`.
pub fn setup<A: App>(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let window = app
        .get_webview_window("main")
        .ok_or("fathom needs a window labelled \"main\"")?;

    // Must happen here: the render window belongs to the thread that creates it, and
    // setup runs on the thread that pumps the Tauri window's messages.
    let parent = window_handle(&window)?;
    let surface = surface::OverlaySurface::create(parent)?;

    let (sender, receiver) = mpsc::channel();
    let shared = Arc::new(Shared::default());

    let thread_shared = Arc::clone(&shared);
    std::thread::Builder::new()
        .name("fathom-render".into())
        .spawn(move || render::run::<A>(surface, receiver, thread_shared))?;

    app.manage(FathomState { sender, shared, surface });

    // The interface reports rects relative to the webview, which do not change when the
    // window itself is dragged across the screen. Ownership keeps the two windows
    // stacked; their positions have to be kept in step here.
    let tracked = window.clone();
    window.on_window_event(move |event| {
        let Some(state) = tracked.try_state::<FathomState>() else { return };
        match event {
            tauri::WindowEvent::Resized(_) => {
                // Ask the window whether it is minimised rather than reading it out of
                // the event: a zero-sized resize is also reported during startup, and
                // latching "hidden" there would leave the app permanently blank.
                let visible = !tracked.is_minimized().unwrap_or(false);
                state.shared.visible.store(visible, Ordering::Relaxed);
                state.reposition();
            }
            tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Focused(true) => state.reposition(),
            _ => {}
        }
    });

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
        // The window moves here, on the thread that owns it. The render thread picks the
        // rect up on its next frame and resizes its surface to match.
        state.surface.set_rect(rect.x, rect.y, rect.width, rect.height);
        *state.shared.placement.lock().unwrap() = Some(rect);
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
        *state.shared.stats.lock().unwrap()
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
        ]
    };
}
