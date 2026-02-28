use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub startup_updates: bool,
    pub output_folder: String,
    pub fps: u32,
    pub microphone: bool,
    pub system_audio: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            startup_updates: true,
            output_folder: String::new(),
            fps: 15,
            microphone: false,
            system_audio: false,
        }
    }
}
pub fn directory() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("ScreenRecorder")
}
impl Settings {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        ensure!(
            fs::metadata(path)?.len() < 64 * 1024,
            "Settings file is too large"
        );
        let settings: Self = serde_json::from_slice(&fs::read(path)?)?;
        ensure!((5..=30).contains(&settings.fps), "Invalid saved frame rate");
        Ok(settings)
    }
    pub fn save(&self, path: &Path) -> Result<()> {
        fs::create_dir_all(path.parent().context("Invalid settings path")?)?;
        let temp = path.with_extension("json.tmp");
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .context("Cannot save settings; check permissions or a leftover .tmp file")?;
        let result = (|| -> Result<()> {
            serde_json::to_writer_pretty(&file, self)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temp, path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(temp);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_settings_do_not_enable_audio() -> Result<()> {
        let settings: Settings =
            serde_json::from_str(r#"{"startup_updates":false,"output_folder":"","fps":15}"#)?;
        assert!(!settings.microphone && !settings.system_audio);
        assert!(!settings.startup_updates);
        Ok(())
    }
    #[test]
    fn startup_opt_out_survives_restart() -> Result<()> {
        let dir = crate::test_directory()?;
        let path = dir.path().join("settings.json");
        assert!(Settings::load(&path)?.startup_updates);
        let s = Settings {
            startup_updates: false,
            ..Default::default()
        };
        s.save(&path)?;
        assert!(!Settings::load(&path)?.startup_updates);
        s.save(&path)?;
        assert!(!Settings::load(&path)?.startup_updates);
        Ok(())
    }
}
