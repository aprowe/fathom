//! The render thread.
//!
//! Native has no `requestAnimationFrame`, so unlike the web host this side owns its own
//! loop. Everything else is the same [`Runner`] the web host drives — the interface
//! sends the same viewport rects, parameter blocks, events and commands, and this
//! translates them into calls on it.

use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use fathom_core::{App, FrameStats, Gpu, Runner, Viewport, ViewportRect, wgpu};

use crate::child::ChildSurface;

/// A message from the interface to the render thread.
pub enum Message {
    Viewport(ViewportRect),
    Params(Vec<u8>),
    Input(String),
    Command { name: String, args: String },
    Shutdown,
}

/// What the interface needs before it can build itself.
#[derive(Clone, Debug)]
pub struct Boot {
    pub descriptor: String,
    pub param_byte_length: usize,
    pub adapter_info: String,
}

/// Filled in once the GPU is up, or with the reason it is not.
pub type BootResult = Arc<Mutex<Option<Result<Boot, String>>>>;

pub fn run<A: App>(
    surface: ChildSurface,
    messages: Receiver<Message>,
    stats: Arc<Mutex<FrameStats>>,
    boot: BootResult,
) {
    let mut state = match Renderer::<A>::new(surface) {
        Ok(state) => {
            *boot.lock().unwrap() = Some(Ok(state.boot()));
            state
        }
        Err(e) => {
            log::error!("fathom native host failed to start: {e}");
            *boot.lock().unwrap() = Some(Err(e));
            return;
        }
    };

    let started = Instant::now();
    loop {
        loop {
            match messages.try_recv() {
                Ok(Message::Shutdown) | Err(TryRecvError::Disconnected) => return,
                Ok(message) => state.handle(message),
                Err(TryRecvError::Empty) => break,
            }
        }

        state.frame(started.elapsed().as_secs_f64() * 1000.0);
        *stats.lock().unwrap() = state.runner.stats();
    }
}

struct Renderer<A: App> {
    runner: Runner<A>,
    surface: wgpu::Surface<'static>,
    child: ChildSurface,
    config: wgpu::SurfaceConfiguration,
    _adapter: wgpu::Adapter,
    adapter_info: String,
}

impl<A: App> Renderer<A> {
    fn new(child: ChildSurface) -> Result<Self, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = unsafe { instance.create_surface_unsafe(child.surface_target()?) }
            .map_err(|e| format!("could not create a surface on the child window: {e}"))?;

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
            .ok_or("the adapter cannot present to the child window")?;
        // Fifo paces the loop against the display, so the render thread does not spin.
        config.present_mode = wgpu::PresentMode::Fifo;
        surface.configure(&device, &config);

        let adapter_info = adapter.get_info();
        let gpu = Gpu { device, queue, format: config.format, adapter_info: adapter_info.clone() };
        let description = gpu.describe_adapter();
        let runner = Runner::new(gpu, Viewport::new(1, 1, 1.0));

        Ok(Self {
            runner,
            surface,
            child,
            config,
            _adapter: adapter,
            adapter_info: description,
        })
    }

    fn boot(&self) -> Boot {
        Boot {
            descriptor: serde_json::to_string(&Runner::<A>::descriptor())
                .unwrap_or_else(|_| "{}".into()),
            param_byte_length: self.runner.param_byte_len(),
            adapter_info: self.adapter_info.clone(),
        }
    }

    fn handle(&mut self, message: Message) {
        match message {
            Message::Viewport(rect) => self.set_viewport(rect),
            Message::Params(bytes) => self.runner.write_params(&bytes),
            Message::Input(json) => match serde_json::from_str(&json) {
                Ok(event) => self.runner.input(event),
                Err(e) => log::warn!("dropped malformed input event: {e}"),
            },
            Message::Command { name, args } => {
                let value = serde_json::from_str(&args).unwrap_or(serde_json::Value::Null);
                self.runner.command(&name, value);
            }
            Message::Shutdown => {}
        }
    }

    /// The interface laid itself out; move the child window to match.
    fn set_viewport(&mut self, rect: ViewportRect) {
        self.child.set_rect(rect.x, rect.y, rect.width, rect.height);

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
                std::thread::sleep(std::time::Duration::from_millis(16));
                return;
            }
        };

        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.runner.frame(now_ms, &view);
        self.runner.gpu().queue.present(frame);
    }
}
