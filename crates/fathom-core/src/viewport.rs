//! Where the simulation is allowed to draw.
//!
//! The interface owns layout; it tells the host the rect the sim occupies, in device
//! pixels. On web that positions a canvas. On native it positions the child window the
//! wgpu surface lives in. Apps only ever see the resulting size.

use serde::{Deserialize, Serialize};

/// The size of the render target, in device pixels, plus the display's pixel ratio.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
    pub dpr: f32,
}

impl Default for Viewport {
    fn default() -> Self {
        Self { width: 1, height: 1, dpr: 1.0 }
    }
}

impl Viewport {
    pub fn new(width: u32, height: u32, dpr: f32) -> Self {
        Self { width: width.max(1), height: height.max(1), dpr }
    }

    pub fn aspect(&self) -> f32 {
        self.width.max(1) as f32 / self.height.max(1) as f32
    }
}

/// A rect in the host window's coordinate space, in device pixels. Native uses this to
/// place the child window; web only needs the size.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ViewportRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub dpr: f32,
}

impl ViewportRect {
    pub fn size(&self) -> Viewport {
        Viewport::new(self.width, self.height, self.dpr)
    }
}
