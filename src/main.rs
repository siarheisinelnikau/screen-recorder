#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod app;
mod theme;

fn main() -> eframe::Result {
    // Parse before GUI construction, settings, updater helper or any capture startup.
    let args: Vec<_> = std::env::args_os().collect();
    if args.get(1).is_some_and(|arg| arg == "--render-review") {
        let result = if args.len() == 3 {
            app::render_review(std::path::Path::new(&args[2]))
        } else {
            Err(anyhow::anyhow!(
                "Usage: ScreenRecorder.exe --render-review ABSOLUTE_NEW_DIRECTORY"
            ))
        };
        if let Err(error) = result {
            eprintln!("Render review failed: {error:#}");
            std::process::exit(1);
        }
        return Ok(());
    }
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
