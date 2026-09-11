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
    // A body being aimed: mass, and whether one is.
    launch_mass: f32,
    launch_on: f32,
    _pad: u32,
    // Where it will start (xy) and where the drag has reached (zw), in world units.
    launch: vec4<f32>,
}

// Point radius as a multiple of the base size. A body heavier than the reference
// grows with the square root of its mass, so a star reads as a disc rather than a
// brighter dot; the swarm, far lighter than the reference, stays at the base size.
const MASS_REF: f32 = 0.002;

fn radius_for(mass: f32) -> f32 {
    return max(1.0, sqrt(mass / MASS_REF));
}
