// Building the grid: clear, count, scan, scatter.
//
// All four entry points live in one module because they share the buffer layout and the
// cell arithmetic, and splitting them would mean four copies of the include header for
// four dispatches that always run together.

@group(0) @binding(1) var<storage, read_write> pos: array<vec4<f32>>;

var<workgroup> block: array<u32, 256>;

// Pass 1. The counts from last frame mean nothing this frame.
@compute @workgroup_size(256)
fn clear(@builtin(global_invocation_id) gid: vec3<u32>) {
    let c = gid.x;
    if (c < MAX_CELLS) {
        atomicStore(&grid[counts_at(c)], 0u);
    }
}

// Pass 2. How many particles land in each cell.
@compute @workgroup_size(256)
fn count(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= u.n) {
        return;
    }
    atomicAdd(&grid[counts_at(cell_of(pos[i].xy))], 1u);
}

// Pass 3a. Exclusive prefix sum within each block of 256 cells, plus the block's total.
//
// Two levels of scan cover the whole grid exactly: 256 blocks of 256 cells is 65,536,
// which is the finest subdivision the cutoff allows. A third level is never needed, so
// there is not one.
@compute @workgroup_size(256)
fn scan_blocks(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    let c = gid.x;
    let mine = select(0u, atomicLoad(&grid[counts_at(c)]), c < MAX_CELLS);
    block[lid.x] = mine;
    workgroupBarrier();

    // Hillis-Steele: inclusive after the loop, made exclusive by subtracting our own.
    for (var off: u32 = 1u; off < SCAN_BLOCK; off = off << 1u) {
        var add: u32 = 0u;
        if (lid.x >= off) {
            add = block[lid.x - off];
        }
        workgroupBarrier();
        block[lid.x] = block[lid.x] + add;
        workgroupBarrier();
    }

    let inclusive = block[lid.x];
    let total = block[SCAN_BLOCK - 1u];
    if (c < MAX_CELLS) {
        atomicStore(&grid[offsets_at(c)], inclusive - mine);
    }
    if (lid.x == 0u) {
        atomicStore(&grid[block_sums_at(wid.x)], total);
    }
}

// Pass 3b. Scan the 256 block totals, in a single workgroup.
@compute @workgroup_size(256)
fn scan_sums(@builtin(local_invocation_id) lid: vec3<u32>) {
    let mine = atomicLoad(&grid[block_sums_at(lid.x)]);
    block[lid.x] = mine;
    workgroupBarrier();

    for (var off: u32 = 1u; off < SCAN_BLOCK; off = off << 1u) {
        var add: u32 = 0u;
        if (lid.x >= off) {
            add = block[lid.x - off];
        }
        workgroupBarrier();
        block[lid.x] = block[lid.x] + add;
        workgroupBarrier();
    }

    atomicStore(&grid[block_sums_at(lid.x)], block[lid.x] - mine);
}

// Pass 3c. Lift each block's offsets by everything before it, and open a write cursor at
// each cell's start. The cursor is a separate array rather than the offsets themselves
// because the scatter destroys it and the force pass still needs where each cell begins.
@compute @workgroup_size(256)
fn scan_add(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    let c = gid.x;
    if (c >= MAX_CELLS) {
        return;
    }
    let base = atomicLoad(&grid[block_sums_at(wid.x)]);
    let start = atomicLoad(&grid[offsets_at(c)]) + base;
    atomicStore(&grid[offsets_at(c)], start);
    atomicStore(&grid[cursor_at(c)], start);
}

// Pass 4. Write each particle's index into its cell's run.
@compute @workgroup_size(256)
fn scatter(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= u.n) {
        return;
    }
    let slot = atomicAdd(&grid[cursor_at(cell_of(pos[i].xy))], 1u);
    sorted[slot] = i;
}
