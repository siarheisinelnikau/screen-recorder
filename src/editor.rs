use crate::{
    avi::AviWriter,
    project::{Cursor, Project},
};
use anyhow::{Context, Result, ensure};
use image::{
    Rgb, RgbImage,
    imageops::{self, FilterType},
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Zoom {
    pub start: f64,
    pub end: f64,
    pub x: f32,
    pub y: f32,
    pub scale: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Edit {
    pub trim_start: f64,
    pub trim_end: f64,
    pub highlight: bool,
    pub clicks: bool,
    pub show_cursor: bool,
    pub automatic_zoom: bool,
    pub zooms: Vec<Zoom>,
}

impl Edit {
    pub fn new(project: &Project) -> Self {
        Self {
            trim_start: 0.0,
            trim_end: project.duration(),
            highlight: true,
            clicks: true,
            show_cursor: true,
            automatic_zoom: true,
            zooms: Vec::new(),
        }
    }
    pub fn validate(&self, project: &Project) -> Result<()> {
        ensure!(
            self.trim_start.is_finite()
                && self.trim_end.is_finite()
                && self.trim_start >= 0.0
                && self.trim_end <= project.duration() + 0.001
                && self.trim_end > self.trim_start,
            "Trim range must be inside the recording and nonempty"
        );
        ensure!(
            self.zooms.len() <= 2000,
            "At most 2000 manual zoom segments are supported"
        );
        for zoom in &self.zooms {
            ensure!(
                zoom.start.is_finite()
                    && zoom.end.is_finite()
                    && zoom.x.is_finite()
                    && zoom.y.is_finite()
                    && zoom.scale.is_finite()
                    && zoom.start >= 0.0
                    && zoom.end > zoom.start
                    && zoom.end <= project.duration() + 0.001
                    && (0.0..=1.0).contains(&zoom.x)
                    && (0.0..=1.0).contains(&zoom.y)
                    && (1.0..=3.0).contains(&zoom.scale),
                "Invalid manual zoom segment"
            );
        }
        Ok(())
    }
    pub fn load(project: &Project) -> Result<Self> {
        let path = project.path.with_extension("edit.json");
        if !path.exists() {
            return Ok(Self::new(project));
        }
        ensure!(
            fs::metadata(&path)?.len() <= 1024 * 1024,
            "Edit file is too large"
        );
        let edit: Self = serde_json::from_slice(&fs::read(path)?)?;
        edit.validate(project)?;
        Ok(edit)
    }
    pub fn save(&self, project: &Project) -> Result<()> {
        self.validate(project)?;
        let path = project.path.with_extension("edit.json");
        // Keep the old file until a fully written replacement is ready.
        let temp = project.path.with_extension("edit.json.tmp");
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .context("Cannot create edit temporary file; a previous .tmp may need recovery")?;
        if let Err(error) = (|| -> Result<()> {
            serde_json::to_writer_pretty(&file, self)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temp, &path)?;
            Ok(())
        })() {
            let _ = fs::remove_file(&temp);
            return Err(error);
        }
        Ok(())
    }
}

pub fn automatic_zooms(project: &Project) -> Vec<Zoom> {
    let mut result: Vec<Zoom> = Vec::new();
    for frame in &project.frames {
        if !frame.cursor.visible || !frame.cursor.click {
            continue;
        }
        let time = frame.time_us as f64 / 1e6;
        // Nearby clicks extend the current view instead of introducing camera jumps.
        if let Some(previous) = result.last_mut().filter(|z| time < z.end) {
            previous.end = (time + 1.4).min(project.duration());
        } else {
            result.push(Zoom {
                start: (time - 0.3).max(0.0),
                end: (time + 1.4).min(project.duration()),
                x: frame.cursor.x.clamp(0.0, 1.0),
                y: frame.cursor.y.clamp(0.0, 1.0),
                scale: 1.8,
            });
        }
    }
    result
}

fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub fn camera(time: f64, manual: &[Zoom], automatic: &[Zoom]) -> (f32, f32, f32) {
    let Some(zoom) = manual
        .iter()
        .find(|z| time >= z.start && time < z.end)
        .or_else(|| {
            let i = automatic.partition_point(|z| z.end <= time);
            automatic.get(i).filter(|z| time >= z.start && time < z.end)
        })
    else {
        return (0.5, 0.5, 1.0);
    };
    let ramp = ((time - zoom.start) / 0.35).min((zoom.end - time) / 0.45) as f32;
    let amount = ease(ramp);
    let scale = 1.0 + (zoom.scale - 1.0) * amount;
    (
        0.5 + (zoom.x - 0.5) * amount,
        0.5 + (zoom.y - 0.5) * amount,
        scale,
    )
}

pub fn render(
    project: &Project,
    edit: &Edit,
    automatic: &[Zoom],
    frame: &RgbImage,
    time: f64,
    width: u32,
    height: u32,
) -> RgbImage {
    let (cx, cy, scale) = camera(
        time,
        &edit.zooms,
        if edit.automatic_zoom { automatic } else { &[] },
    );
    let crop_w = ((project.width as f32 / scale).round() as u32).clamp(1, project.width);
    let crop_h = ((project.height as f32 / scale).round() as u32).clamp(1, project.height);
    let left = (cx * project.width as f32 - crop_w as f32 / 2.0)
        .clamp(0.0, (project.width - crop_w) as f32) as u32;
    let top = (cy * project.height as f32 - crop_h as f32 / 2.0)
        .clamp(0.0, (project.height - crop_h) as f32) as u32;
    let mut output = imageops::resize(
        &imageops::crop_imm(frame, left, top, crop_w, crop_h).to_image(),
        width,
        height,
        FilterType::Triangle,
    );
    let cursor = project.frames[project.index_at(time)].cursor;
    let transform = |c: Cursor| {
        (
            (c.x * project.width as f32 - left as f32) * width as f32 / crop_w as f32,
            (c.y * project.height as f32 - top as f32) * height as f32 / crop_h as f32,
        )
    };
    let radius = (width as f32 / 75.0).max(7.0);
    if cursor.visible {
        let (x, y) = transform(cursor);
        if edit.highlight {
            circle(&mut output, x, y, radius, 0.0, [255, 206, 75], 0.35);
        }
        if edit.show_cursor {
            arrow(&mut output, x, y, (width as f32 / 700.0).max(0.7));
        }
    }
    if edit.clicks {
        let end = project
            .frames
            .partition_point(|f| f.time_us as f64 / 1e6 <= time);
        for f in project.frames[..end]
            .iter()
            .rev()
            .take_while(|f| time - f.time_us as f64 / 1e6 < 0.45)
        {
            if f.cursor.click && f.cursor.visible {
                let age = (time - f.time_us as f64 / 1e6) as f32 / 0.45;
                let (x, y) = transform(f.cursor);
                circle(
                    &mut output,
                    x,
                    y,
                    radius * (1.0 + age * 1.8),
                    2.5,
                    [255, 144, 90],
                    1.0 - age,
                );
            }
        }
    }
    output
}

fn circle(
    image: &mut RgbImage,
    x: f32,
    y: f32,
    radius: f32,
    ring: f32,
    color: [u8; 3],
    alpha: f32,
) {
    for py in
        ((y - radius).max(0.0) as u32)..=((y + radius).min((image.height() - 1) as f32) as u32)
    {
        for px in
            ((x - radius).max(0.0) as u32)..=((x + radius).min((image.width() - 1) as f32) as u32)
        {
            let distance = ((px as f32 - x).powi(2) + (py as f32 - y).powi(2)).sqrt();
            if distance <= radius && (ring == 0.0 || distance >= radius - ring) {
                let pixel = image.get_pixel_mut(px, py);
                for i in 0..3 {
                    pixel[i] = (pixel[i] as f32 * (1.0 - alpha) + color[i] as f32 * alpha) as u8;
                }
            }
        }
    }
}

fn arrow(image: &mut RgbImage, x: f32, y: f32, size: f32) {
    for dy in 0..(22.0 * size) as i32 {
        for dx in 0..=(dy as f32 * 0.55) as i32 {
            let (px, py) = (x as i32 + dx, y as i32 + dy);
            if px >= 0 && py >= 0 && px < image.width() as i32 && py < image.height() as i32 {
                let border =
                    dx == 0 || dx >= (dy as f32 * 0.55) as i32 - 1 || dy >= (21.0 * size) as i32;
                image.put_pixel(
                    px as u32,
                    py as u32,
                    Rgb(if border {
                        [25, 29, 36]
                    } else {
                        [250, 252, 255]
                    }),
                );
            }
        }
    }
}

pub fn export(
    project: &Project,
    edit: &Edit,
    output: &Path,
    cancel: &AtomicBool,
    progress: impl Fn(f32),
) -> Result<()> {
    edit.validate(project)?;
    ensure!(
        !output.exists(),
        "Export destination already exists; choose a new name"
    );
    let partial = output.with_extension("avi.partial");
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial)
        .context("Cannot create export temporary file")?;
    let result = (|| -> Result<()> {
        let mut writer = AviWriter::new(file, project.width, project.height, project.fps)?;
        let automatic = automatic_zooms(project);
        let count = ((edit.trim_end - edit.trim_start) * project.fps as f64).ceil() as usize;
        ensure!(
            count <= crate::project::MAX_FRAMES,
            "Export exceeds frame limit; trim a shorter segment"
        );
        let mut cached = None;
        let mut cached_index = usize::MAX;
        for i in 0..count {
            ensure!(!cancel.load(Ordering::Relaxed), "Export cancelled");
            let time = edit.trim_start + i as f64 / project.fps as f64;
            let index = project.index_at(time);
            if index != cached_index {
                cached = Some(project.read_frame(index)?);
                cached_index = index;
            }
            writer.append(&render(
                project,
                edit,
                &automatic,
                cached.as_ref().unwrap(),
                time,
                project.width,
                project.height,
            ))?;
            progress((i + 1) as f32 / count as f32);
        }
        writer.finish()?.sync_all()?;
        publish_new(&partial, output)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&partial);
    }
    result
}

