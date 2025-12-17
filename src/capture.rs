//! Desktop access is confined to construction and sampling of a user-started Source.
use crate::project::Cursor;
use anyhow::Result;
use image::RgbImage;

#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    Display {
        left: i32,
        top: i32,
        width: u32,
        height: u32,
        name: String,
    },
    Window {
        handle: isize,
        name: String,
    },
    Region {
        left: i32,
        top: i32,
        width: u32,
        height: u32,
    },
}
impl Target {
    pub fn label(&self) -> String {
        match self {
            Self::Display { name, .. } | Self::Window { name, .. } => name.clone(),
            Self::Region {
                left,
                top,
                width,
                height,
            } => format!("Region {left}, {top} · {width} × {height}"),
        }
    }
}

pub trait FrameSource {
    fn dimensions(&self) -> (u32, u32);
    fn sample(&mut self) -> Result<(RgbImage, Cursor)>;
    fn stop_shortcut(&self) -> bool {
        false
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use anyhow::{Context, bail, ensure};
    use std::{
        mem::{size_of, zeroed},
        ptr::{null, null_mut},
    };
    use windows_sys::Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    };

    unsafe extern "system" fn monitor_callback(
        _: HMONITOR,
        _: HDC,
        rect: *mut RECT,
        data: LPARAM,
    ) -> i32 {
        let targets = unsafe { &mut *(data as *mut Vec<Target>) };
        let r = unsafe { *rect };
        targets.push(Target::Display {
            left: r.left,
            top: r.top,
            width: (r.right - r.left) as u32,
            height: (r.bottom - r.top) as u32,
            name: format!(
                "Display {} · {} × {} at {}, {}",
                targets.len() + 1,
                r.right - r.left,
                r.bottom - r.top,
                r.left,
                r.top
            ),
        });
        1
    }
    unsafe extern "system" fn window_callback(hwnd: HWND, data: LPARAM) -> i32 {
        unsafe {
            if IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 {
                return 1;
            }
            let mut pid = 0;
            GetWindowThreadProcessId(hwnd, &mut pid);
            if pid == std::process::id() {
                return 1;
            }
            let mut text = [0u16; 512];
            let count = GetWindowTextW(hwnd, text.as_mut_ptr(), text.len() as i32);
            if count > 0 {
                let targets = &mut *(data as *mut Vec<Target>);
                targets.push(Target::Window {
                    handle: hwnd as isize,
                    name: String::from_utf16_lossy(&text[..count as usize]),
                });
            }
            1
        }
    }
    pub fn targets() -> Result<Vec<Target>> {
        let mut targets = Vec::<Target>::new();
        unsafe {
            ensure!(
                EnumDisplayMonitors(
                    null_mut(),
                    null(),
                    Some(monitor_callback),
                    &mut targets as *mut _ as LPARAM
                ) != 0,
                "Display enumeration failed"
            );
            ensure!(
                EnumWindows(Some(window_callback), &mut targets as *mut _ as LPARAM) != 0,
                "Window enumeration failed"
            );
        }
        Ok(targets)
    }

    fn bounds(target: &Target) -> Result<(i32, i32, u32, u32)> {
        match target {
            Target::Display {
                left,
                top,
                width,
                height,
                ..
            }
            | Target::Region {
                left,
                top,
                width,
                height,
            } => Ok((*left, *top, *width, *height)),
            Target::Window { handle, .. } => unsafe {
                let hwnd = *handle as HWND;
                ensure!(
                    IsWindow(hwnd) != 0 && IsWindowVisible(hwnd) != 0 && IsIconic(hwnd) == 0,
                    "Selected window is closed, hidden or minimized"
                );
                let mut rect: RECT = zeroed();
                ensure!(
                    GetWindowRect(hwnd, &mut rect) != 0,
                    "Cannot locate selected window"
                );
                ensure!(
                    rect.right > rect.left && rect.bottom > rect.top,
                    "Window has no capture area"
                );
                Ok((
                    rect.left,
                    rect.top,
                    (rect.right - rect.left) as u32,
                    (rect.bottom - rect.top) as u32,
                ))
            },
        }
    }

