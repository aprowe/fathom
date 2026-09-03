//! The child window the simulation renders into.
//!
//! This is the load-bearing trick of the native target. Rather than a second top-level
//! window chasing the app window around, the wgpu surface lives in a *child* window
//! owned by the Tauri window and sitting below the webview in the z-order. The operating
//! system then moves, resizes and clips it with its parent for free: no always-on-top
//! tracking, no z-order fights, and no click-through logic, because the transparent
//! webview above it keeps receiving all the input.

/// A child window, addressed by raw handles so it can be sent to the render thread.
///
/// Creating it must happen on the thread that pumps the parent's messages; moving it and
/// drawing into it may happen anywhere, which is exactly the split this type encodes.
#[derive(Clone, Copy, Debug)]
pub struct ChildSurface {
    #[allow(dead_code)]
    window: isize,
    #[allow(dead_code)]
    instance: isize,
}

// The handles are inert integers. The window's messages are pumped by the thread that
// created it; this type only moves and draws.
unsafe impl Send for ChildSurface {}
unsafe impl Sync for ChildSurface {}

#[cfg(target_os = "windows")]
mod platform {
    use std::sync::Once;

    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, HWND_BOTTOM, RegisterClassW, SWP_NOACTIVATE,
        SWP_NOZORDER, SetWindowPos, WINDOW_EX_STYLE, WNDCLASSW, WS_CHILD, WS_CLIPSIBLINGS,
        WS_VISIBLE,
    };
    use windows::core::{PCWSTR, w};

    use super::ChildSurface;

    const CLASS_NAME: PCWSTR = w!("fathom_surface");
    static REGISTER: Once = Once::new();

    unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
    }

    pub fn create(parent: isize) -> Result<ChildSurface, String> {
        unsafe {
            let instance = GetModuleHandleW(None).map_err(|e| format!("GetModuleHandleW: {e}"))?;

            REGISTER.call_once(|| {
                let class = WNDCLASSW {
                    lpfnWndProc: Some(wndproc),
                    hInstance: instance.into(),
                    lpszClassName: CLASS_NAME,
                    // No background brush: every pixel comes from wgpu, and letting the
                    // system paint one first produces a white flash on resize.
                    ..Default::default()
                };
                RegisterClassW(&class);
            });

            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                CLASS_NAME,
                PCWSTR::null(),
                // CLIPSIBLINGS keeps the webview above from being painted over.
                WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS,
                0,
                0,
                1,
                1,
                Some(HWND(parent as *mut _)),
                None,
                Some(instance.into()),
                None,
            )
            .map_err(|e| format!("could not create the render child window: {e}"))?;

            // Below the webview, so the interface is the layer that receives input.
            let _ = SetWindowPos(hwnd, Some(HWND_BOTTOM), 0, 0, 1, 1, SWP_NOACTIVATE);

            Ok(ChildSurface { window: hwnd.0 as isize, instance: instance.0 as isize })
        }
    }

    pub fn set_rect(surface: &ChildSurface, x: i32, y: i32, width: u32, height: u32) {
        unsafe {
            let _ = SetWindowPos(
                HWND(surface.window as *mut _),
                None,
                x,
                y,
                width as i32,
                height as i32,
                SWP_NOACTIVATE | SWP_NOZORDER,
            );
        }
    }

    pub fn surface_target(surface: &ChildSurface) -> Result<wgpu::SurfaceTargetUnsafe, String> {
        use std::num::NonZeroIsize;

        use raw_window_handle::{
            RawDisplayHandle, RawWindowHandle, Win32WindowHandle, WindowsDisplayHandle,
        };

        let window = NonZeroIsize::new(surface.window).ok_or("the child window handle is null")?;
        let mut handle = Win32WindowHandle::new(window);
        handle.hinstance = NonZeroIsize::new(surface.instance);

        Ok(wgpu::SurfaceTargetUnsafe::RawHandle {
            raw_display_handle: Some(RawDisplayHandle::Windows(WindowsDisplayHandle::new())),
            raw_window_handle: RawWindowHandle::Win32(handle),
        })
    }
}

#[cfg(not(target_os = "windows"))]
mod platform {
    use super::ChildSurface;

    const UNSUPPORTED: &str = "fathom's native target currently implements its child \
        surface on Windows only. The web target works everywhere; macOS (NSView subview) \
        and Linux are the next platforms to fill in here.";

    pub fn create(_parent: isize) -> Result<ChildSurface, String> {
        Err(UNSUPPORTED.to_string())
    }

    pub fn set_rect(_surface: &ChildSurface, _x: i32, _y: i32, _width: u32, _height: u32) {}

    pub fn surface_target(_surface: &ChildSurface) -> Result<wgpu::SurfaceTargetUnsafe, String> {
        Err(UNSUPPORTED.to_string())
    }
}

impl ChildSurface {
    /// Create the child window. Must run on the thread that owns `parent`.
    pub fn create(parent: isize) -> Result<Self, String> {
        platform::create(parent)
    }

    /// Move and resize, in physical pixels relative to the parent's client area.
    pub fn set_rect(&self, x: i32, y: i32, width: u32, height: u32) {
        platform::set_rect(self, x, y, width, height)
    }

    pub fn surface_target(&self) -> Result<wgpu::SurfaceTargetUnsafe, String> {
        platform::surface_target(self)
    }
}
