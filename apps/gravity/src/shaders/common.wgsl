// Shared uniform layout. Every pipeline in the gravity app binds this at @group(0)
// @binding(0), so the Rust side only has to keep one struct in sync.
//
// This file is textually included by the other shaders at build time (see shaders.rs);
// WGSL has no include of its own.

struct Uniforms {
    // Camera: clip = (world - center) * scale
    center: vec2<f32>,
    scale: vec2<f32>,
    // Interactive gravity well: xy position, z strength, w on/off.
    well: vec4<f32>,
    viewport: vec2<f32>,
    dt: f32,
    g: f32,
    softening: f32,
    point_size: f32,
    brightness: f32,
    n: u32,
    color_mode: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}
