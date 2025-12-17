//! Append-only, recoverable recording storage. No desktop APIs are used here.
use anyhow::{Context, Result, bail, ensure};
use image::{ExtendedColorType, RgbImage, codecs::jpeg::JpegEncoder};
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

const MAGIC: &[u8; 8] = b"SREC0001";
const HEADER: u64 = 20;
const FRAME_HEADER: usize = 25;
pub const MAX_JPEG: usize = 32 * 1024 * 1024;
pub const MAX_FRAMES: usize = 200_000;
pub const MAX_BYTES: u64 = 16 * 1024 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Cursor {
    pub x: f32,
    pub y: f32,
    pub visible: bool,
    pub click: bool,
}

#[derive(Clone, Debug)]
pub struct FrameIndex {
    pub time_us: u64,
    pub cursor: Cursor,
    offset: u64,
    length: u32,
}

#[derive(Clone, Debug)]
pub struct Project {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub frames: Vec<FrameIndex>,
    pub recovered_tail: bool,
}

pub fn validate_dimensions(width: u32, height: u32, fps: u32) -> Result<()> {
    ensure!(
        (64..=4096).contains(&width) && (64..=2160).contains(&height),
        "Capture dimensions must be 64..4096 × 64..2160 pixels"
    );
    ensure!((5..=30).contains(&fps), "Frame rate must be 5..30");
    Ok(())
}

pub struct ProjectWriter {
    file: File,
    width: u32,
    height: u32,
    count: usize,
    last_time: Option<u64>,
    jpeg: Vec<u8>,
    last_good: u64,
}

impl ProjectWriter {
    pub fn create(path: &Path, width: u32, height: u32, fps: u32) -> Result<Self> {
        validate_dimensions(width, height, fps)?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .context("Cannot create recording (existing files are never overwritten)")?;
        file.write_all(MAGIC)?;
        for n in [width, height, fps] {
            file.write_all(&n.to_le_bytes())?;
        }
        file.sync_data()?;
        Ok(Self {
            file,
            width,
            height,
            count: 0,
            last_time: None,
            jpeg: Vec::new(),
            last_good: HEADER,
        })
    }

    pub fn append(&mut self, image: &RgbImage, time_us: u64, cursor: Cursor) -> Result<()> {
        ensure!(
            cursor.x.is_finite()
                && cursor.y.is_finite()
                && (0.0..=1.0).contains(&cursor.x)
                && (0.0..=1.0).contains(&cursor.y),
            "Cursor coordinates must be normalized"
        );
        ensure!(
            image.dimensions() == (self.width, self.height),
            "Capture size changed"
        );
        ensure!(
            self.last_time.is_none_or(|t| time_us > t),
            "Frame times must increase"
        );
        ensure!(
            self.count < MAX_FRAMES,
            "Recording frame limit reached; start a new recording"
        );
        self.jpeg.clear();
        JpegEncoder::new_with_quality(&mut self.jpeg, 85).encode(
            image,
            self.width,
            self.height,
            ExtendedColorType::Rgb8,
        )?;
        ensure!(
            self.jpeg.len() <= MAX_JPEG,
            "Encoded frame exceeds safety limit"
        );
        ensure!(
            self.last_good + FRAME_HEADER as u64 + self.jpeg.len() as u64 <= MAX_BYTES,
            "Recording reached the 16 GiB file limit"
        );
        let result = (|| -> std::io::Result<()> {
            self.file
                .write_all(&(self.jpeg.len() as u32).to_le_bytes())?;
            self.file.write_all(&time_us.to_le_bytes())?;
            self.file.write_all(&cursor.x.to_le_bytes())?;
            self.file.write_all(&cursor.y.to_le_bytes())?;
            self.file
                .write_all(&[u8::from(cursor.visible) | u8::from(cursor.click) << 1])?;
            self.file.write_all(&[0; 4])?;
            self.file.write_all(&self.jpeg)?;
            Ok(())
        })();
        if let Err(error) = result {
            // A partial tail is never interpreted as a complete frame, even if truncation fails.
            let _ = self.file.set_len(self.last_good);
            return Err(error)
                .context("Recording write failed; completed frames remain recoverable");
        }
        self.last_good = self.file.stream_position()?;
        self.last_time = Some(time_us);
        self.count += 1;
        if self.count.is_multiple_of(15) {
            self.file.sync_data()?;
        }
        Ok(())
    }

    pub fn finish(self) -> Result<()> {
        self.file
            .sync_all()
            .context("Could not flush recording to disk")
    }
}

