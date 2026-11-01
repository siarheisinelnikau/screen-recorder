//! Native pickers are opened only by explicit, non-isolated UI actions.
use anyhow::Result;
use screen_recorder::localization::Language;
pub enum Kind {
    Project,
    Folder,
}
#[cfg(windows)]
pub fn choose(owner: isize, kind: Kind, language: Language) -> Result<Option<String>> {
    use windows::{
        Win32::{
            Foundation::HWND,
            System::Com::{
                CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
                CoTaskMemFree, CoUninitialize,
            },
            UI::Shell::{
                Common::COMDLG_FILTERSPEC, FOS_DONTADDTORECENT, FOS_FILEMUSTEXIST,
                FOS_FORCEFILESYSTEM, FOS_NOCHANGEDIR, FOS_PICKFOLDERS, FileOpenDialog,
                IFileOpenDialog, SIGDN_FILESYSPATH,
            },
        },
        core::{HRESULT, HSTRING, PCWSTR, w},
    };
    struct Com;
    impl Drop for Com {
        fn drop(&mut self) {
            unsafe {
                CoUninitialize();
            }
        }
    }
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        let _com = Com;
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
        let mut flags =
            dialog.GetOptions()? | FOS_FORCEFILESYSTEM | FOS_NOCHANGEDIR | FOS_DONTADDTORECENT;
        match kind {
            Kind::Folder => {
                flags |= FOS_PICKFOLDERS;
                dialog.SetTitle(&HSTRING::from(language.text("Choose a recording folder")))?;
            }
            Kind::Project => {
                flags |= FOS_FILEMUSTEXIST;
                dialog.SetTitle(&HSTRING::from(language.text("Open a recording")))?;
                let name = HSTRING::from(language.text("ScreenRecorder recording"));
                dialog.SetFileTypes(&[COMDLG_FILTERSPEC {
                    pszName: PCWSTR(name.as_ptr()),
                    pszSpec: w!("*.srec"),
                }])?;
            }
        }
        dialog.SetOptions(flags)?;
        if let Err(error) = dialog.Show(Some(HWND(owner as *mut _))) {
            if error.code() == HRESULT(0x800704c7u32 as i32) {
                return Ok(None);
            }
            return Err(error.into());
        }
        let item = dialog.GetResult()?;
        let path = item.GetDisplayName(SIGDN_FILESYSPATH)?;
        let value = path.to_string();
        CoTaskMemFree(Some(path.0.cast()));
        Ok(Some(value?))
    }
}
#[cfg(not(windows))]
pub fn choose(_owner: isize, _kind: Kind, _language: Language) -> Result<Option<String>> {
    anyhow::bail!("Native pickers require Windows")
}
