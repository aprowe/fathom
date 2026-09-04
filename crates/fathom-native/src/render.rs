//! The render thread.
//!
//! Native has no `requestAnimationFrame`, so unlike the web host this side owns its own
//! loop. Everything else is the same [`Runner`] the web host drives — the interface
//! sends the same viewport rects, parameter blocks, events and commands, and this
//! translates them into calls on it.
//!
//! The two kinds of traffic are carried differently, on purpose. Parameters, input and
//! commands are *events*: they are ordered, each one matters, and they go through a
//! channel. Where to draw, and whether the window is on screen, are *state*: only the
//! latest value matters, and they live in [`Shared`], which the loop reads every frame.
//! Keeping the viewport out of the channel also means the loop can never end up waiting
//! on a message while sitting at the wrong size.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use fathom_core::{App, FrameStats, Gpu, Runner, Viewport, ViewportRect, wgpu};

use crate::surface::OverlaySurface;

/// How long the loop idles between checks while there is nothing to draw into.
const IDLE_POLL: Duration = Duration::from_millis(16);

/// An event from the interface to the render thread.
///
/// There is deliberately no "shut down" variant. The renderer belongs to the process,
/// not to any one mount of the interface: the webview reloads on every hot update, and
/// React mounts every component twice in development. Tying the render thread's life to
/// a component's leaves a live-looking app whose simulation has quietly stopped, with
/// the interface still reporting the GPU it no longer has — which is a genuinely
/// difficult failure to read, so it is worth designing out rather than handling.
pub enum Message {
    Params(Vec<u8>),
    Input(String),
    Command { name: String, args: String },
}

/// What the interface needs before it can build itself.
#[derive(Clone, Debug)]
pub struct Boot {
    pub descriptor: String,
    pub param_byte_length: usize,
    pub adapter_info: String,
}

/// Latest-wins state shared with the thread the interface's commands run on.
pub struct Shared {
    /// Where the interface wants the simulation drawn, if it has said yet. Read by the
    /// render thread each frame, and by the host when the window itself moves.
    pub placement: Mutex<Option<ViewportRect>>,
    /// False while the window is minimised. Presenting to a window with no visible area
    /// does not fail — it blocks indefinitely — so the loop has to know.
    pub visible: AtomicBool,
    pub stats: Mutex<FrameStats>,
    /// Filled in once the GPU is up, or with the reason it is not.
    pub boot: Mutex<Option<Result<Boot, String>>>,
}

impl Default for Shared {
    fn default() -> Self {
        Self {
            placement: Mutex::new(None),
            visible: AtomicBool::new(true),
            stats: Mutex::new(FrameStats::default()),
            boot: Mutex::new(None),
        }
    }
}

pub fn run<A: App>(surface: OverlaySurface, messages: Receiver<Message>, shared: Arc<Shared>) {
    let mut state = match Renderer::<A>::new(surface) {
        Ok(state) => {
            *shared.boot.lock().unwrap() = Some(Ok(state.boot()));
            state
        }
        Err(e) => {
            log::error!("fathom native host failed to start: {e}");
            *shared.boot.lock().unwrap() = Some(Err(e));
            return;
        }
    };

    let started = Instant::now();
    loop {
        loop {
            match messages.try_recv() {
                // Every sender is gone, which only happens as the app itself goes away.
                Err(TryRecvError::Disconnected) => return,
                Ok(message) => state.handle(message),
                Err(TryRecvError::Empty) => break,
            }
        }

        if let Some(rect) = *shared.placement.lock().unwrap() {
            state.set_viewport(rect);
        }

        // Nowhere to put a frame yet: idle rather than present into a void.
        if !shared.visible.load(Ordering::Relaxed) || !state.sized() {
            std::thread::sleep(IDLE_POLL);
            continue;
        }

        state.frame(started.elapsed().as_secs_f64() * 1000.0);
        *shared.stats.lock().unwrap() = state.runner.stats();
    }
}

struct Renderer<A: App> {
    runner: Runner<A>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    _adapter: wgpu::Adapter,
    adapter_info: String,
}

impl<A: App> Renderer<A> {
    fn new(window: OverlaySurface) -> Result<Self, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = unsafe { instance.create_surface_unsafe(window.surface_target()?) }
            .map_err(|e| format!("could not create a surface on the render window: {e}"))?;

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .map_err(|e| format!("no GPU adapter available: {e}"))?;

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("fathom"),
            required_features: wgpu::Features::empty(),
            required_limits: adapter.limits(),
            memory_hints: wgpu::MemoryHints::Performance,
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            trace: wgpu::Trace::Off,
        }))
        .map_err(|e| format!("could not create a GPU device: {e}"))?;

        let mut config = surface
            .get_default_config(&adapter, 1, 1)
            .ok_or("the adapter cannot present to the render window")?;
        // Fifo paces the loop against the display, so the thread does not spin.
        config.present_mode = wgpu::PresentMode::Fifo;
        surface.configure(&device, &config);

        let adapter_info = adapter.get_info();
        let gpu = Gpu { device, queue, format: config.format, adapter_info };
        let description = gpu.describe_adapter();
        let runner = Runner::new(gpu, Viewport::new(1, 1, 1.0));

        Ok(Self { runner, surface, config, _adapter: adapter, adapter_info: description })
    }

    fn boot(&self) -> Boot {
        Boot {
            descriptor: serde_json::to_string(&Runner::<A>::descriptor())
                .unwrap_or_else(|_| "{}".into()),
            param_byte_length: self.runner.param_byte_len(),
            adapter_info: self.adapter_info.clone(),
        }
    }

    /// Whether the surface has been given a real size yet.
    fn sized(&self) -> bool {
        self.config.width > 1 && self.config.height > 1
    }

    fn handle(&mut self, message: Message) {
        match message {
            Message::Params(bytes) => self.runner.write_params(&bytes),
            Message::Input(json) => match serde_json::from_str(&json) {
                Ok(event) => self.runner.input(event),
                Err(e) => log::warn!("dropped malformed input event: {e}"),
            },
            Message::Command { name, args } => {
                let value = serde_json::from_str(&args).unwrap_or(serde_json::Value::Null);
                self.runner.command(&name, value);
            }
        }
    }

    /// The interface laid itself out. The window itself is moved by the host, on the
    /// thread that owns it; this is only the surface drawing into it.
    fn set_viewport(&mut self, rect: ViewportRect) {
        let size = rect.size();
        if self.config.width != size.width || self.config.height != size.height {
            self.config.width = size.width;
            self.config.height = size.height;
            self.surface.configure(&self.runner.gpu().device, &self.config);
        }
        self.runner.resize(size);
    }

    fn frame(&mut self, now_ms: f64) {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            other => {
                log::warn!("surface unavailable this frame: {other:?}");
                self.surface.configure(&self.runner.gpu().device, &self.config);
                // Without a present to wait on, this loop would spin at full speed.
                std::thread::sleep(IDLE_POLL);
                return;
            }
        };

        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.runner.frame(now_ms, &view);
        self.runner.gpu().queue.present(frame);
    }
}
