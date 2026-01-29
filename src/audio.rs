//! Timestamped stereo float PCM journals and bounded mixing; device code is separate.
use crate::timeline::{Span, TICKS_PER_SECOND};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};
pub const RATE: u64 = 48_000;
const MAGIC: &[u8; 8] = b"SRAPCM01";
const MAX_PACKETS: usize = 1_000_000;
const MAX_BYTES: u64 = 8 * 1024 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Options {
    pub microphone: bool,
    pub system: bool,
}
impl Options {
    pub fn enabled(self) -> bool {
        self.microphone || self.system
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Microphone,
    System,
}
impl Kind {
    pub fn path(self, project: &Path) -> PathBuf {
        let mut name = project.as_os_str().to_os_string();
        name.push(match self {
            Self::Microphone => ".mic.saudio",
            Self::System => ".system.saudio",
        });
        PathBuf::from(name)
    }
}
#[derive(Clone, Copy, Debug)]
pub enum Encoding {
    Float32,
    Pcm16,
    Pcm24,
    Pcm32,
}
#[derive(Clone, Copy, Debug)]
pub struct Format {
    pub rate: u32,
    pub channels: u16,
    pub encoding: Encoding,
    pub mask: u32,
}
impl Format {
    pub fn validate(self) -> Result<()> {
        ensure!(
            (8_000..=192_000).contains(&self.rate) && (1..=8).contains(&self.channels),
            "Unsupported WASAPI sample rate/channel count"
        );
        ensure!(
            self.channels <= 2
                || (self.mask.count_ones() == self.channels as u32 && self.mask & !0x7ff == 0),
            "Unsupported multichannel speaker layout"
        );
        Ok(())
    }
    pub fn bytes_per_frame(self) -> usize {
        self.channels as usize
            * match self.encoding {
                Encoding::Pcm16 => 2,
                Encoding::Pcm24 => 3,
                _ => 4,
            }
    }
    fn channel(self, bytes: &[u8], frame: usize, channel: usize) -> f32 {
        let size = self.bytes_per_frame() / self.channels as usize;
        let i = frame * self.bytes_per_frame() + channel * size;
        let p = &bytes[i..i + size];
        let value = match self.encoding {
            Encoding::Float32 => f32::from_le_bytes(p.try_into().unwrap()),
            Encoding::Pcm16 => i16::from_le_bytes(p.try_into().unwrap()) as f32 / 32768.0,
            Encoding::Pcm24 => {
                ((i32::from_le_bytes([0, p[0], p[1], p[2]])) >> 8) as f32 / 8_388_608.0
            }
            Encoding::Pcm32 => i32::from_le_bytes(p.try_into().unwrap()) as f32 / 2_147_483_648.0,
        };
        if value.is_finite() {
            value.clamp(-1.0, 1.0)
        } else {
            0.0
        }
    }
    fn stereo(self, bytes: &[u8], frame: usize) -> [f32; 2] {
        if self.channels == 1 {
            let v = self.channel(bytes, frame, 0);
            return [v, v];
        }
        if self.channels == 2 {
            return [self.channel(bytes, frame, 0), self.channel(bytes, frame, 1)];
        }
        let mut result = [0.0; 2];
        let mut channel = 0;
        for bit in 0..11 {
            if self.mask & (1 << bit) == 0 {
                continue;
            }
            let gains = match bit {
                0 => [1.0, 0.0],
                1 => [0.0, 1.0],
                2 => [0.707, 0.707],
                3 => [0.25, 0.25],
                4 | 6 | 9 => [0.5, 0.0],
                5 | 7 | 10 => [0.0, 0.5],
                _ => [0.35, 0.35],
            };
            let value = self.channel(bytes, frame, channel);
            channel += 1;
            for i in 0..2 {
                result[i] += value * gains[i];
            }
        }
        result
    }
}
pub struct Packet {
    pub start: u64,
    pub samples: Vec<[f32; 2]>,
}
/// Resample against absolute clock time, preserving phase across packet/pause boundaries.
pub fn convert(
    format: Format,
    bytes: &[u8],
    frames: u32,
    silent: bool,
    packet_time: u64,
    spans: &[Span],
) -> Result<Vec<Packet>> {
    format.validate()?;
    ensure!(frames <= format.rate, "Audio packet exceeds one second");
    ensure!(
        silent || bytes.len() == frames as usize * format.bytes_per_frame(),
        "Invalid PCM packet length"
    );
    let mut packets = Vec::new();
    for span in spans {
        ensure!(
            span.end >= span.begin
                && span.begin >= packet_time
                && span.active_begin <= 86400 * TICKS_PER_SECOND
                && span.end - span.begin <= TICKS_PER_SECOND,
            "Invalid mapped audio interval"
        );
        let start =
            (span.active_begin as u128 * RATE as u128).div_ceil(TICKS_PER_SECOND as u128) as u64;
        let end = ((span.active_begin + span.end - span.begin) as u128 * RATE as u128)
            .div_ceil(TICKS_PER_SECOND as u128) as u64;
        ensure!(
            end >= start && end - start <= RATE,
            "Invalid mapped audio span"
        );
        let mut samples = Vec::with_capacity((end - start) as usize);
        for output in start..end {
            if silent || frames == 0 {
                samples.push([0.0; 2]);
                continue;
            }
            let absolute = span.begin as f64
                + output as f64 * TICKS_PER_SECOND as f64 / RATE as f64
                - span.active_begin as f64;
            let source = ((absolute - packet_time as f64) * format.rate as f64
                / TICKS_PER_SECOND as f64)
                .clamp(0.0, (frames - 1) as f64);
            let index = source.floor() as usize;
            let fraction = source.fract() as f32;
            let a = format.stereo(bytes, index);
            let b = format.stereo(bytes, (index + 1).min(frames as usize - 1));
            samples.push([
                a[0] + (b[0] - a[0]) * fraction,
                a[1] + (b[1] - a[1]) * fraction,
            ]);
        }
        if !samples.is_empty() {
            packets.push(Packet { start, samples });
        }
    }
    Ok(packets)
}

pub struct TrackWriter {
    file: File,
    last_end: u64,
    last_good: u64,
    packets: usize,
}
impl TrackWriter {
    pub fn create(path: &Path) -> Result<Self> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .context("Cannot create new audio sidecar")?;
        file.write_all(MAGIC)?;
        file.write_all(&(RATE as u32).to_le_bytes())?;
        file.write_all(&2u32.to_le_bytes())?;
        file.sync_data()?;
        Ok(Self {
            file,
            last_end: 0,
            last_good: 16,
            packets: 0,
        })
    }
    pub fn append(&mut self, packet: &Packet) -> Result<()> {
        ensure!(
            !packet.samples.is_empty() && packet.samples.len() <= RATE as usize,
            "Invalid PCM packet size"
        );
        ensure!(
            packet.start <= RATE * 86400
                && packet.samples.len() as u64 <= RATE * 86400 - packet.start,
            "Audio exceeds 24-hour timeline limit"
        );
        let skip = self
            .last_end
            .saturating_sub(packet.start)
            .min(packet.samples.len() as u64) as usize;
        if skip == packet.samples.len() {
            return Ok(());
        }
        let start = packet.start + skip as u64;
        let samples = &packet.samples[skip..];
        ensure!(
            start + samples.len() as u64 <= RATE * 86400,
            "Audio exceeds 24-hour timeline limit"
        );
        ensure!(
            self.packets < MAX_PACKETS
                && self.last_good + 12 + samples.len() as u64 * 8 <= MAX_BYTES,
            "Audio journal size/packet limit reached"
        );
        let mut bytes = Vec::with_capacity(12 + samples.len() * 8);
        bytes.extend_from_slice(&start.to_le_bytes());
        bytes.extend_from_slice(&(samples.len() as u32).to_le_bytes());
        for sample in samples {
            for channel in sample {
                ensure!(channel.is_finite(), "Nonfinite PCM sample");
                bytes.extend_from_slice(&channel.to_le_bytes());
            }
        }
        if let Err(error) = self.file.write_all(&bytes) {
            let _ = self.file.set_len(self.last_good);
            return Err(error).context("Audio write failed; complete packets remain recoverable");
        }
        self.last_good += bytes.len() as u64;
        self.last_end = start + samples.len() as u64;
        self.packets += 1;
        if self.packets.is_multiple_of(100) {
            self.file.sync_data()?;
        }
        Ok(())
    }
    pub fn finish(self) -> Result<()> {
        self.file.sync_all()?;
        Ok(())
    }
}
#[derive(Clone, Debug)]
struct Chunk {
    start: u64,
    frames: u32,
    offset: u64,
}
#[derive(Clone, Debug)]
pub struct Track {
    pub path: PathBuf,
    pub kind: Kind,
    pub recovered_tail: bool,
    chunks: Vec<Chunk>,
}
impl Track {
    pub fn open(path: &Path, kind: Kind) -> Result<Self> {
        let mut file = File::open(path)?;
        let length = file.metadata()?.len();
        ensure!(
            (16..=MAX_BYTES).contains(&length),
            "Invalid audio journal size"
        );
        let mut h = [0u8; 16];
        file.read_exact(&mut h)?;
        ensure!(
            &h[..8] == MAGIC
                && u32::from_le_bytes(h[8..12].try_into().unwrap()) == RATE as u32
                && u32::from_le_bytes(h[12..].try_into().unwrap()) == 2,
            "Invalid audio journal header"
        );
        let mut chunks: Vec<Chunk> = Vec::new();
        let mut position = 16;
        let mut recovered_tail = false;
        while position < length {
            ensure!(
                chunks.len() < MAX_PACKETS,
                "Audio packet index limit reached"
            );
            if length - position < 12 {
                recovered_tail = true;
                break;
            }
            let mut h = [0u8; 12];
            file.read_exact(&mut h)?;
            let start = u64::from_le_bytes(h[..8].try_into().unwrap());
            let frames = u32::from_le_bytes(h[8..].try_into().unwrap());
            ensure!(
                (1..=RATE as u32).contains(&frames)
                    && start <= RATE * 86400
                    && frames as u64 <= RATE * 86400 - start,
                "Invalid audio packet header"
            );
            ensure!(
                chunks
                    .last()
                    .is_none_or(|c| start >= c.start + c.frames as u64),
                "Overlapping audio journal packets"
            );
            let offset = position + 12;
            position = offset + frames as u64 * 8;
            if position > length {
                recovered_tail = true;
                break;
            }
            chunks.push(Chunk {
                start,
                frames,
                offset,
            });
            file.seek(SeekFrom::Start(position))?;
        }
        Ok(Self {
            path: path.to_path_buf(),
            kind,
            recovered_tail,
            chunks,
        })
    }
}
pub fn discover(project: &Path) -> Result<Vec<Track>> {
    let mut tracks = Vec::new();
    for kind in [Kind::Microphone, Kind::System] {
        let path = kind.path(project);
        if path.exists() {
            tracks.push(Track::open(&path, kind)?);
        }
    }
    Ok(tracks)
}
pub struct Mixer {
    tracks: Vec<(Track, File, f32)>,
}
impl Mixer {
    pub fn open(tracks: &[Track], microphone_gain: f32, system_gain: f32) -> Result<Self> {
        ensure!(
            [microphone_gain, system_gain]
                .iter()
                .all(|g| g.is_finite() && (0.0..=2.0).contains(g)),
            "Invalid audio gain"
        );
        let scale = if tracks.len() > 1 { 0.5 } else { 1.0 };
        let mut result = Vec::new();
        for track in tracks {
            result.push((
                track.clone(),
                File::open(&track.path)?,
                scale
                    * match track.kind {
                        Kind::Microphone => microphone_gain,
                        Kind::System => system_gain,
                    },
            ));
        }
        Ok(Self { tracks: result })
    }
    pub fn read(&mut self, start: u64, count: usize) -> Result<Vec<i16>> {
        ensure!(count <= 4800, "Audio export block exceeds 100 ms");
        ensure!(
            start <= RATE * 86400 && count as u64 <= RATE * 86400 - start,
            "Audio read exceeds timeline limit"
        );
        let mut mixed = vec![[0.0f32; 2]; count];
        let end = start + count as u64;
        for (track, file, gain) in &mut self.tracks {
            let first = track
                .chunks
                .partition_point(|c| c.start + c.frames as u64 <= start);
            for chunk in track.chunks[first..].iter().take_while(|c| c.start < end) {
                let left = start.max(chunk.start);
                let right = end.min(chunk.start + chunk.frames as u64);
                file.seek(SeekFrom::Start(chunk.offset + (left - chunk.start) * 8))?;
                let mut bytes = vec![0; (right - left) as usize * 8];
                file.read_exact(&mut bytes)?;
                for (i, sample) in bytes.chunks_exact(8).enumerate() {
                    for channel in 0..2 {
                        let value = f32::from_le_bytes(
                            sample[channel * 4..channel * 4 + 4].try_into().unwrap(),
                        );
                        ensure!(value.is_finite(), "Corrupt nonfinite audio sample");
                        mixed[(left - start) as usize + i][channel] += value * *gain;
                    }
                }
            }
        }
        Ok(mixed
            .into_iter()
            .flat_map(|s| s.map(|v| (v.clamp(-1.0, 1.0) * 32767.0).round() as i16))
            .collect())
    }
}

