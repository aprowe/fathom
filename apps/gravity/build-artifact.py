"""Bundle the gravity app into one self-contained HTML page.

Artifacts cannot fetch anything, so the wasm module is embedded as base64 and handed
straight to wasm-bindgen as bytes, and the interface is a small vanilla-JS port of
`@fathom/ui` that builds itself from the same descriptor the React panel uses.

Run after `wasm-pack build --target web --out-dir pkg --release`.
"""

import base64
import pathlib

HERE = pathlib.Path(__file__).parent
PKG = HERE / "pkg"
OUT = HERE / "gravity-artifact.html"

glue = (PKG / "gravity.js").read_text(encoding="utf-8").replace("</script", "<\\/script")
wasm_b64 = base64.b64encode((PKG / "gravity_bg.wasm").read_bytes()).decode("ascii")

TEMPLATE = r"""<title>Fathom Gravity</title>
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:wght@400;500&family=IBM+Plex+Sans:wght@400;500;600&display=swap">
<style>
/*
 * The chrome is fathom's own: a cool slate casing built like a measurement instrument
 * rather than an app, with the accent lifted from the simulation's colour ramp so the
 * panel belongs to the thing it controls. Deliberately single-theme — the simulation is
 * an additive HDR render on black, and a light ground would fight it — so every colour
 * is painted explicitly and nothing is inherited from the host.
 */
:root {
  --ground: #0e1116;
  --panel: #121722;
  --raised: #1a212c;
  --line: #232c3a;
  --text: #dce3ed;
  --muted: #7c8899;
  --accent: #6fd2ff;
  --warm: #ffc98a;
  --sans: 'IBM Plex Sans', ui-sans-serif, system-ui, sans-serif;
  --mono: 'IBM Plex Mono', ui-monospace, 'SF Mono', monospace;
  color-scheme: dark;
}

* { box-sizing: border-box; }

html, body { height: 100%; margin: 0; }

body {
  background: var(--ground);
  color: var(--text);
  font-family: var(--sans);
  font-size: 13px;
  line-height: 1.45;
  -webkit-font-smoothing: antialiased;
  overflow: hidden;
}

.app {
  display: grid;
  grid-template-columns: 1fr 300px;
  height: 100%;
}

.stage {
  position: relative;
  min-width: 0;
  overflow: hidden;
  background: #05070b;
}

canvas {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  display: block;
  cursor: crosshair;
  touch-action: none;
}

/* Panel ------------------------------------------------------------------- */

.panel {
  display: flex;
  flex-direction: column;
  min-height: 0;
  background: var(--panel);
  border-left: 1px solid var(--line);
}

.panel-head { padding: 18px 18px 14px; border-bottom: 1px solid var(--line); }
.panel-head h1 { margin: 0; font-size: 17px; font-weight: 600; letter-spacing: -0.01em; }
.panel-head p {
  margin: 3px 0 0;
  color: var(--muted);
  font-size: 11px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.panel-body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 4px 18px 24px;
  scrollbar-width: thin;
  scrollbar-color: var(--line) transparent;
}

.group { padding: 16px 0 4px; }
.group + .group { border-top: 1px solid var(--line); }
.group h2 { margin: 0 0 12px; font-size: 12px; font-weight: 600; color: var(--muted); }

/* Controls ---------------------------------------------------------------- */

.control { display: block; margin-bottom: 14px; }

.control-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 6px;
}

.readout {
  font-family: var(--mono);
  font-size: 11px;
  font-variant-numeric: tabular-nums;
  color: var(--accent);
}

input[type='range'] {
  -webkit-appearance: none;
  appearance: none;
  display: block;
  width: 100%;
  height: 16px;
  margin: 0;
  background: transparent;
  cursor: ew-resize;
}

input[type='range']::-webkit-slider-runnable-track {
  height: 2px;
  border-radius: 1px;
  background: linear-gradient(to right, var(--accent) var(--fill, 0%), var(--line) var(--fill, 0%));
}

/* A needle, not a knob: this is a scale being read, not a switch being thrown. */
input[type='range']::-webkit-slider-thumb {
  -webkit-appearance: none;
  appearance: none;
  width: 2px;
  height: 14px;
  margin-top: -6px;
  border-radius: 1px;
  background: var(--text);
  transition: background 90ms linear;
}

input[type='range']:hover::-webkit-slider-thumb,
input[type='range']:focus-visible::-webkit-slider-thumb { background: var(--accent); }

input[type='range']::-moz-range-track { height: 2px; border-radius: 1px; background: var(--line); }
input[type='range']::-moz-range-progress { height: 2px; border-radius: 1px; background: var(--accent); }
input[type='range']::-moz-range-thumb {
  width: 2px;
  height: 14px;
  border: 0;
  border-radius: 1px;
  background: var(--text);
}

.row { display: flex; align-items: center; justify-content: space-between; gap: 12px; }

.switch {
  flex: none;
  width: 30px;
  height: 16px;
  padding: 2px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: var(--raised);
  cursor: pointer;
}

.switch span {
  display: block;
  width: 10px;
  height: 10px;
  border-radius: 50%;
  background: var(--muted);
  transition: transform 120ms ease, background 120ms linear;
}

.switch[aria-checked='true'] { border-color: rgba(111, 210, 255, 0.45); }
.switch[aria-checked='true'] span { transform: translateX(14px); background: var(--accent); }

select {
  flex: 1;
  max-width: 152px;
  padding: 5px 8px;
  border: 1px solid var(--line);
  border-radius: 4px;
  background: var(--raised);
  color: var(--text);
  font-family: inherit;
  font-size: 12px;
  cursor: pointer;
}

.buttons { display: flex; gap: 8px; margin-top: 12px; }

button.action {
  flex: 1;
  padding: 7px 10px;
  border: 1px solid var(--line);
  border-radius: 4px;
  background: var(--raised);
  color: var(--text);
  font-family: inherit;
  font-size: 12px;
  cursor: pointer;
  transition: border-color 90ms linear, color 90ms linear;
}

button.action:hover { border-color: rgba(111, 210, 255, 0.5); color: var(--accent); }

.note {
  margin: 20px 0 0;
  padding-top: 16px;
  border-top: 1px solid var(--line);
  color: var(--muted);
  font-size: 11px;
  line-height: 1.55;
}

.note strong { color: var(--text); font-weight: 500; }

/* Toolbar ----------------------------------------------------------------- */

.toolbar {
  position: absolute;
  left: 16px;
  bottom: 16px;
  z-index: 2;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 7px 12px;
  border: 1px solid var(--line);
  border-radius: 6px;
  background: rgba(14, 17, 22, 0.76);
  backdrop-filter: blur(14px);
  box-shadow: 0 8px 28px rgba(0, 0, 0, 0.45);
}

.transport {
  display: grid;
  place-items: center;
  width: 24px;
  height: 24px;
  padding: 0;
  border: 0;
  border-radius: 4px;
  background: transparent;
  color: var(--text);
  cursor: pointer;
  transition: color 90ms linear, background 90ms linear;
}

.transport:hover:not(:disabled) { color: var(--accent); background: var(--raised); }
.transport:disabled { color: rgba(124, 136, 153, 0.55); cursor: default; }

.rule { width: 1px; height: 16px; background: var(--line); }

.stat { color: var(--muted); font-size: 11px; }
.stat b {
  font-family: var(--mono);
  font-weight: 500;
  font-variant-numeric: tabular-nums;
  color: var(--text);
}

.state { display: flex; align-items: center; gap: 6px; font-size: 11px; color: var(--muted); }
.state::before { content: ''; width: 6px; height: 6px; border-radius: 50%; background: var(--accent); }
.state.paused::before { background: var(--warm); }

/* Startup and failure ----------------------------------------------------- */

.splash, .failure {
  display: grid;
  place-content: center;
  gap: 10px;
  height: 100%;
  padding: 40px;
}

.splash { text-align: center; color: var(--muted); }

.mark {
  justify-self: center;
  width: 22px;
  height: 22px;
  border: 2px solid var(--line);
  border-top-color: var(--accent);
  border-radius: 50%;
  animation: spin 900ms linear infinite;
}

@keyframes spin { to { transform: rotate(1turn); } }

.failure { max-width: 46ch; margin: 0 auto; }
.failure h1 { margin: 0; font-size: 19px; font-weight: 600; }
.failure p { margin: 0; color: var(--muted); }
.failure .reason {
  padding: 10px 12px;
  border-left: 2px solid var(--warm);
  background: var(--raised);
  color: var(--text);
  font-family: var(--mono);
  font-size: 11px;
  word-break: break-word;
}
.failure code { font-family: var(--mono); color: var(--text); }

:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }

@media (prefers-reduced-motion: reduce) {
  *, *::before, *::after {
    animation-duration: 0.01ms !important;
    animation-iteration-count: 1 !important;
    transition-duration: 0.01ms !important;
  }
}

@media (max-width: 760px) {
  .app { grid-template-columns: 1fr; grid-template-rows: 1fr auto; }
  .panel { border-left: 0; border-top: 1px solid var(--line); max-height: 48vh; }
}
</style>

<div id="root">
  <div class="splash"><span class="mark"></span><p>Starting the simulation</p></div>
</div>

<script type="module">
// ---------------------------------------------------------------------------
// The app, compiled from Rust to wasm. Embedded rather than fetched, because an
// artifact cannot load anything over the network.
// ---------------------------------------------------------------------------
__GLUE__

const WASM_BASE64 = "__WASM__";

function wasmBytes() {
  const binary = atob(WASM_BASE64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

// ---------------------------------------------------------------------------
// The interface. A small port of @fathom/ui: it builds every control from the
// schema the app declares, so this file knows nothing about gravity in particular.
// ---------------------------------------------------------------------------

const el = (tag, className, text) => {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
};

/**
 * The interface's mirror of the parameter block. Controls write into the typed array
 * directly and the whole block is flushed once per animation frame — dragging a slider
 * fires dozens of events a second, and a round trip per event would cost more than the
 * simulation does.
 */
class ParamMirror {
  constructor(schema, byteLength) {
    const buffer = new ArrayBuffer(Math.max(byteLength, schema.length * 4, 16));
    this.bytes = new Uint8Array(buffer);
    this.floats = new Float32Array(buffer);
    this.ints = new Uint32Array(buffer);
    this.defs = new Map();
    this.dirty = true;
    schema.forEach((def, index) => {
      this.defs.set(def.name, { index, def });
      this.write(index, def, def.default);
    });
  }

  write(index, def, value) {
    const clamped = Math.min(Math.max(value, def.min), def.max);
    if (def.kind === 'float') this.floats[index] = clamped;
    else this.ints[index] = Math.round(clamped);
  }

  get(name) {
    const { index, def } = this.defs.get(name);
    return def.kind === 'float' ? this.floats[index] : this.ints[index];
  }

  set(name, value) {
    const entry = this.defs.get(name);
    if (!entry) throw new Error(`No parameter named "${name}"`);
    this.write(entry.index, entry.def, value);
    this.dirty = true;
  }

  takeDirty() {
    if (!this.dirty) return null;
    this.dirty = false;
    return this.bytes;
  }
}

function formatValue(def, value) {
  if (def.kind !== 'float') return String(Math.round(value));
  const span = def.max - def.min;
  return value.toFixed(span >= 20 ? 0 : span >= 2 ? 2 : 3);
}

function slider(def, params) {
  const wrap = el('label', 'control');
  const head = el('span', 'control-head');
  head.append(el('span', null, def.label));
  const readout = el('span', 'readout', formatValue(def, params.get(def.name)));
  head.append(readout);

  const input = el('input');
  input.type = 'range';
  input.min = def.min;
  input.max = def.max;
  input.step = def.step > 0 ? def.step : (def.max - def.min) / 1000;
  input.value = params.get(def.name);
  const fill = (v) => `${((v - def.min) / (def.max - def.min)) * 100}%`;
  input.style.setProperty('--fill', fill(Number(input.value)));
  input.addEventListener('input', () => {
    const value = Number(input.value);
    params.set(def.name, value);
    input.style.setProperty('--fill', fill(value));
    readout.textContent = formatValue(def, value);
  });

  wrap.append(head, input);
  return wrap;
}

function toggle(def, params) {
  const wrap = el('label', 'control row');
  wrap.append(el('span', null, def.label));
  const button = el('button', 'switch');
  button.type = 'button';
  button.setAttribute('role', 'switch');
  let on = params.get(def.name) !== 0;
  button.setAttribute('aria-checked', String(on));
  button.append(el('span'));
  button.addEventListener('click', () => {
    on = !on;
    button.setAttribute('aria-checked', String(on));
    params.set(def.name, on ? 1 : 0);
  });
  wrap.append(button);
  return wrap;
}

function choice(label, options, initial, onChange) {
  const wrap = el('label', 'control row');
  wrap.append(el('span', null, label));
  const select = el('select');
  options.forEach((option, i) => {
    const node = el('option', null, option);
    node.value = String(i);
    select.append(node);
  });
  select.value = String(initial);
  select.addEventListener('change', () => onChange(Number(select.value)));
  wrap.append(select);
  return wrap;
}

const SVG_NS = 'http://www.w3.org/2000/svg';

/** One 14px icon from a list of shapes, so a glyph that mixes fill and stroke stays a
 *  single element rather than two overlapping ones. */
function icon(shapes) {
  const svg = document.createElementNS(SVG_NS, 'svg');
  svg.setAttribute('width', '14');
  svg.setAttribute('height', '14');
  svg.setAttribute('viewBox', '0 0 14 14');
  svg.setAttribute('aria-hidden', 'true');
  for (const shape of shapes) {
    const node = document.createElementNS(SVG_NS, shape.tag || 'path');
    for (const [key, value] of Object.entries(shape.attrs)) node.setAttribute(key, value);
    if (shape.filled) {
      node.setAttribute('fill', 'currentColor');
    } else {
      node.setAttribute('fill', 'none');
      node.setAttribute('stroke', 'currentColor');
      node.setAttribute('stroke-width', '1.6');
      node.setAttribute('stroke-linecap', 'round');
      node.setAttribute('stroke-linejoin', 'round');
    }
    svg.append(node);
  }
  return svg;
}

function failure(message) {
  const root = document.getElementById('root');
  root.replaceChildren();
  const card = el('div', 'failure');
  card.setAttribute('role', 'alert');
  card.append(el('h1', null, "This browser can't reach a GPU"));
  card.append(el('p', 'reason', message));
  const help = el('p');
  help.append(
    document.createTextNode('The simulation draws through WebGPU. Chrome and Edge 113 or newer support it, as does Safari 26. In Firefox, enable '),
  );
  help.append(el('code', null, 'dom.webgpu.enabled'));
  help.append(document.createTextNode(' and restart.'));
  card.append(help);
  root.append(card);
}

async function main() {
  try {
    await __wbg_init(wasmBytes());
  } catch (error) {
    failure(`The simulation could not start: ${error}`);
    return;
  }

  const root = document.getElementById('root');
  const app = el('div', 'app');
  const stage = el('div', 'stage');
  const canvas = el('canvas');
  stage.append(canvas);

  let sim;
  try {
    sim = await FathomApp.create(canvas);
  } catch (error) {
    failure(String(error && error.message ? error.message : error));
    return;
  }

  const descriptor = JSON.parse(sim.descriptor());
  const params = new ParamMirror(descriptor.params, sim.paramByteLength());

  // Panel -------------------------------------------------------------------
  const panel = el('aside', 'panel');
  const head = el('div', 'panel-head');
  head.append(el('h1', null, descriptor.name));
  const adapter = el('p', null, sim.adapterInfo());
  adapter.title = sim.adapterInfo();
  head.append(adapter);
  const body = el('div', 'panel-body');
  panel.append(head, body);

  // One group per declared group, in declaration order. This is the payoff of the
  // schema: the app gets a complete panel without describing any of it.
  const groups = [];
  const group = (name) => {
    let found = groups.find((g) => g.name === name);
    if (!found) {
      found = { name, params: [], commands: [] };
      groups.push(found);
    }
    return found;
  };
  for (const def of descriptor.params) group(def.group).params.push(def);
  for (const def of descriptor.commands) group(def.group).commands.push(def);

  for (const g of groups) {
    const section = el('section', 'group');
    section.append(el('h2', null, g.name));
    for (const def of g.params) {
      if (def.kind === 'toggle') section.append(toggle(def, params));
      else if (def.kind === 'choice') {
        section.append(choice(def.label, def.options, params.get(def.name), (v) => params.set(def.name, v)));
      } else section.append(slider(def, params));
    }
    for (const def of g.commands.filter((c) => c.options.length > 0)) {
      section.append(
        choice(def.label, def.options, def.initial, (v) => sim.command(def.name, JSON.stringify({ value: v }))),
      );
    }
    const buttons = g.commands.filter((c) => c.options.length === 0);
    if (buttons.length) {
      const row = el('div', 'buttons');
      for (const def of buttons) {
        const button = el('button', 'action', def.label);
        button.type = 'button';
        button.addEventListener('click', () => sim.command(def.name, '{}'));
        row.append(button);
      }
      section.append(row);
    }
    body.append(section);
  }

  const note = el('p', 'note');
  note.append(el('strong', null, 'Every body pulls on every other body, exactly. '));
  note.append(
    document.createTextNode(
      'No approximation: the force pass is quadratic, tiled through workgroup shared memory so tens of thousands of bodies still run at frame rate. Drag in the view to pull bodies toward the cursor; shift-drag or middle-drag to pan, scroll to zoom, press R to reseed.',
    ),
  );
  body.append(note);

  // Toolbar -----------------------------------------------------------------
  const toolbar = el('div', 'toolbar');
  let paused = false;

  const playPause = el('button', 'transport');
  playPause.type = 'button';
  playPause.setAttribute('aria-label', 'Pause');
  const pauseIcon = icon([{ attrs: { d: 'M5 2.5v9M9 2.5v9' } }]);
  const playIcon = icon([{ attrs: { d: 'M4 2.6 11 7l-7 4.4z' }, filled: true }]);
  playPause.append(pauseIcon);

  const step = el('button', 'transport');
  step.type = 'button';
  step.disabled = true;
  step.setAttribute('aria-label', 'Step one frame');
  step.append(
    icon([
      { attrs: { d: 'M3 2.6 9 7l-6 4.4z' }, filled: true },
      { attrs: { d: 'M11 2.6v8.8' } },
    ]),
  );

  const recentre = el('button', 'transport');
  recentre.type = 'button';
  recentre.setAttribute('aria-label', 'Recentre the view');
  recentre.append(
    icon([
      { tag: 'circle', attrs: { cx: '7', cy: '7', r: '3.2' } },
      { attrs: { d: 'M7 1v1.8M7 11.2V13M1 7h1.8M11.2 7H13' } },
    ]),
  );

  const fps = el('span', 'stat');
  const fpsValue = el('b', null, '0');
  fps.append(fpsValue, document.createTextNode(' fps'));
  const ms = el('span', 'stat');
  const msValue = el('b', null, '0.0');
  ms.append(msValue, document.createTextNode(' ms'));
  const state = el('span', 'state', 'Running');

  toolbar.append(playPause, step, recentre, el('span', 'rule'), fps, ms, state);

  playPause.addEventListener('click', () => {
    paused = !paused;
    sim.command(paused ? 'fathom.pause' : 'fathom.resume', '{}');
    playPause.replaceChildren(paused ? playIcon : pauseIcon);
    playPause.setAttribute('aria-label', paused ? 'Run' : 'Pause');
    step.disabled = !paused;
    state.textContent = paused ? 'Paused' : 'Running';
    state.className = paused ? 'state paused' : 'state';
  });
  step.addEventListener('click', () => sim.command('fathom.step', '{}'));
  recentre.addEventListener('click', () => sim.command('fathom.reset_camera', '{}'));

  stage.append(toolbar);
  app.append(stage, panel);
  root.replaceChildren(app);

  // Viewport ----------------------------------------------------------------
  // The interface owns layout and tells the app where to draw, which is the same
  // contract the native target uses to position a window.
  const report = () => {
    const box = stage.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    sim.setViewport(Math.max(1, Math.round(box.width * dpr)), Math.max(1, Math.round(box.height * dpr)), dpr);
  };
  new ResizeObserver(report).observe(stage);
  window.addEventListener('resize', report);
  report();

  // Input -------------------------------------------------------------------
  const local = (e) => {
    const box = canvas.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    return { x: (e.clientX - box.left) * dpr, y: (e.clientY - box.top) * dpr };
  };
  const mouse = (kind, e) => ({
    kind,
    ...local(e),
    button: e.button,
    buttons: e.buttons,
    shift: e.shiftKey,
    ctrl: e.ctrlKey,
    alt: e.altKey,
  });

  let dragging = false;
  canvas.addEventListener('pointerdown', (e) => {
    dragging = true;
    canvas.setPointerCapture(e.pointerId);
    sim.input(JSON.stringify(mouse('mousePressed', e)));
  });
  canvas.addEventListener('pointermove', (e) => {
    sim.input(JSON.stringify(mouse(dragging ? 'mouseDragged' : 'mouseMoved', e)));
  });
  const release = (e) => {
    dragging = false;
    sim.input(JSON.stringify(mouse('mouseReleased', e)));
  };
  canvas.addEventListener('pointerup', release);
  canvas.addEventListener('pointercancel', release);
  canvas.addEventListener(
    'wheel',
    (e) => {
      e.preventDefault();
      sim.input(
        JSON.stringify({ kind: 'scrolled', ...local(e), deltaY: e.deltaY, shift: e.shiftKey, ctrl: e.ctrlKey, alt: e.altKey }),
      );
    },
    { passive: false },
  );
  canvas.addEventListener('contextmenu', (e) => e.preventDefault());
  window.addEventListener('keydown', (e) => {
    const target = e.target;
    if (target instanceof HTMLElement && (target.tagName === 'INPUT' || target.tagName === 'SELECT')) return;
    sim.input(JSON.stringify({ kind: 'keyPressed', key: e.key, shift: e.shiftKey, ctrl: e.ctrlKey, alt: e.altKey }));
  });

  // Frame loop --------------------------------------------------------------
  let lastStats = 0;
  const tick = (now) => {
    requestAnimationFrame(tick);
    const dirty = params.takeDirty();
    if (dirty) sim.writeParams(dirty);
    sim.frame(now);
    if (now - lastStats > 400) {
      lastStats = now;
      const s = JSON.parse(sim.stats());
      fpsValue.textContent = s.fps.toFixed(0);
      msValue.textContent = s.frameMs.toFixed(1);
    }
  };
  requestAnimationFrame(tick);
}

main().catch((error) => failure(String(error && error.message ? error.message : error)));
</script>
"""

html = TEMPLATE.replace("__GLUE__", glue).replace("__WASM__", wasm_b64)
OUT.write_text(html, encoding="utf-8")
print(f"wrote {OUT} ({len(html) / 1024:.0f} KB)")
