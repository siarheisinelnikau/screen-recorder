#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod app;

fn main() -> eframe::Result {
    if screen_recorder::updater::run_install_helper() {
        return Ok(());
    }
    eframe::run_native(
        "ScreenRecorder",
        eframe::NativeOptions {
            viewport: eframe::egui::ViewportBuilder::default()
                .with_inner_size([1120.0, 800.0])
                .with_min_inner_size([880.0, 650.0]),
            renderer: eframe::Renderer::Glow,
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(app::RecorderApp::new(cc)))),
    )
}