#[cfg(windows)]
mod wasapi;
#[cfg(windows)]
pub use wasapi::Session;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn silence_resampling_pause_trim_and_recovery() -> Result<()> {
        let format = Format {
            rate: 44_100,
            channels: 1,
            encoding: Encoding::Pcm16,
            mask: 0,
        };
        let mut timeline = crate::timeline::Timeline::new(0, false);
        timeline.pause(50_000, true);
        timeline.pause(100_000, false);
        let packets = convert(
            format,
            &vec![0; 882],
            441,
            false,
            0,
            &timeline.spans(0, 100_000),
        )?;
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].samples.len(), 240);
        assert!(packets[0].samples.iter().all(|s| *s == [0.0; 2]));
        let dir = crate::test_directory()?;
        let project = dir.path().join("fixture.srec");
        let path = Kind::Microphone.path(&project);
        let mut writer = TrackWriter::create(&path)?;
        writer.append(&packets[0])?;
        writer.finish()?;
        OpenOptions::new()
            .append(true)
            .open(&path)?
            .write_all(&[1, 2, 3])?;
        let track = Track::open(&path, Kind::Microphone)?;
        assert!(track.recovered_tail);
        let mut mixer = Mixer::open(&[track], 1.0, 1.0)?;
        assert_eq!(mixer.read(120, 480)?, vec![0; 960]);
        assert!(TrackWriter::create(&path).is_err());
        Ok(())
    }
    #[test]
    fn packet_phase_is_independent_of_native_packet_boundaries() -> Result<()> {
        let f = Format {
            rate: 44_100,
            channels: 2,
            encoding: Encoding::Float32,
            mask: 3,
        };
        let clock = crate::timeline::Timeline::new(0, false);
        let mut total = 0;
        for (begin, frames) in [(0, 441), (100_000, 441)] {
            let packets = convert(
                f,
                &[],
                frames,
                true,
                begin,
                &clock.spans(begin, begin + 100_000),
            )?;
            assert_eq!(packets[0].start, total);
            total += packets[0].samples.len() as u64;
        }
        assert_eq!(total, 960);
        Ok(())
    }
    #[test]
    fn rejects_corrupt_audio_lengths_timestamps_and_formats() -> Result<()> {
        let dir = crate::test_directory()?;
        let path = dir.path().join("corrupt.saudio");
        let writer = TrackWriter::create(&path)?;
        writer.finish()?;
        let mut file = OpenOptions::new().append(true).open(&path)?;
        file.write_all(&u64::MAX.to_le_bytes())?;
        file.write_all(&1u32.to_le_bytes())?;
        drop(file);
        assert!(Track::open(&path, Kind::Microphone).is_err());
        assert!(
            Format {
                rate: 48_000,
                channels: 6,
                mask: 0,
                encoding: Encoding::Float32
            }
            .validate()
            .is_err()
        );
        assert!(Mixer::open(&[], f32::NAN, 1.0).is_err());
        let mut mixer = Mixer::open(&[], 1.0, 1.0)?;
        assert!(mixer.read(u64::MAX, 4).is_err());
        assert!(
            convert(
                Format {
                    rate: 48_000,
                    channels: 2,
                    mask: 3,
                    encoding: Encoding::Pcm16
                },
                &[0; 2],
                1,
                false,
                0,
                &[]
            )
            .is_err()
        );
        Ok(())
    }
}
