//! Public HTTPS update checks. No updater is invoked by tests or build scripts.
use anyhow::{Context, Result, bail, ensure};
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const REPOSITORY: &str = "https://github.com/siarheisinelnikau/screen-recorder";
const API: &str = "https://api.github.com/repos/siarheisinelnikau/screen-recorder/releases/latest";
const ASSET_PREFIX: &str =
    "https://github.com/siarheisinelnikau/screen-recorder/releases/download/";
const MAX_DOWNLOAD: u64 = 256 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
    pub size: u64,
}
#[derive(Debug, Deserialize)]
pub struct Release {
    pub tag_name: String,
    pub draft: bool,
    pub prerelease: bool,
    pub assets: Vec<Asset>,
}
#[derive(Clone, Debug)]
pub struct Update {
    pub version: Version,
    pub executable: Asset,
    pub checksums: Asset,
}

pub fn select(release: Release, installed: &str) -> Result<Option<Update>> {
    ensure!(
        !release.draft && !release.prerelease,
        "Latest release is not a stable published release"
    );
    let version = Version::parse(release.tag_name.trim_start_matches('v'))
        .context("Invalid release version")?;
    ensure!(
        version.pre.is_empty(),
        "Prerelease updates are not supported on the stable channel"
    );
    if version <= Version::parse(installed)? {
        return Ok(None);
    }
    ensure!(
        cfg!(target_arch = "x86_64") && cfg!(windows),
        "Automatic installation supports Windows x64 only"
    );
    let name = format!("ScreenRecorder-v{version}.exe");
    let executable = release
        .assets
        .iter()
        .find(|a| a.name == name)
        .cloned()
        .context("Release has no compatible standalone executable")?;
    let checksums = release
        .assets
        .iter()
        .find(|a| a.name == "SHA256SUMS.txt")
        .cloned()
        .context("Release has no SHA256SUMS.txt; installation is unavailable")?;
    for asset in [&executable, &checksums] {
        ensure!(
            asset.browser_download_url.starts_with(ASSET_PREFIX),
            "Asset is outside the canonical repository"
        );
        ensure!(
            asset.size > 0 && asset.size <= MAX_DOWNLOAD,
            "Invalid release asset size"
        );
    }
    ensure!(
        checksums.size <= 64 * 1024,
        "Checksum asset exceeds size limit"
    );
    Ok(Some(Update {
        version,
        executable,
        checksums,
    }))
}

fn agent(timeout: u64) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(timeout)))
        .build()
        .into()
}
pub fn check() -> Result<Option<Update>> {
    let mut response=agent(20).get(API).header("User-Agent","ScreenRecorder/0.1.0")
        .header("Accept","application/vnd.github+json").call()
        .context("Update check unavailable: network, rate limit, or repository/release not yet published")?;
    let release: Release = response
        .body_mut()
        .with_config()
        .limit(2 * 1024 * 1024)
        .read_json()?;
    select(release, VERSION)
}

pub fn checksum_for(text: &str, name: &str) -> Result<String> {
    for line in text.lines() {
        let parts: Vec<_> = line.split_whitespace().collect();
        if parts.len() == 2 && parts[1].trim_start_matches('*') == name {
            ensure!(
                parts[0].len() == 64 && parts[0].bytes().all(|c| c.is_ascii_hexdigit()),
                "Invalid SHA-256 digest"
            );
            return Ok(parts[0].to_ascii_lowercase());
        }
    }
    bail!("Executable is missing from SHA256SUMS.txt")
}

pub fn verify_executable(path: &Path, expected: &str) -> Result<()> {
    let mut file = fs::File::open(path)?;
    let length = file.metadata()?.len();
    ensure!(
        (70..=MAX_DOWNLOAD).contains(&length),
        "Invalid executable length"
    );
    let mut header = [0u8; 64];
    file.read_exact(&mut header)?;
    ensure!(
        &header[..2] == b"MZ",
        "Release asset has no DOS executable header"
    );
    let offset = u32::from_le_bytes(header[60..64].try_into().unwrap()) as u64;
    ensure!(
        offset >= 64 && offset + 6 <= length,
        "Invalid PE header offset"
    );
    file.seek(SeekFrom::Start(offset))?;
    let mut pe = [0u8; 6];
    file.read_exact(&mut pe)?;
    ensure!(
        &pe[..4] == b"PE\0\0" && u16::from_le_bytes(pe[4..6].try_into().unwrap()) == 0x8664,
        "Update is not a Windows x64 PE executable"
    );
    file.seek(SeekFrom::Start(0))?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    ensure!(
        format!("{:x}", hash.finalize()) == expected,
        "Update checksum mismatch"
    );
    Ok(())
}

