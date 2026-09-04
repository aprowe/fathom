# fathom

A framework for **GPU simulation apps with an interface around them**, written once and
run on two targets:

- **Web** — the app compiled to wasm, drawing into a `<canvas>` through WebGPU.
- **Native** — a Tauri window whose transparent webview *is* the interface, floating over
  a wgpu child window that the interface tells where to draw.

The panel is the same React code on both. That is enforced by a type, not by discipline:
everything the interface can do to a simulation goes through one `FathomHost` interface
with two implementations.

```bash
# web
cd apps/gravity && wasm-pack build --target web --out-dir pkg --release
cd ui && npm run dev

# native (Windows)
cd apps/gravity/src-tauri && npx tauri dev

# one self-contained HTML file, wasm and all
python apps/gravity/build-artifact.py
```

## Writing an app

An app is one Rust crate. The lifecycle is openFrameworks-shaped — `setup`, `update`,
`draw`, plus input callbacks that default to doing nothing, so you only write the ones
you care about.

```rust
fathom_core::params! {
    G      => ParamDef::float("g", "Gravity", 1.0, 0.0, 4.0).group("Physics"),
    TRAILS => ParamDef::toggle("trails", "Trails", true).group("Render"),
}

impl App for Gravity {
    fn describe() -> AppDescriptor { /* name, SCHEMA, COMMANDS */ }

    fn setup(ctx: &mut SetupCtx<'_>) -> Self { /* buffers and pipelines */ }
    fn update(&mut self, ctx: &mut UpdateCtx<'_>) { /* ctx.dt, ctx.params.float(G) */ }
    fn draw(&mut self, ctx: &mut DrawCtx<'_>) { /* record into ctx.encoder */ }

    fn mouse_dragged(&mut self, e: &MouseEvent, ctx: &mut EventCtx<'_>) { /* optional */ }
}
```

`params!` produces both the schema the interface reads and the index constants the app
reads, from one declaration, so the two cannot drift apart. Declaring a parameter is
most of the work of getting a slider: `<AutoControls/>` builds the whole panel from the
schema, grouped as declared.

Pan, zoom and the transport controls are framework behaviour — every app gets them
without writing any code.

## How the two targets fit together

```
            React panel + @fathom/ui          ← identical on both targets
                     │
                FathomHost                    ← one interface, two implementations
          ┌──────────┴───────────┐
      WebHost                NativeHost
     (wasm calls)           (Tauri commands)
          │                       │
     fathom-web              fathom-native
          └──────────┬────────────┘
                fathom-core                   ← Runner: the loop, shared
                     │
                 your App
```

`fathom-core` owns the frame loop, the camera, the clock and the parameter block. The
two hosts only manage a surface and marshal messages, which is what keeps the targets
from drifting apart.

**The viewport.** `<SimViewport/>` renders a transparent div, measures itself, and reports
its device-pixel rect. On web that sizes a canvas. On native it moves a wgpu **child
window** owned by the Tauri window — so the surface moves, resizes and clips with the
layout for free, with no always-on-top tracking and no z-order fights.

**Input** is captured in the DOM on both targets. Natively the transparent webview is the
topmost layer, so pointer and key events land in React exactly as they do on web. One
input path, no platform branching.

**Parameters** live in one flat block. Controls write into a `Float32Array` mirror — no
React re-render per slider frame — and the host flushes the whole block once per frame:
a zero-copy write into wasm memory on web, one small IPC message on native.

## The example: 2D gravity

`apps/gravity` is an exact N-body simulation. Every body pulls on every other body, with
no approximation: the force pass is O(n²) but tiled through workgroup shared memory, which
is what lets tens of thousands of bodies run at frame rate. Force and integration are
separate passes, so a Barnes-Hut or grid accelerator can replace the first without
reshaping the app.

Drag inside the view to pull bodies toward the cursor. Shift-drag or middle-drag to pan,
scroll to zoom, press <kbd>R</kbd> to reseed.

## Layout

```
crates/fathom-core     the App trait, params, camera, clock, Runner
crates/fathom-web      wasm host: a WebGPU canvas driven from JavaScript
crates/fathom-native   Tauri host: a wgpu child window and a render thread
packages/fathom-ui     @fathom/ui — SimViewport, Panel, Toolbar, controls
apps/gravity           the example: sim crate, React panel, Tauri shell
```

## Tests

```bash
cargo test --workspace   # params, camera, clock, scenes, and a real GPU check
npm test                 # parameter mirror, viewport rects, host selection
```

The GPU test dispatches the tiled force kernel at N=64 and compares it against a plain
CPU sum. A tiling bug drops or double-counts bodies while still *looking* plausible,
which is exactly the kind of thing an eye test misses. It skips itself when no adapter is
available.

## Known issue

On native, the simulation diverges during its first second and the view comes up empty;
pressing **Reset** restores it, and it then runs stably. The initial conditions and the
parameter defaults are identical either side of that Reset, so the cause is in the first
few steps rather than in the setup — the next thing to check is the `dt` the render
thread hands the clock across the gap between the surface being configured and the first
presented frame. The web target is unaffected.

## Status

The native render surface is implemented for Windows. macOS (an `NSView` subview) and
Linux sit behind the same `ChildSurface` interface and are not filled in yet; the web
target works everywhere WebGPU does.
