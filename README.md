# fathom

A framework for **GPU simulation apps with an interface around them**, in Rust, written
once and run on two targets from the same crate:

- **Native** — one window. The panel is drawn by egui beside the simulation, on the same
  wgpu device, in the same process.
- **Web** — the same shell compiled to wasm. egui draws into a canvas and wgpu talks to
  WebGPU. The only HTML is a page whose whole job is to hand egui a canvas.

Every app is a small crate implementing one trait. The framework supplies the window,
the surface, the clock, the camera, pan and zoom, the transport, the panel and both
targets.

```bash
# desktop
cargo run --release -p gravity --bin gravity-egui
cargo run --release -p ecp-life2 --bin ecp-life2-egui

# browser (WebGPU: Chrome/Edge 113+, Safari 26, Firefox with dom.webgpu.enabled)
cd apps/gravity && wasm-pack build --target web --out-dir pkg --release
python -m http.server 8099        # then open http://localhost:8099/web/
```

## Writing an app

The lifecycle is openFrameworks-shaped — `setup`, `update`, `draw`, plus input callbacks
that default to doing nothing, so you only write the ones you care about.

```rust
fathom_core::params! {
    G      => ParamDef::float("g", "Gravity", 1.0, 0.0, 4.0).group("Physics"),
    TRAILS => ParamDef::toggle("trails", "Trails", true).group("Render"),
    FADE   => ParamDef::float("fade", "Trail length", 0.9, 0.5, 0.99).group("Render").advanced(),
}

impl App for Gravity {
    fn describe() -> AppDescriptor { /* name, SCHEMA, COMMANDS */ }

    fn setup(ctx: &mut SetupCtx<'_>) -> Self { /* buffers and pipelines */ }
    fn update(&mut self, ctx: &mut UpdateCtx<'_>) { /* ctx.dt, ctx.params.float(G) */ }
    fn draw(&mut self, ctx: &mut DrawCtx<'_>) { /* record into ctx.encoder */ }

    fn mouse_dragged(&mut self, e: &MouseEvent, ctx: &mut EventCtx<'_>) { /* optional */ }
    fn command(&mut self, ctx: &mut CommandCtx<'_>) { /* buttons and selects */ }
}
```

`params!` produces both the schema the panel reads and the index constants the app
reads, from one declaration, so the two cannot drift apart. Declaring a parameter is most
of the work of getting a control for it: the panel is generated from the schema, grouped
as declared, with `.advanced()` controls filed behind each group's disclosure.

Commands are the things an app cannot express as a number — a Reset button, a scene
select. `CommandCtx` is the one context that can *write* parameters, because applying a
preset means moving the sliders. Everywhere else the panel is the author of those values
and the app only reads them, so an app never fights the widget the user is holding.

## How it fits together

```
                 fathom-shell            ← egui panel + offscreen sim texture
              (eframe: winit/wgpu natively, a canvas on web)
                       │
                  fathom-core            ← Runner: the loop, camera, clock, params
                       │
                    your App
```

`fathom-core` owns the frame loop, the camera, the clock and the parameter block, and
knows nothing about windows. `fathom-shell` owns a `Runner` and draws around it.

**The viewport.** The simulation renders into an offscreen texture that the panel then
shows as an image. That is what keeps an app's `draw` unchanged between targets: it
receives a target view and renders into it, exactly as it would if the view were a
swapchain. The texture is re-registered with egui when the viewport resizes.

**Parameters** live in one flat, uniform-friendly block. Because the panel and the
simulation share a process, a slider writes straight into the live block — there is no
mirror, no IPC, and nothing serialised anywhere in the frame.

**Layout.** On a wide screen the panel docks beside the simulation and can be resized.
Below 760pt it becomes a drawer that slides in over the simulation, because a docked
column would take half a phone screen from the thing it controls. The transport is a
floating bar in both layouts.

**The skin.** egui gives itself away by its typeface, its widget shapes and a flat grey
palette; `skin.rs` replaces all three. IBM Plex is embedded (OFL), so neither build
fetches anything at runtime. Sliders are drawn rather than configured — a row that *is*
the reading, with a bright edge where the value falls — and drags are relative, so a
slider you touch does not lose its value before you have moved a pixel. Shift makes a
drag crawl; a click on the readout lets you type.

## The apps

**[`apps/gravity`](apps/gravity)** — an exact 2D N-body simulation. Every body pulls on
every other body, with no approximation: the force pass is O(n²) but tiled through
workgroup shared memory, which is what lets tens of thousands of bodies run at frame
rate. It opens on a binary: two heavy stars orbiting each other, each with a swarm bound
to it and a ring around the pair. Right-drag anywhere to launch a new body — the line
you draw is its velocity, the *Launch mass* slider is its weight, and it is drawn at the
size its mass earns. Left-drag pulls bodies toward the cursor (or launches too, if you
switch it in the panel); shift-drag or middle-drag to pan, scroll to zoom, <kbd>R</kbd>
to reseed.

**[`apps/ecp-life2`](apps/ecp-life2)** — particle life with relational colour energy.
Each particle carries a continuous colour on a circle, and how two particles interact is
a smooth surface over the *pair* of their colours. The part of that surface that does
net work is paid for, locally and in the same step, out of energy stored in the colour
mismatch between neighbours. Its README goes into the physics and the tests that pin it.

## Layout

```
crates/fathom-core     the App trait, params, camera, clock, Runner
crates/fathom-shell    the egui shell: panel, skin, drawer, native and web entry points
apps/gravity           N-body gravity
apps/ecp-life2         particle life with a colour-energy ledger
```

## Tests

```bash
cargo test --workspace
```

The suites that matter most run the real compute shaders and check them against a CPU
statement of what they should produce: gravity's tiled force kernel against a plain
O(n²) sum at N=256, and ecp-life2's whole step against a closed-form total energy. A
tiling bug drops or double-counts bodies while still *looking* plausible, which is
exactly the kind of thing an eye test misses. GPU tests skip themselves when no adapter
is available.

## Status

Desktop is built and verified on Windows (Vulkan); the shell is eframe, so macOS and
Linux should follow without changes but have not been checked here. The web build is
verified in Chrome. Firefox and Safari need WebGPU enabled.

## License

MIT. The bundled IBM Plex fonts are under the SIL Open Font License, see
`crates/fathom-shell/fonts/OFL.txt`.