pub fn download(update: &Update, cancel: &AtomicBool, progress: impl Fn(f32)) -> Result<PathBuf> {
    let current = std::env::current_exe()?;
    let parent = current
        .parent()
        .context("Cannot locate application folder")?;
    let unique = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let staging = parent.join(format!(
        ".screenrecorder-update-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir(&staging).context(
        "Application folder is not writable; move the portable app to a writable folder",
    )?;
    let result = (|| -> Result<PathBuf> {
        let mut response = agent(20)
            .get(&update.checksums.browser_download_url)
            .call()?;
        let text = response
            .body_mut()
            .with_config()
            .limit(64 * 1024)
            .read_to_string()?;
        let expected = checksum_for(&text, &update.executable.name)?;
        let payload = staging.join("download.exe");
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&payload)?;
        let mut response = agent(180)
            .get(&update.executable.browser_download_url)
            .call()?;
        let mut reader = response.body_mut().as_reader();
        let mut hash = Sha256::new();
        let mut count = 0u64;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            ensure!(!cancel.load(Ordering::Relaxed), "Update download cancelled");
            let n = reader.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            count += n as u64;
            ensure!(
                count <= update.executable.size && count <= MAX_DOWNLOAD,
                "Download exceeds advertised size"
            );
            file.write_all(&buffer[..n])?;
            hash.update(&buffer[..n]);
            progress(count as f32 / update.executable.size as f32);
        }
        ensure!(
            count == update.executable.size,
            "Incomplete update download"
        );
        ensure!(
            format!("{:x}", hash.finalize()) == expected,
            "Update checksum mismatch; current installation was preserved"
        );
        file.sync_all()?;
        drop(file);
        ensure!(!cancel.load(Ordering::Relaxed), "Update download cancelled");
        verify_executable(&payload, &expected)?;
        fs::copy(&current, staging.join("helper.exe"))?;
        fs::write(staging.join("sha256.txt"), expected)?;
        Ok(staging.clone())
    })();
    if result.is_err() {
        // Only files created above are removed; never recursively delete a computed path.
        for name in ["download.exe", "helper.exe", "sha256.txt"] {
            let _ = fs::remove_file(staging.join(name));
        }
        let _ = fs::remove_dir(&staging);
    }
    result
}

pub fn launch_install(staging: &Path) -> Result<()> {
    let current = std::env::current_exe()?;
    let mut command = std::process::Command::new(staging.join("helper.exe"));
    command
        .arg("--apply-update")
        .arg(&current)
        .arg(std::process::id().to_string());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
        .spawn()
        .context("Cannot launch update helper; current installation was preserved")?;
    Ok(())
}

