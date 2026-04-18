//! Reactive CPU rasterization and Win32 GDI presentation. No GPU context or frame loop.
use crate::{raster, render_diagnostics as metrics};
use anyhow::{Result, anyhow, ensure};
use egui_software_backend::{BufferMutRef, ColorFieldOrder, EguiSoftwareRender};
use std::{
    num::NonZeroU32,
    sync::Arc,
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowId},
};

pub trait Application {
    fn logic(&mut self, ctx: &egui::Context);
    fn ui(&mut self, ui: &mut egui::Ui);
    fn on_exit(&mut self);
}
type Factory = Box<dyn FnOnce(&egui::Context, Arc<Window>) -> Result<Box<dyn Application>>>;
#[derive(Debug)]
struct Wake(Instant);
struct Root {
    window: Arc<Window>,
    surface: softbuffer::Surface<Arc<Window>, Arc<Window>>,
    ctx: egui::Context,
    input: egui_winit::State,
    info: egui::ViewportInfo,
    app: Box<dyn Application>,
    raster: EguiSoftwareRender,
    pixels: Vec<[u8; 4]>,
    bitmap: Vec<u32>,
    bitmap_size: [u32; 2],
    dirty: bool,
    occluded: bool,
}
impl Root {
    fn hidden(&self) -> bool {
        self.occluded
            || self.window.is_minimized() == Some(true)
            || self.window.is_visible() == Some(false)
    }
    fn commands(
        &mut self,
        commands: Vec<egui::ViewportCommand>,
        requested_close: bool,
        event_loop: &ActiveEventLoop,
    ) {
        let close = requested_close || commands.contains(&egui::ViewportCommand::Close);
        let cancel = commands.contains(&egui::ViewportCommand::CancelClose);
        let mut actions = Vec::new();
        egui_winit::process_viewport_commands(
            &self.ctx,
            &mut self.info,
            commands,
            &self.window,
            &mut actions,
        );
        // No screenshot/readback command is implemented by this host.
        if close && !cancel {
            event_loop.exit();
        }
        self.info.events.clear();
    }
    fn present(&mut self) -> Result<()> {
        let [w, h] = self.bitmap_size;
        if w == 0 || h == 0 {
            return Ok(());
        }
        let start = metrics::begin();
        let result = (|| {
            self.surface
                .resize(NonZeroU32::new(w).unwrap(), NonZeroU32::new(h).unwrap())
                .map_err(|e| anyhow!("GDI resize: {e}"))?;
            let mut buffer = self
                .surface
                .buffer_mut()
                .map_err(|e| anyhow!("GDI buffer: {e}"))?;
            buffer.copy_from_slice(&self.bitmap);
            self.window.pre_present_notify();
            buffer.present().map_err(|e| anyhow!("GDI present: {e}"))
        })();
        metrics::finish(metrics::Stage::Present, start, result.is_ok());
        result
    }
    fn draw(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        if !self.dirty && !self.hidden() {
            return self.present();
        } // OS expose: reuse the bitmap.
        let size = self.window.inner_size();
        let hidden = self.hidden() || size.width == 0 || size.height == 0;
        egui_winit::update_viewport_info(&mut self.info, &self.ctx, &self.window, false);
        let mut raw = self.input.take_egui_input(&self.window);
        raw.viewports
            .insert(egui::ViewportId::ROOT, self.info.clone());
        let requested_close = raw.viewport().close_requested();
        if hidden {
            // Logic/worker completion/close still work. No layout, tessellation or texture
            // consumption occurs here; egui retains texture updates until the next UI pass.
            let output = self.ctx.run_logic(&raw, |ctx| self.app.logic(ctx));
            self.input.handle_platform_output_with_event_loop(
                &self.window,
                event_loop,
                output.platform_output,
            );
            self.commands(
                output
                    .viewport_commands
                    .get(&egui::ViewportId::ROOT)
                    .cloned()
                    .unwrap_or_default(),
                requested_close,
                event_loop,
            );
            return Ok(());
        }
        ensure!(
            u64::from(size.width) * u64::from(size.height) <= 16_777_216,
            "UI bitmap exceeds the 16-megapixel allocation limit"
        );
        let _frame = metrics::frame();
        let layout = metrics::begin();
        let mut output = self.ctx.run_ui(raw, |ui| {
            self.app.logic(ui.ctx());
            self.app.ui(ui);
        });
        metrics::finish(metrics::Stage::Layout, layout, true);
        let paint = metrics::begin();
        let primitives = raster::meshes(
            self.ctx
                .tessellate(std::mem::take(&mut output.shapes), output.pixels_per_point),
        )?;
        let len = size.width as usize * size.height as usize;
        self.pixels.resize(len, [0; 4]);
        self.raster.render(
            &mut BufferMutRef::new(&mut self.pixels, size.width as usize, size.height as usize),
            &primitives,
            &raster::textures(&mut output.textures_delta),
            output.pixels_per_point,
        );
        self.bitmap.clear();
        self.bitmap
            .extend(self.pixels.iter().copied().map(gdi_pixel));
        self.bitmap_size = [size.width, size.height];
        metrics::finish(metrics::Stage::Paint, paint, true);
        self.present()?;
        self.dirty = false;
        self.input.handle_platform_output_with_event_loop(
            &self.window,
            event_loop,
            output.platform_output,
        );
        self.commands(
            output
                .viewport_output
                .remove(&egui::ViewportId::ROOT)
                .map(|v| v.commands)
                .unwrap_or_default(),
            requested_close,
            event_loop,
        );
        Ok(())
    }
}
struct Host {
    ctx: egui::Context,
    factory: Option<Factory>,
    title: String,
    active: bool,
    root: Option<Root>,
    next: Deadline,
    error: Option<anyhow::Error>,
}
impl Host {
    fn schedule(&mut self, when: Instant) {
        self.next.schedule(when);
    }
    fn create(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        let icon =
            image::load_from_memory(include_bytes!("../assets/ScreenRecorder.png"))?.into_rgba8();
        let icon =
            winit::window::Icon::from_rgba(icon.as_raw().clone(), icon.width(), icon.height())?;
        let window = Arc::new(
            event_loop.create_window(
                Window::default_attributes()
                    .with_title(&self.title)
                    .with_visible(false)
                    .with_active(self.active)
                    .with_window_icon(Some(icon))
                    .with_inner_size(winit::dpi::LogicalSize::new(1120.0, 800.0))
                    .with_min_inner_size(winit::dpi::LogicalSize::new(880.0, 650.0)),
            )?,
        );
        let input = egui_winit::State::new(
            self.ctx.clone(),
            egui::ViewportId::ROOT,
            window.as_ref(),
            Some(window.scale_factor() as f32),
            window.theme(),
            Some(16384),
        );
        let context =
            softbuffer::Context::new(window.clone()).map_err(|e| anyhow!("GDI context: {e}"))?;
        let surface = softbuffer::Surface::new(&context, window.clone())
            .map_err(|e| anyhow!("GDI surface: {e}"))?;
        let app = self
            .factory
            .take()
            .ok_or_else(|| anyhow!("Root window already initialized"))?(
            &self.ctx, window.clone()
        )?;
        self.root = Some(Root {
            window: window.clone(),
            surface,
            ctx: self.ctx.clone(),
            input,
            info: Default::default(),
            app,
            raster: EguiSoftwareRender::new(ColorFieldOrder::Rgba),
            pixels: Vec::new(),
            bitmap: Vec::new(),
            bitmap_size: [0, 0],
            dirty: true,
            occluded: false,
        });
        window.set_visible(true);
        self.schedule(Instant::now());
        Ok(())
    }
    fn fail(&mut self, error: anyhow::Error, event_loop: &ActiveEventLoop) {
        self.error = Some(error);
        event_loop.exit();
    }
}
impl ApplicationHandler<Wake> for Host {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.root.is_none()
            && let Err(error) = self.create(event_loop)
        {
            self.fail(error, event_loop);
        }
    }
    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: Wake) {
        self.schedule(event.0);
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(root) = &mut self.root else {
            return;
        };
        if id != root.window.id() {
            return;
        }
        if matches!(event, WindowEvent::RedrawRequested) {
            if let Err(error) = root.draw(event_loop) {
                self.fail(error, event_loop);
            }
            return;
        }
        if let WindowEvent::Occluded(value) = event {
            root.occluded = value;
        }
        if matches!(event, WindowEvent::CloseRequested) {
            root.info.events.push(egui::ViewportEvent::Close);
        }
        let response = root.input.on_window_event(&root.window, &event);
        if response.repaint
            || matches!(
                event,
                WindowEvent::CloseRequested
                    | WindowEvent::Resized(_)
                    | WindowEvent::ScaleFactorChanged { .. }
                    | WindowEvent::Occluded(false)
            )
        {
            root.dirty = true;
            self.schedule(Instant::now());
        }
    }
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.next.take_due(Instant::now())
            && let Some(root) = &mut self.root
        {
            root.dirty = true;
            root.window.request_redraw();
        }
        // Always reset Poll/expired deadlines. No periodic idle/hidden timer exists.
        event_loop.set_control_flow(
            self.next
                .0
                .map_or(ControlFlow::Wait, ControlFlow::WaitUntil),
        );
    }
    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(root) = &mut self.root {
            root.app.on_exit();
        }
        self.ctx.set_request_repaint_callback(|_| {});
    }
}
pub fn run(title: &str, active: bool, factory: Factory) -> Result<()> {
    let event_loop = EventLoop::<Wake>::with_user_event().build()?;
    let proxy = event_loop.create_proxy();
    let ctx = egui::Context::default();
    ctx.set_request_repaint_callback(move |info| {
        if info.delay != Duration::MAX
            && let Some(when) = Instant::now().checked_add(info.delay)
        {
            let _ = proxy.send_event(Wake(when));
        }
    });
    let mut host = Host {
        ctx,
        factory: Some(factory),
        title: title.into(),
        active,
        root: None,
        next: Deadline::default(),
        error: None,
    };
    event_loop.run_app(&mut host)?;
    if let Some(error) = host.error {
        return Err(error);
    }
    Ok(())
}

