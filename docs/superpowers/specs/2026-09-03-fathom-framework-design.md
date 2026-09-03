# fathom — design

A framework for building **GPU simulation apps with an interface around them**, from
one source, to two targets:

- **Web** — a wasm sim rendering into a `<canvas>` through WebGPU.
- **Native** — a Tauri window whose transparent webview is the interface, floating
  over a wgpu-rendered **child window** that the interface tells where to draw.

The lineage is `incompute` (a Vulkan compute sketch with an egui panel), generalized:
the sim is the app, the panel is framework-provided, and the two render paths are an
implementation detail app authors never see.

## Goals

1. An app author writes one Rust crate (+ WGSL) and one React panel, and gets both targets.
2. The panel code is *identical* across targets — enforced by a type, not by discipline.
3. Controls are cheap: declaring a parameter should be most of the work of getting a slider.
4. Prove it end to end with a real example, a 2D N-body gravity sim.

## Non-goals (this milestone)

- Linux/Wayland support (behind the same trait; untested).
- Spatial acceleration structures (Barnes-Hut, grids) — the example is O(n²).
- Hot-reloading WGSL.
- A `fathom` CLI. Apps are launched with plain npm and cargo commands.
- Multi-window, or more than one sim per page.

## Architecture

### Repository layout

```
fathom/
  Cargo.toml                     cargo workspace
  package.json                   npm workspace
  crates/
    fathom-core/                 Simulation trait, Gpu, Viewport/camera, ParamBlock, clock
    fathom-web/                  wasm-bindgen host: canvas surface + JS class
    fathom-native/               Tauri plugin: child-window surface, render thread, commands
  packages/
    fathom-ui/                   @fathom/ui — React runtime: SimViewport, Panel, controls, host shim
                                 (consumed as source through a Vite alias, so editing a
                                 control needs no build step)
  apps/
    gravity/
      Cargo.toml                 the sim crate (cdylib for wasm + rlib for native)
      src/lib.rs
      src/force.wgsl  src/integrate.wgsl  src/draw.wgsl
      ui/                        React app (Vite)
      src-tauri/                 Tauri shell binary
  docs/superpowers/specs/
```

### The app contract

```rust
pub trait App: Sized + 'static {
    fn describe() -> AppDescriptor;            // name + param schema + command list
    fn setup(ctx: &mut SetupCtx<'_>) -> Self;
    fn update(&mut self, ctx: &mut UpdateCtx<'_>);  // dt, time, params, camera
    fn draw(&mut self, ctx: &mut DrawCtx<'_>);      // encoder, target, viewport, camera

    // openFrameworks-shaped input callbacks; all default to doing nothing.
    fn mouse_pressed(&mut self, e: &MouseEvent, ctx: &mut EventCtx<'_>) {}
    fn mouse_dragged(&mut self, e: &MouseEvent, ctx: &mut EventCtx<'_>) {}
    fn mouse_scrolled(&mut self, e: &ScrollEvent, ctx: &mut EventCtx<'_>) {}
    fn key_pressed(&mut self, e: &KeyEvent, ctx: &mut EventCtx<'_>) {}
    fn resized(&mut self, ctx: &mut EventCtx<'_>) {}
    fn command(&mut self, ctx: &mut CommandCtx<'_>) {}
}
```

The lifecycle is `setup` / `update` / `draw` after openFrameworks: an app writes only the
callbacks it cares about, and the framework owns everything else. Pan, zoom and the
transport controls are handled by the runner before the app sees an event, so every app
gets them for free.

`Gpu` is a thin wrapper over `wgpu::Device`/`Queue` plus adapter limits. It is identical
on both targets: wgpu's WebGPU backend *is* the browser path, so an app crate compiles
unchanged to `wasm32-unknown-unknown` and to a native rlib.

`StepCtx` carries `dt`, a read-only `&Params` view over the packed block, and the
frame's drained `SimEvent` queue. `RenderCtx` carries the target texture view, the
viewport rect in device pixels, and the camera uniform.

### Host abstraction

One TypeScript interface, two implementations, selected by feature-detecting
`window.__TAURI__`:

```ts
interface FathomHost {
  init(el: HTMLElement, appId: string): Promise<AppDescriptor>
  setViewport(rect: Rect, dpr: number): void
  writeParams(buf: Float32Array): void
  sendEvent(e: SimEvent): void
  command(c: { name: string; args?: unknown }): void
  stats(): FrameStats
  destroy(): void
}
```

- **`WebHost`** wraps the `fathom-web` wasm class. `init` attaches a canvas to the
  element, requests an adapter, and starts a `requestAnimationFrame` loop.
- **`NativeHost`** calls Tauri commands (`fathom_init`, `fathom_set_viewport`,
  `fathom_write_params`, `fathom_input`, `fathom_command`, `fathom_stats`,
  `fathom_destroy`) exported by `fathom-native`, which owns a render thread driving the
  child window. These are registered as ordinary app commands rather than as a Tauri
  plugin: a plugin would add a permissions manifest for no benefit, since the only caller
  is the app's own interface. An app wires them up with
  `.setup(fathom_native::setup::<MyApp>)` and `.invoke_handler(fathom_native::handlers!())`.

Rejected alternatives: running the sim in a Worker on web too (uniform, but adds
latency and OffscreenCanvas complexity to the target that did not need it); and no
shared interface at all (less code now, no reuse later). A Worker-backed host can be
added as a third `FathomHost` implementation without changing app code.

### Parameters

`describe()` returns a `ParamSchema`: an ordered list of `{ name, kind, range, default,
group }`, where `kind` is `f32 | u32 | bool | enum`. The framework packs these into a
single flat block (16-byte aligned, uploaded to one uniform buffer).

