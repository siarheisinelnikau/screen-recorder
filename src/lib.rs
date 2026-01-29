pub mod audio;
pub mod avi;
pub mod capture;
pub mod editor;
pub mod mp4;
pub mod project;
pub mod recorder;
pub mod settings;
pub mod timeline;
pub mod updater;
#[cfg(windows)]
mod windows_support;

#[cfg(test)]
pub(crate) fn test_directory() -> std::io::Result<tempfile::TempDir> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".test-output");
    std::fs::create_dir_all(&root)?;
    tempfile::tempdir_in(root)
}