// Softbuffer's native 0x00RRGGBB format ignores alpha. Source pixels are RGBA.
fn gdi_pixel([r, g, b, _]: [u8; 4]) -> u32 {
    u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b)
}
#[derive(Default)]
struct Deadline(Option<Instant>);
impl Deadline {
    fn schedule(&mut self, when: Instant) {
        self.0 = Some(self.0.map_or(when, |old| old.min(when)));
    }
    fn take_due(&mut self, now: Instant) -> bool {
        if self.0.is_some_and(|when| when <= now) {
            self.0 = None;
            true
        } else {
            false
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deadlines_coalesce_and_expire_without_a_recurring_wakeup() {
        let now = Instant::now();
        let mut next = Deadline::default();
        next.schedule(now + Duration::from_secs(60));
        next.schedule(now);
        next.schedule(now + Duration::from_secs(1));
        assert!(next.take_due(now));
        assert!(next.0.is_none());
        assert!(!next.take_due(now + Duration::from_secs(100)));
        next.schedule(now + Duration::from_secs(2));
        assert!(!next.take_due(now + Duration::from_secs(1)));
        assert!(next.take_due(now + Duration::from_secs(2)));
        assert!(next.0.is_none());
    }
    #[test]
    fn native_bitmap_preserves_rgb_channels_and_ignores_alpha() {
        assert_eq!(gdi_pixel([255, 0, 0, 255]), 0x00ff0000);
        assert_eq!(gdi_pixel([0, 0, 255, 255]), 0x000000ff);
        assert_eq!(gdi_pixel([38, 57, 91, 0]), 0x0026395b);
    }
}
