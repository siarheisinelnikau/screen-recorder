//! Streaming AVI 1.0 Motion JPEG writer. All frames are independently decodable.
use anyhow::{Result, ensure};
use image::{ExtendedColorType, RgbImage, codecs::jpeg::JpegEncoder};
use std::io::{Seek, SeekFrom, Write};

pub struct AviWriter<W: Write + Seek> {
    out: W,
    width: u32,
    height: u32,
    movi_size_pos: u64,
    movi_base: u64,
    total_frames_pos: u64,
    stream_length_pos: u64,
    index: Vec<(u32, u32)>,
    jpeg: Vec<u8>,
}

fn u32w(w: &mut impl Write, n: u32) -> std::io::Result<()> {
    w.write_all(&n.to_le_bytes())
}
fn u16w(w: &mut impl Write, n: u16) -> std::io::Result<()> {
    w.write_all(&n.to_le_bytes())
}

impl<W: Write + Seek> AviWriter<W> {
    pub fn new(mut out: W, width: u32, height: u32, fps: u32) -> Result<Self> {
        crate::project::validate_dimensions(width, height, fps)?;
        out.write_all(b"RIFF")?;
        u32w(&mut out, 0)?;
        out.write_all(b"AVI ")?;
        out.write_all(b"LIST")?;
        u32w(&mut out, 192)?;
        out.write_all(b"hdrl")?;
        out.write_all(b"avih")?;
        u32w(&mut out, 56)?;
        u32w(&mut out, 1_000_000 / fps)?;
        u32w(&mut out, 0)?;
        u32w(&mut out, 0)?;
        u32w(&mut out, 0x10)?;
        let total_frames_pos = out.stream_position()?;
        u32w(&mut out, 0)?;
        u32w(&mut out, 0)?;
        u32w(&mut out, 1)?;
        u32w(&mut out, width * height * 3)?;
        u32w(&mut out, width)?;
        u32w(&mut out, height)?;
        out.write_all(&[0; 16])?;
        out.write_all(b"LIST")?;
        u32w(&mut out, 116)?;
        out.write_all(b"strl")?;
        out.write_all(b"strh")?;
        u32w(&mut out, 56)?;
        out.write_all(b"vidsMJPG")?;
        u32w(&mut out, 0)?;
        u16w(&mut out, 0)?;
        u16w(&mut out, 0)?;
        u32w(&mut out, 0)?;
        u32w(&mut out, 1)?;
        u32w(&mut out, fps)?;
        u32w(&mut out, 0)?;
        let stream_length_pos = out.stream_position()?;
        u32w(&mut out, 0)?;
        u32w(&mut out, width * height * 3)?;
        u32w(&mut out, u32::MAX)?;
        u32w(&mut out, 0)?;
        u16w(&mut out, 0)?;
        u16w(&mut out, 0)?;
        u16w(&mut out, width as u16)?;
        u16w(&mut out, height as u16)?;
        out.write_all(b"strf")?;
        u32w(&mut out, 40)?;
        u32w(&mut out, 40)?;
        u32w(&mut out, width)?;
        u32w(&mut out, height)?;
        u16w(&mut out, 1)?;
        u16w(&mut out, 24)?;
        out.write_all(b"MJPG")?;
        u32w(&mut out, width * height * 3)?;
        out.write_all(&[0; 16])?;
        out.write_all(b"LIST")?;
        let movi_size_pos = out.stream_position()?;
        u32w(&mut out, 0)?;
        let movi_base = out.stream_position()?;
        out.write_all(b"movi")?;
        Ok(Self {
            out,
            width,
            height,
            movi_size_pos,
            movi_base,
            total_frames_pos,
            stream_length_pos,
            index: Vec::new(),
            jpeg: Vec::new(),
        })
    }

    pub fn append(&mut self, image: &RgbImage) -> Result<()> {
        ensure!(
            image.dimensions() == (self.width, self.height),
            "Export frame size changed"
        );
        self.jpeg.clear();
        JpegEncoder::new_with_quality(&mut self.jpeg, 90).encode(
            image,
            self.width,
            self.height,
            ExtendedColorType::Rgb8,
        )?;
        let position = self.out.stream_position()?;
        ensure!(
            position + self.jpeg.len() as u64 + (self.index.len() as u64 + 1) * 16 + 16
                < 2_000_000_000,
            "AVI 1.0 export limit (2 GB) reached; trim a shorter segment"
        );
        self.out.write_all(b"00dc")?;
        u32w(&mut self.out, self.jpeg.len() as u32)?;
        self.out.write_all(&self.jpeg)?;
        if self.jpeg.len() % 2 == 1 {
            self.out.write_all(&[0])?;
        }
        self.index
            .push(((position - self.movi_base) as u32, self.jpeg.len() as u32));
        Ok(())
    }

    pub fn finish(mut self) -> Result<W> {
        let end_movi = self.out.stream_position()?;
        self.out.write_all(b"idx1")?;
        u32w(&mut self.out, self.index.len() as u32 * 16)?;
        for &(offset, length) in &self.index {
            self.out.write_all(b"00dc")?;
            u32w(&mut self.out, 0x10)?;
            u32w(&mut self.out, offset)?;
            u32w(&mut self.out, length)?;
        }
        let end = self.out.stream_position()?;
        for (position, value) in [
            (4, (end - 8) as u32),
            (self.movi_size_pos, (end_movi - self.movi_base) as u32),
            (self.total_frames_pos, self.index.len() as u32),
            (self.stream_length_pos, self.index.len() as u32),
        ] {
            self.out.seek(SeekFrom::Start(position))?;
            u32w(&mut self.out, value)?;
        }
        self.out.seek(SeekFrom::Start(end))?;
        self.out.flush()?;
        Ok(self.out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct FullDisk(std::io::Cursor<Vec<u8>>);
    impl Write for FullDisk {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if self.0.position() + bytes.len() as u64 > 230 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::StorageFull,
                    "Injected full disk",
                ));
            }
            self.0.write(bytes)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl Seek for FullDisk {
        fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
            self.0.seek(position)
        }
    }
    #[test]
    fn propagates_disk_full_without_claiming_finalization() -> Result<()> {
        let mut writer = AviWriter::new(FullDisk(std::io::Cursor::new(Vec::new())), 64, 64, 10)?;
        assert!(
            writer
                .append(&RgbImage::new(64, 64))
                .unwrap_err()
                .to_string()
                .contains("Injected full disk")
        );
        Ok(())
    }
    #[test]
    fn writes_seekable_avi_with_correct_index() -> Result<()> {
        let mut writer = AviWriter::new(std::io::Cursor::new(Vec::new()), 64, 64, 10)?;
        writer.append(&RgbImage::new(64, 64))?;
        writer.append(&RgbImage::new(64, 64))?;
        let bytes = writer.finish()?.into_inner();
        let n = |i| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap()) as usize;
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(n(4), bytes.len() - 8);
        assert_eq!(&bytes[212..216], b"LIST");
        assert_eq!(&bytes[220..224], b"movi");
        let idx = 220 + n(216);
        assert_eq!(&bytes[idx..idx + 4], b"idx1");
        assert_eq!(n(idx + 4), 32);
        let frame = 220 + n(idx + 16);
        assert_eq!(&bytes[frame..frame + 4], b"00dc");
        assert_eq!(
            image::load_from_memory(&bytes[frame + 8..frame + 8 + n(frame + 4)])?.width(),
            64
        );
        Ok(())
    }
}
