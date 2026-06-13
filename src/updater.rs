//! Product settings for the shared verified Windows updater.
use anyhow::{Result, anyhow};
pub use rust_auto_updator::{Prepared, Release as Update};
use std::sync::atomic::AtomicBool;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const REPOSITORY: &str = "https://github.com/siarheisinelnikau/screen-recorder";

fn config_for(version: &str) -> Result<rust_auto_updator::Config> {
    let mut config = rust_auto_updator::Config::new(
        "siarheisinelnikau",
        "screen-recorder",
        version,
        "ScreenRecorder",
    )
    .map_err(|e| anyhow!(e))?;
    config.expected_subsystem = Some(2);
    config.install.startup_health_timeout = Some(std::time::Duration::from_secs(30));
    config.max_executable_size = 256 * 1024 * 1024;
    config.install.stage_prefix = ".screenrecorder-update-".into();
    config.install.helper_name = "ScreenRecorder.exe".into();
    Ok(config)
}

pub fn check() -> Result<Option<Update>> {
    rust_auto_updator::check_latest(&config_for(VERSION)?).map_err(|e| anyhow!(e))
}

pub fn download(update: &Update, cancel: &AtomicBool, progress: impl Fn(f32)) -> Result<Prepared> {
    rust_auto_updator::prepare_download_cancellable(
        update,
        &config_for(VERSION)?,
        cancel,
        |done, total| progress(done as f32 / total.max(1) as f32),
    )
    .map_err(|e| anyhow!(e))
}

pub fn launch_install(prepared: &mut Prepared) -> Result<()> {
    rust_auto_updator::handoff(prepared).map_err(|e| anyhow!(e))
}

/// Route the shared helper protocol before UI, settings and recording startup.
pub fn run_install_helper() -> bool {
    let args: Vec<_> = std::env::args_os().collect();
    if args.get(1).is_none_or(|a| a != "--apply-update") {
        return false;
    }
    if let Err(error) = config_for(VERSION)
        .and_then(|config| rust_auto_updator::apply_update(&config).map_err(|e| anyhow!(e)))
    {
        let directory = crate::settings::directory();
        let _ = std::fs::create_dir_all(&directory);
        let _ = std::fs::write(directory.join("update-error.txt"), format!("{error:#}"));
    }
    true
}

/// Remove the update backup only after the restarted host presents its first frame.
pub fn cleanup_after_update() {
    let args: Vec<_> = std::env::args_os().collect();
    if args.get(1).is_some_and(|a| a == "--cleanup-update")
        && let Some(directory) = args.get(2)
        && let Ok(config) = config_for(VERSION)
    {
        rust_auto_updator::cleanup_after_update(directory.into(), config.install);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn product_release_contract_is_repository_bound() -> Result<()> {
        let config = config_for("0.2.0")?;
        let fixture = serde_json::json!({
            "tag_name":"v0.3.0", "draft":false, "prerelease":false,
            "html_url":format!("{REPOSITORY}/releases/tag/v0.3.0"),
            "assets":[
                {"name":"ScreenRecorder-v0.3.0.exe", "size":4096,
                 "browser_download_url":format!("{REPOSITORY}/releases/download/v0.3.0/ScreenRecorder-v0.3.0.exe")},
                {"name":"SHA256SUMS.txt", "size":128,
                 "browser_download_url":format!("{REPOSITORY}/releases/download/v0.3.0/SHA256SUMS.txt")}
            ]
        });
        let release = rust_auto_updator::parse_release(&fixture.to_string(), &config)
            .map_err(|e| anyhow!(e))?
            .unwrap();
        assert_eq!(release.executable_name(), "ScreenRecorder-v0.3.0.exe");
        let mut foreign = fixture;
        foreign["assets"][0]["browser_download_url"] = "https://example.com/app.exe".into();
        assert!(rust_auto_updator::parse_release(&foreign.to_string(), &config).is_err());
        Ok(())
    }
}