fn apply_update(target: &Path, parent_pid: u32) -> Result<()> {
    let helper = std::env::current_exe()?;
    let staging = helper.parent().context("Missing update staging folder")?;
    ensure!(
        target.is_absolute()
            && target
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("exe")),
        "Invalid update destination"
    );
    ensure!(
        staging.parent() == target.parent()
            && staging
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with(".screenrecorder-update-")),
        "Invalid staging location"
    );
    let payload = staging.join("download.exe");
    let expected = fs::read_to_string(staging.join("sha256.txt"))?;
    verify_executable(&payload, &expected)
        .context("Update changed after download or is incompatible")?;
    let backup = staging.join("previous.exe");
    ensure!(!backup.exists(), "Update backup already exists");
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::{Foundation::CloseHandle, System::Threading::*};
        let parent = OpenProcess(PROCESS_SYNCHRONIZE, 0, parent_pid);
        if !parent.is_null() {
            let state = WaitForSingleObject(parent, 60_000);
            CloseHandle(parent);
            ensure!(
                state == 0,
                "Application did not exit within 60 seconds; installation was not changed"
            );
        }
    }
    #[cfg(not(windows))]
    let _ = parent_pid;
    let mut renamed = false;
    for _ in 0..120 {
        if fs::rename(target, &backup).is_ok() {
            renamed = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    ensure!(
        renamed,
        "Could not replace the running executable after 60 seconds; check permissions"
    );
    if let Err(error) = fs::rename(&payload, target) {
        let _ = fs::rename(&backup, target);
        return Err(error).context("Installation failed; previous executable was restored");
    }
    if let Err(error) = std::process::Command::new(target).spawn() {
        // Keep both binaries and the backup for a manual recovery if relaunch fails.
        return Err(error).context("Installed update could not restart; launch it manually");
    }
    Ok(())
}

pub fn run_install_helper() -> bool {
    let args: Vec<_> = std::env::args_os().collect();
    if args.get(1).is_none_or(|x| x != "--apply-update") {
        return false;
    }
    let result = if args.len() == 4 {
        args[3]
            .to_string_lossy()
            .parse::<u32>()
            .context("Invalid parent process")
            .and_then(|pid| apply_update(Path::new(&args[2]), pid))
    } else {
        Err(anyhow::anyhow!("Invalid update helper arguments"))
    };
    if let Err(error) = result {
        let directory = crate::settings::directory();
        let _ = fs::create_dir_all(&directory);
        let _ = fs::write(directory.join("update-error.txt"), format!("{error:#}"));
        if let Ok(exe) = std::env::current_exe()
            && let Some(parent) = exe.parent()
        {
            let _ = fs::write(parent.join("update-error.txt"), format!("{error:#}"));
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    fn release(tag: &str) -> Release {
        Release {
            tag_name: tag.into(),
            draft: false,
            prerelease: false,
            assets: vec![
                Asset {
                    name: format!("ScreenRecorder-{tag}.exe"),
                    browser_download_url: format!("{ASSET_PREFIX}{tag}/app.exe"),
                    size: 12,
                },
                Asset {
                    name: "SHA256SUMS.txt".into(),
                    browser_download_url: format!("{ASSET_PREFIX}{tag}/SHA256SUMS.txt"),
                    size: 200,
                },
            ],
        }
    }
    #[test]
    fn stable_numeric_selection_and_asset_validation() -> Result<()> {
        assert!(select(release("v0.1.0"), "0.1.0")?.is_none());
        #[cfg(all(windows, target_arch = "x86_64"))]
        {
            assert_eq!(
                select(release("v0.10.0"), "0.9.0")?.unwrap().version,
                Version::parse("0.10.0")?
            );
            let mut r = release("v0.2.0");
            r.assets[0].browser_download_url = "https://example.com/evil.exe".into();
            assert!(select(r, "0.1.0").is_err());
        }
        let mut r = release("v0.2.0");
        r.prerelease = true;
        assert!(select(r, "0.1.0").is_err());
        Ok(())
    }
    #[test]
    fn requires_exact_checksum_name() -> Result<()> {
        let hash = "ab".repeat(32);
        assert_eq!(
            checksum_for(
                &format!("{hash} *ScreenRecorder-v0.2.0.exe"),
                "ScreenRecorder-v0.2.0.exe"
            )?,
            hash
        );
        assert!(checksum_for("bad app.exe", "app.exe").is_err());
        assert!(checksum_for(&format!("{hash} other.exe"), "app.exe").is_err());
        Ok(())
    }
    #[test]
    fn verifies_payload_hash_and_architecture_without_execution() -> Result<()> {
        let dir = crate::test_directory()?;
        let path = dir.path().join("artificial-pe.bin");
        let mut bytes = vec![0u8; 128];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[60..64].copy_from_slice(&64u32.to_le_bytes());
        bytes[64..70].copy_from_slice(b"PE\0\0\x64\x86");
        fs::write(&path, &bytes)?;
        let digest = format!("{:x}", Sha256::digest(&bytes));
        verify_executable(&path, &digest)?;
        assert!(verify_executable(&path, &"00".repeat(32)).is_err());
        bytes[68] = 0x4c;
        bytes[69] = 0x01;
        fs::write(&path, &bytes)?;
        assert!(verify_executable(&path, &format!("{:x}", Sha256::digest(&bytes))).is_err());
        Ok(())
    }
}
