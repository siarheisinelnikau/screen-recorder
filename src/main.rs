#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod app;
mod chrome;
mod dialogs;
mod host;
mod raster;
mod render_diagnostics;
mod theme;
mod ui;

fn main() -> anyhow::Result<()> {
    // Parse before GUI construction, settings, updater helper or any capture startup.
    let args: Vec<_> = std::env::args_os().collect();
    if args.get(1).is_some_and(|arg| arg == "--diagnose-render") {
        let result = if args.len() == 3 {
            app::diagnose_render(std::path::Path::new(&args[2]))
        } else {
            Err(anyhow::anyhow!(
                "Usage: ScreenRecorder.exe --diagnose-render ABSOLUTE_NEW_DIRECTORY"
            ))
        };
        if let Err(error) = result {
            eprintln!("Render diagnostic failed: {error:#}");
            std::process::exit(1);
        }
        return Ok(());
    }
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
    host::run(
        "ScreenRecorder",
        true,
        Box::new(|ctx, window| {
            use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
            let owner = match window.window_handle()?.as_raw() {
                RawWindowHandle::Win32(h) => h.hwnd.get(),
                _ => 0,
            };
            Ok(Box::new(app::RecorderApp::new(ctx, owner)))
        }),
    )
}
