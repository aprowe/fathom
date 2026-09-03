// Trails: dim what is already in the accumulation buffer instead of clearing it.
//
// The pipeline this belongs to blends with src = Zero and dst = Constant, so drawing
// this triangle multiplies the existing image by the blend constant (the trail-fade
// parameter). Nothing about the fragment colour matters; the blend does the work.

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    // One oversized triangle covers the viewport with no vertex buffer and no seam
    // down the middle that a two-triangle quad would have.
    let x = f32(i32(vi) / 2) * 4.0 - 1.0;
    let y = f32(i32(vi) & 1) * 4.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

@fragment
fn fs_main() -> @location(0) vec4<f32> {
    return vec4<f32>(0.0, 0.0, 0.0, 0.0);
}
