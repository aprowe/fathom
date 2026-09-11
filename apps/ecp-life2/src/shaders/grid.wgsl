// The uniform grid that turns the cutoff into a speedup instead of a wasted test.
//
// Beyond R a pair contributes exactly nothing, so an all-pairs sweep spends essentially
// all of its time proving that particles are far apart. Binning by cell and scanning the
// 3×3 neighbourhood turns that into a constant amount of work per particle, which is
// what puts a hundred thousand of them on screen at frame rate.
//
// One buffer holds four arrays back to back, because a WebGPU device is only guaranteed
// eight storage bindings per stage and the force pass needs most of them for particles.

@group(0) @binding(5) var<storage, read_write> grid: array<atomic<u32>>;
@group(0) @binding(6) var<storage, read_write> sorted: array<u32>;

// The grid is sized for the finest subdivision the cutoff slider allows and reused at
// every coarser one, so changing R costs nothing and reallocates nothing.
const MAX_CELLS_1D: u32 = 256u;
const MAX_CELLS: u32 = 65536u;
const SCAN_BLOCK: u32 = 256u;

// Regions of the shared buffer.
fn counts_at(c: u32) -> u32 { return c; }
fn offsets_at(c: u32) -> u32 { return MAX_CELLS + c; }
fn cursor_at(c: u32) -> u32 { return 2u * MAX_CELLS + c; }
fn block_sums_at(b: u32) -> u32 { return 3u * MAX_CELLS + b; }

fn cell_of(p: vec2<f32>) -> u32 {
    let cells = max(u.cells, 1u);
    let f = (p + vec2<f32>(0.5, 0.5)) * f32(cells);
    // A particle exactly on the far edge would index one past the end; wrapping rather
    // than clamping is also what the torus says should happen to it.
    let cx = u32(clamp(f.x, 0.0, f32(cells) - 0.001));
    let cy = u32(clamp(f.y, 0.0, f32(cells) - 0.001));
    return cy * cells + cx;
}
