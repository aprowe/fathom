# ecp-life2

Particle life with **relational colour energy**, on the GPU, built on fathom.

```bash
cargo run --release -p ecp-life2 --bin ecp-life2-egui
```

Each particle carries a continuous colour on a circle. How two particles interact is a
smooth surface over the *pair* of their colours — not a matrix of N×N numbers — and that
surface splits into an even part, which is an ordinary reciprocal pair force, and an odd
part, which is not. The odd part pushes both members of a pair the same way along their
separation. It is the only thing in the system that does net work.

That work is paid for, locally and in the same step, out of energy stored in the colour
mismatch between neighbours. Colours shift to balance the books.

## The four things it does

**A smooth interaction landscape.** `A(a,b)` is a truncated Fourier series over the colour
torus — analytic by construction, so the force field has no cliffs and the derivatives the
energy ledger needs come out in closed form rather than from a finite difference.
Smoothness is one integer: the harmonic order. Baked to a 256² table the shader samples,
split into `S` (even) and `Λ` (odd) at bake time.

**A distance interaction profile**, switchable:

- *Classic* — `S·V(r)` with `V` monotonic. Binds reliably, but every pair wants the same
  separation and no relationship can be repulsive at range.
- *Well* — a genuine minimum at a **colour-dependent** distance, drawn from a second
  landscape. Colour then says how far apart two particles want to be, the force changes
  sign across that distance, and the landscape gains repulsive regions the classic profile
  cannot express.

**Presets** — five compiled-in starting points. A preset carries its landscape seed and
detail as well as its slider values, because the terrain matters as much as how hard you
push on it.

**Randomise** — <kbd>N</kbd> or the button for a new landscape, <kbd>R</kbd> for new
particles.

## The chase

`Chase gain` is the control that decides whether anything is alive. With it at zero the
system settles into static clumps and stops — measured mean speed 1.4e-4, mean force 0.1.
With it at the default 0.4 the same system churns: speed 0.085, force 190, and colour
turning over continuously as the engine burns the mismatch and regenerates it.

The chase is divided by the cutoff, like every other force here, so the gain means the
same thing at any cutoff. It did not used to be: the reciprocal terms are all gradients of
an energy and pick up a `1/R` from differentiating their envelopes, while the chase was
declared directly as a force and never did. That left it at five per cent of the total
force at a cutoff of 0.02, and a *different* five per cent at every other cutoff.

## Instruments

Two panels in the corners of the view, drawn from the same landscape buffer and the same
`pair_terms` the force pass runs — so a chart cannot be right while the simulation is
wrong. Bottom left is `A(a,b)` over the colour torus, warm for attraction and blue for
repulsion, with the diagonal `a = b` marked (the chase is exactly zero along it). Bottom
right is the radial force profile for the probe pair of colours. The two probe sliders
sweep you continuously across the landscape.

## Layout

```
src/landscape.rs   the Fourier surface, its split, and the bake
src/physics.rs     slider → absolute settings, and a CPU statement of the total energy
src/sim.rs         buffers, pipelines, and the passes of a frame
src/scenes.rs      initial conditions
src/presets.rs     places worth starting from
src/shaders/       common (the pair physics), lut, grid, force, integrate, draw, overlay
```

The world is the unit torus, which makes the cutoff `R` the only length anyone has to
think about — every other distance is a fraction of it — and minimum-image separation a
single `round`.

Per substep: `clear → count → scan → scatter` rebuilds a uniform grid, `force` accumulates
total force, chase force and `dE/dc` separately, `integrate` moves and settles up. The
cell side is never smaller than the cutoff, so the 3×3 neighbourhood scan is exact rather
than an approximation.

## Tests

```bash
cargo test -p ecp-life2
```

`tests/conservation.rs` runs the **real compute shaders** against a CPU statement of the
total energy. What it measures is the peak excursion of that energy over a run, not its
final value: a symplectic integrator makes energy oscillate within a bound set by the
timestep, so sampling only the end reads off wherever the oscillation happened to stop.

Two claims, which fail for different reasons and are tested apart:

- **With the chase off**, the system is conservative and its error must be the
  integrator's — first order, halving with the step. Measured ratios 2.01 and 2.02.
- **With the chase running**, the well profile holds its energy to 6e-5, about a
  hundredth of a percent, with an engine doing net work the whole time.

What is left over is the *table's* resolution, not the ledger's arithmetic. The colour
update pays `W·ċ·dt = −P·dt` by dividing exactly, but `W` is read from the baked table,
and an interpolated derivative is not the derivative of the interpolated energy. That
mismatch is a fixed fraction of the work done, so it is a residual per unit *time* and
does not move when the timestep does — which is precisely what
`what_the_chase_leaves_behind_is_the_tables_resolution_not_the_timestep` asserts. Doubling
`LUT_SIZE` cut it 4.7×; halving the timestep does nothing to it. That is why the table is
256 rather than 128.

The earlier version of this used `α = P/(W² + ε)`, which looks harmless and is not: it
pays back `W²/(W² + ε)` of the work and keeps the rest, per unit time rather than per step.
It was invisible while the chase was fifty times too weak and became six per cent of the
total energy the moment the chase was scaled correctly.

`tests/renders.rs` drives the render path headlessly and checks the pixels.

## A note on fathom

This app drove two framework changes:

- `CommandCtx` can *write* parameters (`set_float` / `set_int`), because applying a preset
  means moving the sliders. Every other context stays read-only — elsewhere the interface
  is the author of those values, and an app writing underneath it would fight the widget
  the user is holding.
- `ParamDef::advanced()` files a control behind its group's disclosure. Twenty-two
  parameters shown at once show nothing in particular; nine of these are the tail that
  exists so the first thirteen can be trusted.