fn publish_new(partial: &Path, output: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let source: Vec<u16> = partial.as_os_str().encode_wide().chain(Some(0)).collect();
        let target: Vec<u16> = output.as_os_str().encode_wide().chain(Some(0)).collect();
        // MoveFileW fails if a destination appeared while exporting. It also supports FAT/exFAT.
        if unsafe {
            windows_sys::Win32::Storage::FileSystem::MoveFileW(source.as_ptr(), target.as_ptr())
        } == 0
        {
            return Err(std::io::Error::last_os_error())
                .context("Cannot publish AVI; choose a new writable destination");
        }
    }
    #[cfg(not(windows))]
    {
        fs::hard_link(partial, output)?;
        fs::remove_file(partial)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn camera_is_continuous_and_manual_segments_override_auto() {
        let z = Zoom {
            start: 1.0,
            end: 3.0,
            x: 0.8,
            y: 0.2,
            scale: 2.0,
        };
        assert_eq!(camera(1.0, std::slice::from_ref(&z), &[]), (0.5, 0.5, 1.0));
        assert_eq!(
            camera(2.0, &[], std::slice::from_ref(&z)),
            (0.8, 0.19999999, 2.0)
        );
        assert_eq!(camera(3.0, &[z], &[]), (0.5, 0.5, 1.0));
    }
    #[test]
    fn export_trim_cancel_and_pixel_effects() -> Result<()> {
        let dir = crate::test_directory()?;
        let path = dir.path().join("test.srec");
        let mut w = crate::project::ProjectWriter::create(&path, 64, 64, 10)?;
        for i in 0..10 {
            w.append(
                &RgbImage::new(64, 64),
                i * 100_000,
                Cursor {
                    x: 0.5,
                    y: 0.5,
                    visible: true,
                    click: i == 3,
                },
            )?;
        }
        w.finish()?;
        let p = Project::open(&path)?;
        let original = fs::read(&path)?;
        let mut e = Edit::new(&p);
        e.trim_start = 0.2;
        e.trim_end = 0.6;
        let output = dir.path().join("test.avi");
        let cancel = AtomicBool::new(true);
        assert!(export(&p, &e, &output, &cancel, |_| {}).is_err());
        assert!(!output.exists());
        cancel.store(false, Ordering::Relaxed);
        export(&p, &e, &output, &cancel, |_| {})?;
        assert!(fs::metadata(&output)?.len() > 1000);
        let exported = fs::read(&output)?;
        assert!(export(&p, &e, &output, &cancel, |_| {}).is_err());
        assert_eq!(exported, fs::read(&output)?);
        assert_eq!(original, fs::read(&path)?);
        let rendered = render(&p, &e, &[], &RgbImage::new(64, 64), 0.3, 64, 64);
        assert_ne!(rendered.get_pixel(32, 32), &Rgb([0, 0, 0]));
        e.save(&p)?;
        e.save(&p)?;
        assert_eq!(Edit::load(&p)?.trim_start, 0.2);
        e.trim_end = 2.0;
        assert!(e.validate(&p).is_err());
        Ok(())
    }
}
