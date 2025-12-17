pub mod avi;
pub mod capture;
pub mod editor;
pub mod project;
pub mod recorder;
pub mod settings;
pub mod updater;

#[cfg(test)]
pub(crate) fn test_directory() -> std::io::Result<tempfile::TempDir> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".test-output");
    std::fs::create_dir_all(&root)?;
    tempfile::tempdir_in(root)
}
