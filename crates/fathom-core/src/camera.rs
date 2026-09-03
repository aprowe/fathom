//! A 2D pan/zoom camera.
//!
//! The camera lives in a uniform, so panning and zooming never touch simulation
//! state — they cost one 16-byte upload per frame regardless of how many bodies
//! are on screen.

use crate::viewport::Viewport;

/// `zoom` is clip units per world unit along Y: the visible world height is `2 / zoom`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub center: [f32; 2],
    pub zoom: f32,
}

/// The camera as the shader sees it: `clip = (world - center) * scale`.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CameraUniform {
    pub center: [f32; 2],
    pub scale: [f32; 2],
}

impl Default for Camera {
    fn default() -> Self {
        Self { center: [0.0, 0.0], zoom: 1.0 }
    }
}

impl Camera {
    pub fn uniform(&self, viewport: Viewport) -> CameraUniform {
        let aspect = viewport.aspect();
        CameraUniform {
            center: self.center,
            scale: [self.zoom / aspect, self.zoom],
        }
    }

    /// Map a point in viewport-local device pixels to world space.
    pub fn screen_to_world(&self, x: f32, y: f32, viewport: Viewport) -> [f32; 2] {
        let (w, h) = (viewport.width.max(1) as f32, viewport.height.max(1) as f32);
        let ndc_x = (x / w) * 2.0 - 1.0;
        let ndc_y = 1.0 - (y / h) * 2.0;
        let aspect = viewport.aspect();
        [
            self.center[0] + ndc_x * aspect / self.zoom,
            self.center[1] + ndc_y / self.zoom,
        ]
    }

    /// Map a world point back to viewport-local device pixels.
    pub fn world_to_screen(&self, p: [f32; 2], viewport: Viewport) -> [f32; 2] {
        let (w, h) = (viewport.width.max(1) as f32, viewport.height.max(1) as f32);
        let aspect = viewport.aspect();
        let ndc_x = (p[0] - self.center[0]) * self.zoom / aspect;
        let ndc_y = (p[1] - self.center[1]) * self.zoom;
        [(ndc_x + 1.0) * 0.5 * w, (1.0 - ndc_y) * 0.5 * h]
    }

    /// Zoom by `factor`, keeping the world point under the cursor pinned to it.
    pub fn zoom_at(&mut self, x: f32, y: f32, factor: f32, viewport: Viewport) {
        let before = self.screen_to_world(x, y, viewport);
        self.zoom = (self.zoom * factor).clamp(1e-4, 1e5);
        let after = self.screen_to_world(x, y, viewport);
        self.center[0] += before[0] - after[0];
        self.center[1] += before[1] - after[1];
    }

    /// Pan by a delta measured in device pixels.
    pub fn pan_pixels(&mut self, dx: f32, dy: f32, viewport: Viewport) {
        let h = viewport.height.max(1) as f32;
        let world_per_pixel = 2.0 / (self.zoom * h);
        self.center[0] -= dx * world_per_pixel;
        self.center[1] += dy * world_per_pixel;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vp() -> Viewport {
        Viewport { width: 800, height: 400, dpr: 1.0 }
    }

    fn assert_close(a: [f32; 2], b: [f32; 2]) {
        assert!(
            (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3,
            "{a:?} != {b:?}"
        );
    }

    #[test]
    fn screen_and_world_round_trip() {
        let cam = Camera { center: [3.0, -1.0], zoom: 0.4 };
        for p in [[0.0, 0.0], [400.0, 200.0], [799.0, 399.0]] {
            let world = cam.screen_to_world(p[0], p[1], vp());
            assert_close(cam.world_to_screen(world, vp()), p);
        }
    }

    #[test]
    fn the_viewport_centre_maps_to_the_camera_centre() {
        let cam = Camera { center: [2.0, 5.0], zoom: 3.0 };
        assert_close(cam.screen_to_world(400.0, 200.0, vp()), [2.0, 5.0]);
    }

    #[test]
    fn aspect_is_corrected_so_pixels_stay_square() {
        let cam = Camera::default();
        // The viewport is 2:1, so a pixel must still be square: 200px right and 100px
        // up have to cover the same world distance *per pixel*.
        let right = cam.screen_to_world(600.0, 200.0, vp());
        let up = cam.screen_to_world(400.0, 100.0, vp());
        let world_per_px_x = right[0] / 200.0;
        let world_per_px_y = up[1] / 100.0;
        assert!(
            (world_per_px_x - world_per_px_y).abs() < 1e-6,
            "{world_per_px_x} != {world_per_px_y}"
        );
    }

    #[test]
    fn zooming_pins_the_world_point_under_the_cursor() {
        let mut cam = Camera { center: [0.0, 0.0], zoom: 1.0 };
        let before = cam.screen_to_world(120.0, 300.0, vp());
        cam.zoom_at(120.0, 300.0, 1.7, vp());
        assert_close(cam.screen_to_world(120.0, 300.0, vp()), before);
        assert!((cam.zoom - 1.7).abs() < 1e-5);
    }

    #[test]
    fn panning_moves_the_world_opposite_the_drag() {
        let mut cam = Camera::default();
        let anchor = cam.screen_to_world(400.0, 200.0, vp());
        cam.pan_pixels(100.0, 0.0, vp());
        let after = cam.screen_to_world(500.0, 200.0, vp());
        assert_close(after, anchor);
    }
}
