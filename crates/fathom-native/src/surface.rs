//! The window the simulation renders into, beneath the transparent interface.
//!
//! ## Why this is a sibling window and not a child window
//!
//! The obvious shape is a child window inside the Tauri window, below the webview: the
//! OS would then move, resize and clip it with its parent for free. That does not work
//! on Windows. A transparent WRY window is created with `WS_EX_NOREDIRECTIONBITMAP`, so
//! it has no redirection surface, and a legacy child HWND holding a DXGI swapchain is
//! never composited into it. The child window sits there correctly positioned, visible
//! by every API measure, and renders nothing anyone can see.
//!
//! So the surface is a borderless top-level window that the Tauri window *owns*. An
//! owned window is always ordered directly beneath its owner, follows it when it is
//! minimised and restored, and stays out of the taskbar — which gives back most of what
//! the child-window arrangement would have provided. What it does not give back is
//! automatic geometry, so this module tracks the parent's client origin itself and the
//! host re-places the surface whenever the window moves.
//!
//! `WS_EX_NOACTIVATE` keeps the surface from ever taking focus, so the interface above
//! it stays the layer that receives input, which is what makes one input path possible.

/// The render surface's window, addressed by raw handles so it can be sent to the
/// render thread.
#[derive(Clone, Copy, Debug)]
pub struct OverlaySurface {
    #[allow(dead_code)]
    window: isize,
    #[allow(dead_code)]
    parent: isize,
    #[allow(dead_code)]
    instance: isize,
}

// The handles are inert integers. Messages for this window are pumped by the thread
// that created it; this type only moves it and draws into it.
unsafe impl Send for OverlaySurface {}
unsafe impl Sync for OverlaySurface {}

#[cfg(target_os = "windows")]
mod platform {
    use std::sync::Once;

    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows::Win32::Graphics::Gdi::ClientToScreen;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, GWLP_HWNDPARENT, RegisterClassW, SWP_NOACTIVATE,
        SWP_NOMOVE, SWP_NOSIZE, SetWindowLongPtrW, SetWindowPos, WNDCLASSW, WS_EX_NOACTIVATE,
        WS_EX_TOOLWINDOW, WS_POPUP, WS_VISIBLE,
    };
    use windows::core::{PCWSTR, w};

    use super::OverlaySurface;

    const CLASS_NAME: PCWSTR = w!("fathom_surface");
    static REGISTER: Once = Once::new();

    unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
    }

    pub fn create(parent: isize) -> Result<OverlaySurface, String> {
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
                // NOACTIVATE so it never steals focus from the interface; TOOLWINDOW so
                // it never appears as a second entry in the taskbar.
                WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                CLASS_NAME,
                PCWSTR::null(),
                WS_POPUP | WS_VISIBLE,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(instance.into()),
                None,
            )
            .map_err(|e| format!("could not create the render surface window: {e}"))?;

            // Make the Tauri window the *owner* of the surface. Windows then keeps the
            // owner above it, minimises and restores the pair together, and closes the
            // surface with the app.
            SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, parent);

            Ok(OverlaySurface {
                window: hwnd.0 as isize,
                parent,
                instance: instance.0 as isize,
            })
        }
    }

    /// Place the surface, given a rect relative to the parent's client area.
    pub fn set_rect(surface: &OverlaySurface, x: i32, y: i32, width: u32, height: u32) {
        unsafe {
            // The interface measures in webview coordinates; the webview fills the
            // parent's client area, so its origin is the client origin.
            let mut origin = POINT { x: 0, y: 0 };
            let _ = ClientToScreen(HWND(surface.parent as *mut _), &mut origin);

            let _ = SetWindowPos(
                HWND(surface.window as *mut _),
                None,
                origin.x + x,
                origin.y + y,
                width as i32,
                height as i32,
                SWP_NOACTIVATE,
            );
        }
    }

    /// Re-assert that the surface sits directly beneath the interface.
    ///
    /// Ownership handles this in the normal case, but activating the app from the
    /// taskbar or from another window can leave the pair interleaved with whatever was
    /// on top before.
    pub fn restack(surface: &OverlaySurface) {
        unsafe {
            let _ = SetWindowPos(
                HWND(surface.window as *mut _),
                Some(HWND(surface.parent as *mut _)),
                0,
                0,
                0,
                0,
                SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
            );
        }
    }

    pub fn surface_target(surface: &OverlaySurface) -> Result<wgpu::SurfaceTargetUnsafe, String> {
        use std::num::NonZeroIsize;

        use raw_window_handle::{
            RawDisplayHandle, RawWindowHandle, Win32WindowHandle, WindowsDisplayHandle,
        };

        let window = NonZeroIsize::new(surface.window).ok_or("the surface window handle is null")?;
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
    use super::OverlaySurface;

    const UNSUPPORTED: &str = "fathom's native target implements its render surface on \
        Windows only so far. The web target works everywhere WebGPU does; macOS (an \
        NSView subview, which does not have the composition problem Windows has) and \
        Linux are the next platforms to fill in here.";

    pub fn create(_parent: isize) -> Result<OverlaySurface, String> {
        Err(UNSUPPORTED.to_string())
    }

    pub fn set_rect(_surface: &OverlaySurface, _x: i32, _y: i32, _width: u32, _height: u32) {}

    pub fn restack(_surface: &OverlaySurface) {}

    pub fn surface_target(_surface: &OverlaySurface) -> Result<wgpu::SurfaceTargetUnsafe, String> {
        Err(UNSUPPORTED.to_string())
    }
}

impl OverlaySurface {
    /// Create the surface window, owned by `parent`. Must run on the thread that owns
    /// `parent`.
    pub fn create(parent: isize) -> Result<Self, String> {
        platform::create(parent)
    }

    /// Move and resize, given a rect relative to the parent's client area in physical
    /// pixels — exactly what the interface reports.
    pub fn set_rect(&self, x: i32, y: i32, width: u32, height: u32) {
        platform::set_rect(self, x, y, width, height)
    }

    /// Put the surface back directly beneath the interface.
    pub fn restack(&self) {
        platform::restack(self)
    }

    pub fn surface_target(&self) -> Result<wgpu::SurfaceTargetUnsafe, String> {
        platform::surface_target(self)
    }
}