TypeScript keeps a `Float32Array` mirror of that block. A control writes into the array
directly — no React state, no re-render on a slider drag — and the host flushes the
whole block exactly once per animation frame:

- **Web:** a zero-copy write into wasm linear memory.
- **Native:** one Tauri IPC message carrying ~100 bytes of raw block.

The call site is the same on both: `params.g = 0.5`.

Parameters that cannot change without reallocating buffers (particle count, for one)
are **commands**, not params.

### Viewport — "events tell it where to render"

`<SimViewport/>` renders a transparent div. A `ResizeObserver` plus scroll and window
listeners compute its device-pixel rect and call `host.setViewport(rect, dpr)`,
coalesced to one call per frame.

- **Web:** the canvas is sized and positioned to that rect.
- **Native:** the rect drives the wgpu **child window** — a child HWND on Windows, an
  NSView subview on macOS — owned by the Tauri window. It therefore moves, resizes and
  clips with the layout automatically: no always-on-top tracking, no z-order fights,
  no manual click-through. The div is a transparent hole in the webview above it
  (`transparent: true` on the Tauri window, transparent background on the div).

### Input

Pointer, wheel and keyboard events are captured on the `SimViewport` div **in the DOM
on both targets**. Natively the transparent webview is the topmost layer, so events
land in React exactly as they do on web. They are normalized to a `SimEvent`
(`{ kind, x, y, buttons, modifiers, delta }`, coordinates in viewport-local device
pixels) and forwarded through the host. One input path, no platform branching.

### Control kit (`@fathom/ui`)

Framework-provided so every app reads as one product:

- `<Panel>` — dockable, collapsible, grouped sections.
- `<Toolbar>` — play/pause, step, reset, fps and ms/frame readout.
- `<Slider param>`, `<Toggle param>`, `<Select param>`, `<Button command>`, `<Stat>`.

Controls bind by parameter name against the schema returned by `init`. An unknown name
throws at startup rather than silently doing nothing.

## The gravity example

A 2D N-body simulation, brute force, structured so the force pass can be replaced.

**Passes**

1. `force.wgsl` — O(n²) accumulation, tiled through workgroup shared memory (256 bodies
   per tile), with Plummer softening `ε`.
2. `integrate.wgsl` — leapfrog integration with a clamped `dt`.
3. `draw.wgsl` — instanced points, additive blending, optional trails via a feedback
   texture faded each frame.

Splitting force from integrate is the upgrade seam: a Barnes-Hut or uniform-grid pass
can replace step 1 without reshaping the app.

**State** — `pos` (`vec2<f32>` + mass), `vel` (`vec2<f32>`), `accel`, double-buffered.

**Parameters** — `G`, softening `ε`, timescale, point size, trail fade, color-by-speed,
paused.

**Commands** — `reset`, `randomize`, `set_count(n)`, `set_initial_condition(kind)` where
kind is one of disc, two-galaxy collision, ring, uniform.

**Interaction** — drag inside the viewport to place a gravity well, scroll to zoom,
middle-drag to pan (camera lives in a uniform, so pan and zoom cost nothing).

Target: 20,000–30,000 bodies at 60fps on a mid-range discrete GPU.

## Error handling

| Failure | Behavior |
|---|---|
| No adapter / device request fails | Framework renders a fallback card naming the actual reason (e.g. "WebGPU unavailable — needs Chrome 113+"). |
| Device lost | Host tears down and re-initializes the sim, preserving the current parameter block. |
| WGSL compile error | Red banner in the toolbar carrying the compiler's own message. |
| Child window creation fails (native) | Loud diagnostic and exit. No silent fallback to a second top-level window. |
| Control names a missing param | Throws during `init`, before the first frame. |

## Testing

**Rust**

- Param block packing/unpacking round-trips for every `kind`, including alignment.
- Viewport and camera transform math: screen point to world point and back.
- Seeded initial-condition generators are deterministic and produce expected invariants
  (particle count, bounded radius, zero net momentum for the symmetric cases).
- Headless wgpu: one `force` dispatch at N=64 compared against a CPU reference
  implementation within tolerance. This is the "it computes" test.

**TypeScript (vitest)**

- Parameter mirror: writes land at the right offsets; one flush per frame, not per write.
- Host selection picks `NativeHost` iff `window.__TAURI__` is present.
- Viewport rect computation under scroll, resize and non-1 device pixel ratio, against
  a mock host.

No end-to-end tests this milestone.

## Risks

- **Child-window compositing** is the load-bearing native trick. Solid on Windows (the
  development platform) and expected to be fine on macOS. Linux/Wayland is the weak
  spot and is explicitly out of scope.
- **wgpu WebGPU backend parity** — a shader valid natively can be rejected by the
  browser's stricter validation. Mitigated by developing the example on web first and
  keeping WGSL to the common subset.
- **IPC parameter cost on native** — one message per frame is expected to be
  negligible; if it is not, the fallback is a shared memory-mapped param block.

## Milestones

1. **Core** — `fathom-core`: trait, `Gpu`, param schema and packing, camera, clock. Tests.
2. **Web path** — `fathom-web` wasm host, `@fathom/ui` with `SimViewport` and the control
   kit, `WebHost`. A trivial built-in sim (a clear-color that reacts to one slider)
   proves the loop before gravity exists.
3. **Gravity** — the three passes, initial conditions, panel, interaction. Web only.
4. **Native path** — `fathom-native` child window and Tauri commands, `NativeHost`, and
   the gravity app's `src-tauri` shell. The same panel code, unchanged.
5. **Polish** — error surfaces, fps/ms readout, README with both run commands.

## Running it

```bash
# web
cd apps/gravity/ui && npm run dev

# native
cd apps/gravity && cargo tauri dev
```
