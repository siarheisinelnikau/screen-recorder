//! Match the native caption when the application's palette changes.
use crate::theme;

#[cfg(windows)]
fn colorref(color: egui::Color32) -> u32 {
    u32::from(color.r()) | (u32::from(color.g()) << 8) | (u32::from(color.b()) << 16)
}

pub fn apply(window: &winit::window::Window, dark: bool) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Graphics::Dwm::{
            DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE,
            DwmSetWindowAttribute,
        };
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let Ok(handle) = window.window_handle() else {
            return;
        };
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return;
        };
        let hwnd = handle.hwnd.get() as *mut _;
        let palette = theme::for_dark(dark);
        for (attribute, value) in [
            (DWMWA_CAPTION_COLOR, colorref(palette.background)),
            (DWMWA_TEXT_COLOR, colorref(palette.text)),
            (DWMWA_USE_IMMERSIVE_DARK_MODE, u32::from(dark)),
        ] {
            // Windows 10 may not support caption colors. The explicit window
            // theme remains its fallback; never change the user's system theme.
            unsafe {
                DwmSetWindowAttribute(hwnd, attribute as u32, (&value as *const u32).cast(), 4);
            }
        }
    }
}

pub fn inspect(owner: isize) -> serde_json::Value {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Graphics::Dwm::{
            DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE,
            DwmGetWindowAttribute,
        };
        let read = |attribute| {
            let mut value = 0u32;
            let result = unsafe {
                DwmGetWindowAttribute(
                    owner as *mut _,
                    attribute as u32,
                    (&mut value as *mut u32).cast(),
                    4,
                )
            };
            (result >= 0).then_some(value)
        };
        let dark = read(DWMWA_USE_IMMERSIVE_DARK_MODE);
        let caption = read(DWMWA_CAPTION_COLOR);
        let text = read(DWMWA_TEXT_COLOR);
        serde_json::json!({"immersive_dark_mode":dark,"caption_colorref":caption,"text_colorref":text,
            "caption_matches_palette":caption.map(|v|v==colorref(theme::for_dark(dark==Some(1)).background)),
            "text_matches_palette":text.map(|v|v==colorref(theme::for_dark(dark==Some(1)).text)),
            "scope":"DWM attributes of this owned native window; no pixel readback"})
    }
    #[cfg(not(windows))]
    serde_json::json!({"unsupported":true,"owner":owner})
}