    pub struct Source {
        target: Target,
        width: u32,
        height: u32,
        screen: HDC,
        memory: HDC,
        bitmap: HBITMAP,
        old: HGDIOBJ,
        bits: *mut u8,
        left_down: bool,
    }
    impl Source {
        pub fn new(target: Target, fps: u32) -> Result<Self> {
            let (_, _, width, height) = bounds(&target)?;
            crate::project::validate_dimensions(width, height, fps)?;
            unsafe {
                let mut source = Self {
                    target,
                    width,
                    height,
                    screen: null_mut(),
                    memory: null_mut(),
                    bitmap: null_mut(),
                    old: null_mut(),
                    bits: null_mut(),
                    left_down: false,
                };
                source.screen = GetDC(null_mut());
                ensure!(!source.screen.is_null(), "GetDC failed");
                source.memory = CreateCompatibleDC(source.screen);
                ensure!(!source.memory.is_null(), "CreateCompatibleDC failed");
                let mut info: BITMAPINFO = zeroed();
                info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
                info.bmiHeader.biWidth = width as i32;
                info.bmiHeader.biHeight = -(height as i32);
                info.bmiHeader.biPlanes = 1;
                info.bmiHeader.biBitCount = 32;
                info.bmiHeader.biCompression = BI_RGB;
                source.bitmap = CreateDIBSection(
                    source.screen,
                    &info,
                    DIB_RGB_COLORS,
                    &mut source.bits as *mut _ as *mut _,
                    null_mut(),
                    0,
                );
                ensure!(
                    !source.bitmap.is_null() && !source.bits.is_null(),
                    "Cannot allocate capture bitmap"
                );
                source.old = SelectObject(source.memory, source.bitmap);
                ensure!(
                    !source.old.is_null() && source.old as isize != -1,
                    "SelectObject failed"
                );
                Ok(source)
            }
        }
    }
    impl FrameSource for Source {
        fn dimensions(&self) -> (u32, u32) {
            (self.width, self.height)
        }
        fn sample(&mut self) -> Result<(RgbImage, Cursor)> {
            let (left, top, width, height) = bounds(&self.target)?;
            ensure!(
                (width, height) == (self.width, self.height),
                "Window was resized; recording stopped to preserve frame dimensions"
            );
            unsafe {
                let vx = GetSystemMetrics(SM_XVIRTUALSCREEN);
                let vy = GetSystemMetrics(SM_YVIRTUALSCREEN);
                let vr = vx as i64 + GetSystemMetrics(SM_CXVIRTUALSCREEN) as i64;
                let vb = vy as i64 + GetSystemMetrics(SM_CYVIRTUALSCREEN) as i64;
                ensure!(
                    left >= vx
                        && top >= vy
                        && left as i64 + width as i64 <= vr
                        && top as i64 + height as i64 <= vb,
                    "Capture area must remain within the virtual desktop"
                );
                if BitBlt(
                    self.memory,
                    0,
                    0,
                    width as i32,
                    height as i32,
                    self.screen,
                    left,
                    top,
                    SRCCOPY | CAPTUREBLT,
                ) == 0
                {
                    bail!(
                        "Windows capture failed (secure or protected content cannot be captured)"
                    );
                }
                GdiFlush();
                let bytes =
                    std::slice::from_raw_parts(self.bits, width as usize * height as usize * 4);
                let mut rgb = Vec::with_capacity(width as usize * height as usize * 3);
                for p in bytes.chunks_exact(4) {
                    rgb.extend_from_slice(&[p[2], p[1], p[0]]);
                }
                let mut cursor: CURSORINFO = zeroed();
                cursor.cbSize = size_of::<CURSORINFO>() as u32;
                let has_cursor = GetCursorInfo(&mut cursor) != 0 && cursor.flags == CURSOR_SHOWING;
                let x = (cursor.ptScreenPos.x as i64 - left as i64) as f32 / width as f32;
                let y = (cursor.ptScreenPos.y as i64 - top as i64) as f32 / height as f32;
                let down = GetAsyncKeyState(VK_LBUTTON as i32) < 0;
                let click = down && !self.left_down;
                self.left_down = down;
                Ok((
                    RgbImage::from_raw(width, height, rgb).context("Invalid capture buffer")?,
                    Cursor {
                        x: x.clamp(0.0, 1.0),
                        y: y.clamp(0.0, 1.0),
                        visible: has_cursor && (0.0..1.0).contains(&x) && (0.0..1.0).contains(&y),
                        click,
                    },
                ))
            }
        }
        fn stop_shortcut(&self) -> bool {
            unsafe {
                GetAsyncKeyState(VK_CONTROL as i32) < 0
                    && GetAsyncKeyState(VK_SHIFT as i32) < 0
                    && GetAsyncKeyState(VK_F10 as i32) < 0
            }
        }
    }
    impl Drop for Source {
        fn drop(&mut self) {
            unsafe {
                if !self.old.is_null() && self.old as isize != -1 {
                    SelectObject(self.memory, self.old);
                }
                if !self.bitmap.is_null() {
                    DeleteObject(self.bitmap);
                }
                if !self.memory.is_null() {
                    DeleteDC(self.memory);
                }
                if !self.screen.is_null() {
                    ReleaseDC(null_mut(), self.screen);
                }
            }
        }
    }
}

#[cfg(windows)]
pub use platform::{Source, targets};

#[cfg(not(windows))]
pub fn targets() -> Result<Vec<Target>> {
    anyhow::bail!("Desktop capture is supported on Windows only")
}