impl Project {
    pub fn open(path: &Path) -> Result<Self> {
        let mut file = File::open(path).context("Cannot open recording")?;
        let length = file.metadata()?.len();
        ensure!(
            (HEADER..=MAX_BYTES).contains(&length),
            "Invalid recording size"
        );
        let mut header = [0u8; HEADER as usize];
        file.read_exact(&mut header)?;
        ensure!(
            &header[..8] == MAGIC,
            "Not a ScreenRecorder .srec version 1 project"
        );
        let n = |i| u32::from_le_bytes(header[i..i + 4].try_into().unwrap());
        let (width, height, fps) = (n(8), n(12), n(16));
        validate_dimensions(width, height, fps)?;
        let mut frames: Vec<FrameIndex> = Vec::new();
        let mut recovered_tail = false;
        let mut position = HEADER;
        while position < length {
            ensure!(frames.len() < MAX_FRAMES, "Too many frames in recording");
            if length - position < FRAME_HEADER as u64 {
                recovered_tail = true;
                break;
            }
            let mut h = [0u8; FRAME_HEADER];
            file.read_exact(&mut h)?;
            let size = u32::from_le_bytes(h[..4].try_into().unwrap());
            ensure!(
                size > 0 && size as usize <= MAX_JPEG,
                "Corrupt frame length"
            );
            let offset = position + FRAME_HEADER as u64;
            if offset + size as u64 > length {
                recovered_tail = true;
                break;
            }
            let time_us = u64::from_le_bytes(h[4..12].try_into().unwrap());
            ensure!(
                time_us <= 24 * 60 * 60 * 1_000_000u64,
                "Invalid frame timestamp"
            );
            if let Some(previous) = frames.last() {
                ensure!(time_us > previous.time_us, "Non-monotonic frame times");
            }
            let x = f32::from_le_bytes(h[12..16].try_into().unwrap());
            let y = f32::from_le_bytes(h[16..20].try_into().unwrap());
            ensure!(
                x.is_finite()
                    && y.is_finite()
                    && (0.0..=1.0).contains(&x)
                    && (0.0..=1.0).contains(&y),
                "Invalid cursor coordinates"
            );
            frames.push(FrameIndex {
                time_us,
                cursor: Cursor {
                    x,
                    y,
                    visible: h[20] & 1 != 0,
                    click: h[20] & 2 != 0,
                },
                offset,
                length: size,
            });
            position = offset + size as u64;
            file.seek(SeekFrom::Start(position))?;
        }
        if frames.is_empty() {
            bail!("Recording contains no complete frames");
        }
        Ok(Self {
            path: path.to_path_buf(),
            width,
            height,
            fps,
            frames,
            recovered_tail,
        })
    }

    pub fn duration(&self) -> f64 {
        self.frames
            .last()
            .map_or(0.0, |f| f.time_us as f64 / 1e6 + 1.0 / self.fps as f64)
    }

    pub fn index_at(&self, seconds: f64) -> usize {
        self.frames
            .partition_point(|f| f.time_us as f64 <= seconds.max(0.0) * 1e6)
            .saturating_sub(1)
    }

    pub fn read_frame(&self, index: usize) -> Result<RgbImage> {
        let entry = self.frames.get(index).context("Invalid frame index")?;
        let mut file = File::open(&self.path)?;
        file.seek(SeekFrom::Start(entry.offset))?;
        let mut bytes = vec![0; entry.length as usize];
        file.read_exact(&mut bytes)?;
        let mut reader =
            image::ImageReader::with_format(std::io::Cursor::new(bytes), image::ImageFormat::Jpeg);
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(self.width);
        limits.max_image_height = Some(self.height);
        limits.max_alloc = Some(128 * 1024 * 1024);
        reader.limits(limits);
        let image = reader.decode()?.to_rgb8();
        ensure!(
            image.dimensions() == (self.width, self.height),
            "Frame dimensions do not match project"
        );
        Ok(image)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recovers_complete_frames_after_interrupted_append() -> Result<()> {
        let dir = crate::test_directory()?;
        let path = dir.path().join("fixture.srec");
        let mut writer = ProjectWriter::create(&path, 64, 64, 15)?;
        writer.append(
            &RgbImage::from_pixel(64, 64, image::Rgb([10, 30, 50])),
            0,
            Cursor::default(),
        )?;
        writer.finish()?;
        OpenOptions::new()
            .append(true)
            .open(&path)?
            .write_all(&[22, 0, 0])?;
        let project = Project::open(&path)?;
        assert!(project.recovered_tail);
        assert_eq!(project.frames.len(), 1);
        assert_eq!(project.read_frame(0)?.dimensions(), (64, 64));
        assert!(ProjectWriter::create(&path, 64, 64, 15).is_err());
        Ok(())
    }
    #[test]
    fn rejects_malformed_headers_and_timestamps() -> Result<()> {
        let dir = crate::test_directory()?;
        let path = dir.path().join("fixture.srec");
        let mut w = ProjectWriter::create(&path, 64, 64, 10)?;
        let frame = RgbImage::new(64, 64);
        w.append(&frame, 1, Cursor::default())?;
        assert!(w.append(&frame, 1, Cursor::default()).is_err());
        assert!(
            w.append(&RgbImage::new(65, 64), 2, Cursor::default())
                .is_err()
        );
        assert!(validate_dimensions(u32::MAX, 64, 10).is_err());
        Ok(())
    }
}
