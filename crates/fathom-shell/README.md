# fathom-shell

A third host for the same `fathom_core::App` trait, alongside the wasm and Tauri ones.
The app crate it runs is byte-for-byte the one they run — `apps/gravity` is not modified
on this branch at all. What changes is only who draws the interface.

```bash
cargo run -p gravity --bin gravity-egui
```

## Why

The Tauri host buys a real webview for the panel, and pays for it:

| | Tauri host | egui shell |
|---|---|---|
| Windows | **two** HWNDs, kept in step by hand | one |
| Alt-tab / taskbar preview | sim area black — DWM composes the preview from the Tauri window alone | the whole window |
| Parameter edit | control → JS mirror → IPC → block | control → block |
| Interface | React, HTML, CSS | Rust |
| Per-frame serialisation | a parameter block each frame | none |

Minimise, maximise, resize, snap, alt-tab and per-monitor DPI all work here because
there is one window and the OS is not being tricked into treating two as one.

## How it fits together

The shell owns a `Runner<A>` exactly as the other hosts do, so the frame loop, camera,
clock, parameter block and command dispatch are all the shared ones. Two things are
specific to it:

**The simulation renders into an offscreen texture**, which the panel then draws as an
image. That is what keeps the app's `draw` unchanged — it still receives a target view
and renders into it, exactly as when that view is a swapchain. The texture is registered
with egui's renderer and re-registered when the viewport resizes.

**The panel is built from the app's schema**, the same way `<AutoControls/>` is on the
web. It is the same bargain in a different language: declaring a parameter is most of the
work of getting a control for it. Because the panel and the simulation share a process,
a slider writes straight into the live `ParamBlock` through `Runner::params_mut` — there
is no mirror and no serialisation anywhere in the frame.

## Both targets

`run_native` uses eframe's winit + wgpu backend. `run_web` starts the same shell through
`eframe::WebRunner`, where egui draws to a canvas and wgpu talks to WebGPU — so no part
of the interface is HTML on either target, and this host covers both on its own.

The web entry point is written but **not yet built or verified**; only the native path
has been run.
