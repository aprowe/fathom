# fathom-shell

The host for `fathom_core::App`: one native window with an egui panel, and the same
shell compiled to a browser canvas.

```bash
cargo run --release -p gravity --bin gravity-egui       # desktop

cd apps/gravity && wasm-pack build --target web --out-dir pkg --release
python -m http.server 8099                               # then open /web/
```

## How it fits together

The shell owns a `Runner<A>`, so the frame loop, camera, clock, parameter block and
command dispatch are all `fathom-core`'s. Two things are specific to the shell:

**The simulation renders into an offscreen texture**, which the panel then draws as an
image. That is what keeps the app's `draw` unchanged — it still receives a target view
and renders into it, exactly as it would if that view were a swapchain. The texture is
registered with egui's renderer and re-registered when the viewport resizes.

**The panel is built from the app's schema.** Declaring a parameter is most of the work
of getting a control for it. Because the panel and the simulation share a process, a
slider writes straight into the live `ParamBlock` through `Runner::params_mut` — there
is no mirror and no serialisation anywhere in the frame.

## Both targets

`run_native` uses eframe's winit + wgpu backend. `run_web` starts the same shell through
`eframe::WebRunner`, where egui draws to a canvas and wgpu talks to WebGPU — so no part
of the interface is HTML on either target. The only HTML in the web build is a page
whose whole job is to hand egui a canvas.

Below 760pt the docked panel becomes a drawer that slides in over the simulation. It
overlays rather than pushing the viewport aside, and not only for looks: the simulation
texture is sized from the central panel, so a drawer that pushed would reallocate that
texture — and the app's trail buffer with it — on every frame of the slide.

## The skin

egui has a strong default appearance, and three things give it away: the typeface, the
widget shapes, and a flat grey palette. `skin.rs` replaces all three.

* **Type** — IBM Plex Sans and Plex Mono, embedded (OFL, `fonts/OFL.txt`), so neither
  build fetches anything at runtime. Numerals are mono and tabular so a value does not
  shift sideways as it changes.
* **Widgets** — the sliders and toggles are drawn, not configured: a row that *is* the
  reading, with a bright edge where the value falls, and a sliding pill rather than a
  tick-box. egui's hover-expansion is switched off, because that bounce is one of its
  tells.
* **Palette** — a cool slate casing built like the front of a measurement instrument,
  with the accent lifted from the simulation's own colour ramp so the panel belongs to
  what it controls.

`control.rs` holds the arithmetic behind the widgets — how far a drag moves a value, how
many decimals to show, whether a row of buttons fits — as pure functions, so the parts
of a control that are actually *wrong* when it misbehaves are the parts a test can reach.
