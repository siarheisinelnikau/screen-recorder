//! Media Foundation H.264/AAC export. No capture or playback APIs are called here.
use anyhow::{Result, ensure};
use image::RgbImage;

pub fn padded_dimensions(width: u32, height: u32) -> (u32, u32) {
    (width.next_multiple_of(2), height.next_multiple_of(2))
}
/// Top-down BT.709 limited-range NV12. Odd edges are replicated, never cropped.
pub fn nv12(image: &RgbImage) -> Vec<u8> {
    let (width, height) = padded_dimensions(image.width(), image.height());
    let mut output = vec![0; (width * height * 3 / 2) as usize];
    let pixel = |x: u32, y: u32| {
        let p = image.get_pixel(x.min(image.width() - 1), y.min(image.height() - 1));
        [p[0] as f32, p[1] as f32, p[2] as f32]
    };
    for y in 0..height {
        for x in 0..width {
            let [r, g, b] = pixel(x, y);
            output[(y * width + x) as usize] = (16.0 + 0.182586 * r + 0.614231 * g + 0.062007 * b)
                .round()
                .clamp(16.0, 235.0) as u8;
        }
    }
    for y in (0..height).step_by(2) {
        for x in (0..width).step_by(2) {
            let mut sum = [0.0; 3];
            for dy in 0..2 {
                for dx in 0..2 {
                    let p = pixel(x + dx, y + dy);
                    for i in 0..3 {
                        sum[i] += p[i] / 4.0;
                    }
                }
            }
            let [r, g, b] = sum;
            let i = (width * height + y / 2 * width + x) as usize;
            output[i] = (128.0 - 0.100644 * r - 0.338572 * g + 0.439216 * b)
                .round()
                .clamp(16.0, 240.0) as u8;
            output[i + 1] = (128.0 + 0.439216 * r - 0.398942 * g - 0.040274 * b)
                .round()
                .clamp(16.0, 240.0) as u8;
        }
    }
    output
}

#[cfg(windows)]
mod foundation {
    use super::*;
    use crate::{
        audio::{Mixer, RATE},
        editor::{self, Edit},
        project::Project,
        windows_support::Apartment,
    };
    use anyhow::Context;
    use std::{
        path::Path,
        sync::atomic::{AtomicBool, Ordering},
    };
    use windows::{
        Win32::Media::MediaFoundation::*,
        core::{HSTRING, PCWSTR},
    };

    struct Runtime {
        _apartment: Apartment,
    }
    struct CreationGuard {
        stream: IMFByteStream,
        path: std::path::PathBuf,
        keep: bool,
    }
    impl Drop for CreationGuard {
        fn drop(&mut self) {
            if !self.keep {
                unsafe {
                    let _ = self.stream.Close();
                }
                let _ = std::fs::remove_file(&self.path);
            }
        }
    }
    impl Runtime {
        fn new() -> Result<Self> {
            let apartment = Apartment::new()?;
            unsafe {
                MFStartup(MF_VERSION, MFSTARTUP_LITE)?;
            }
            Ok(Self {
                _apartment: apartment,
            })
        }
    }
    impl Drop for Runtime {
        fn drop(&mut self) {
            unsafe {
                let _ = MFShutdown();
            }
        }
    }
    pub struct Writer {
        sink: Option<IMFSinkWriter>,
        stream: Option<IMFByteStream>,
        video: u32,
        audio: Option<u32>,
        width: u32,
        height: u32,
        path: std::path::PathBuf,
        _runtime: Runtime,
    }
    unsafe fn video_type(
        width: u32,
        height: u32,
        fps: u32,
        compressed: bool,
    ) -> Result<IMFMediaType> {
        unsafe {
            let ty = MFCreateMediaType()?;
            ty.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            ty.SetGUID(
                &MF_MT_SUBTYPE,
                if compressed {
                    &MFVideoFormat_H264
                } else {
                    &MFVideoFormat_NV12
                },
            )?;
            ty.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
            ty.SetUINT64(&MF_MT_FRAME_SIZE, ((width as u64) << 32) | height as u64)?;
            ty.SetUINT64(&MF_MT_FRAME_RATE, ((fps as u64) << 32) | 1)?;
            ty.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, (1u64 << 32) | 1)?;
            ty.SetUINT32(&MF_MT_YUV_MATRIX, MFVideoTransferMatrix_BT709.0 as u32)?;
            ty.SetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE, MFNominalRange_16_235.0 as u32)?;
            if compressed {
                ty.SetUINT32(
                    &MF_MT_AVG_BITRATE,
                    (width as u64 * height as u64 * fps as u64 / 6).clamp(2_000_000, 20_000_000)
                        as u32,
                )?;
            } else {
                ty.SetUINT32(&MF_MT_DEFAULT_STRIDE, width)?;
                ty.SetUINT32(&MF_MT_SAMPLE_SIZE, width * height * 3 / 2)?;
                ty.SetUINT32(&MF_MT_ALL_SAMPLES_INDEPENDENT, 1)?;
            }
            Ok(ty)
        }
    }
    unsafe fn audio_type(compressed: bool) -> Result<IMFMediaType> {
        unsafe {
            let ty = MFCreateMediaType()?;
            ty.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
            ty.SetGUID(
                &MF_MT_SUBTYPE,
                if compressed {
                    &MFAudioFormat_AAC
                } else {
                    &MFAudioFormat_PCM
                },
            )?;
            ty.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, 2)?;
            ty.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, RATE as u32)?;
            ty.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
            ty.SetUINT32(
                &MF_MT_AUDIO_AVG_BYTES_PER_SECOND,
                if compressed { 24_000 } else { RATE as u32 * 4 },
            )?;
            ty.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, if compressed { 1 } else { 4 })?;
            if compressed {
                ty.SetUINT32(&MF_MT_AAC_PAYLOAD_TYPE, 0)?;
            } else {
                ty.SetUINT32(&MF_MT_ALL_SAMPLES_INDEPENDENT, 1)?;
            }
            Ok(ty)
        }
    }
    unsafe fn sample(bytes: &[u8], time: i64, duration: i64) -> Result<IMFSample> {
        unsafe {
            ensure!(
                !bytes.is_empty() && duration > 0,
                "Invalid Media Foundation sample"
            );
            let buffer = MFCreateMemoryBuffer(bytes.len() as u32)?;
            let mut pointer = std::ptr::null_mut();
            buffer.Lock(&mut pointer, None, None)?;
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), pointer, bytes.len());
            buffer.Unlock()?;
            buffer.SetCurrentLength(bytes.len() as u32)?;
            let sample = MFCreateSample()?;
            sample.AddBuffer(&buffer)?;
            sample.SetSampleTime(time)?;
            sample.SetSampleDuration(duration)?;
            Ok(sample)
        }
    }
    impl Writer {
        pub fn create(path: &Path, width: u32, height: u32, fps: u32, audio: bool) -> Result<Self> {
            crate::project::validate_dimensions(width, height, fps)?;
            let runtime = Runtime::new().context(
                "Media Foundation is unavailable (Windows N requires Media Feature Pack)",
            )?;
            let (width, height) = padded_dimensions(width, height);
            unsafe {
                let stream = MFCreateFile(
                    MF_ACCESSMODE_WRITE,
                    MF_OPENMODE_FAIL_IF_EXIST,
                    MF_FILEFLAGS_NONE,
                    &HSTRING::from(path.as_os_str()),
                )?;
                let mut guard = CreationGuard {
                    stream: stream.clone(),
                    path: path.to_path_buf(),
                    keep: false,
                };
                let mut attributes = None;
                MFCreateAttributes(&mut attributes, 3)?;
                let attributes = attributes.context("MFCreateAttributes returned null")?;
                attributes.SetGUID(&MF_TRANSCODE_CONTAINERTYPE, &MFTranscodeContainerType_MPEG4)?;
                attributes.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 0)?;
                let sink = MFCreateSinkWriterFromURL(PCWSTR::null(), &stream, &attributes)
                    .context("MP4 sink is unavailable")?;
                let video = sink
                    .AddStream(&video_type(width, height, fps, true)?)
                    .context("Windows H.264 encoder is unavailable or rejects the resolution")?;
                sink.SetInputMediaType(video, &video_type(width, height, fps, false)?, None)
                    .context("H.264 encoder rejected NV12 input")?;
                let audio = if audio {
                    let index = sink
                        .AddStream(&audio_type(true)?)
                        .context("Windows AAC encoder is unavailable")?;
                    sink.SetInputMediaType(index, &audio_type(false)?, None)?;
                    Some(index)
                } else {
                    None
                };
                sink.BeginWriting()?;
                guard.keep = true;
                Ok(Self {
                    sink: Some(sink),
                    stream: Some(stream),
                    video,
                    audio,
                    width,
                    height,
                    path: path.to_path_buf(),
                    _runtime: runtime,
                })
            }
        }
        pub fn video(&mut self, frame: &RgbImage, time: i64, duration: i64) -> Result<()> {
            ensure!(
                padded_dimensions(frame.width(), frame.height()) == (self.width, self.height),
                "MP4 frame size changed"
            );
            unsafe {
                self.sink
                    .as_ref()
                    .unwrap()
                    .WriteSample(self.video, &sample(&nv12(frame), time, duration)?)
                    .context("H.264 encoding/write failed")?;
            }
            Ok(())
        }
        pub fn audio(&mut self, pcm: &[i16], first_frame: u64) -> Result<()> {
            let index = self.audio.context("MP4 has no audio stream")?;
            ensure!(
                !pcm.is_empty() && pcm.len().is_multiple_of(2),
                "Invalid stereo audio sample"
            );
            let bytes: Vec<u8> = pcm.iter().flat_map(|n| n.to_le_bytes()).collect();
            let start = (first_frame * 10_000_000 / RATE) as i64;
            let end = ((first_frame + pcm.len() as u64 / 2) * 10_000_000 / RATE) as i64;
            unsafe {
                self.sink
                    .as_ref()
                    .unwrap()
                    .WriteSample(index, &sample(&bytes, start, end - start)?)
                    .context("AAC encoding/write failed")?;
            }
            Ok(())
        }
        pub fn finish(mut self) -> Result<()> {
            unsafe {
                self.sink
                    .as_ref()
                    .unwrap()
                    .Finalize()
                    .context("MP4 finalization failed; output was not published")?;
            }
            self.sink.take();
            if let Some(stream) = self.stream.take() {
                unsafe {
                    // Releasing the sink can already close its byte stream.
                    let _ = stream.Close();
                }
            }
            std::fs::OpenOptions::new()
                .write(true)
                .open(&self.path)?
                .sync_all()
                .context("Flushing finalized MP4 file")?;
            Ok(())
        }
    }
    impl Drop for Writer {
        fn drop(&mut self) {
            self.sink.take();
            if let Some(stream) = self.stream.take() {
                unsafe {
                    let _ = stream.Close();
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
        let partial = output.with_extension("partial.mp4");
        // Writer uses FAIL_IF_EXIST; an existing partial is never opened or removed.
        let mut writer = Writer::create(
            &partial,
            project.width,
            project.height,
            project.fps,
            edit.include_audio && !project.audio_tracks.is_empty(),
        )?;
        let result = (|| -> Result<()> {
            let mut mixer = if edit.include_audio && !project.audio_tracks.is_empty() {
                Some(Mixer::open(
                    &project.audio_tracks,
                    edit.microphone_gain,
                    edit.system_gain,
                )?)
            } else {
                None
            };
            let automatic = editor::automatic_zooms(project);
            let duration = edit.trim_end - edit.trim_start;
            let end_ticks = (duration * 10_000_000.0).round() as i64;
            ensure!(
                end_ticks > 0,
                "Trim range is shorter than one Media Foundation clock tick"
            );
            let count = (end_ticks as u64 * project.fps as u64).div_ceil(10_000_000) as usize;
            ensure!(
                count <= crate::project::MAX_FRAMES,
                "Export frame limit reached; trim a shorter segment"
            );
            let trim_frame = (edit.trim_start * RATE as f64).round() as u64;
            let audio_frames = (duration * RATE as f64).round() as u64;
            let mut audio_written = 0;
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
                let frame = editor::render(
                    project,
                    edit,
                    &automatic,
                    cached.as_ref().unwrap(),
                    time,
                    project.width,
                    project.height,
                );
                let start = i as i64 * 10_000_000 / project.fps as i64;
                let end = ((i + 1) as i64 * 10_000_000 / project.fps as i64).min(end_ticks);
                writer.video(&frame, start, end - start)?;
                if let Some(mixer) = mixer.as_mut() {
                    let until = if end == end_ticks {
                        audio_frames
                    } else {
                        ((end as u64 * RATE) / 10_000_000).min(audio_frames)
                    };
                    while audio_written < until {
                        ensure!(!cancel.load(Ordering::Relaxed), "Export cancelled");
                        let n = (until - audio_written).min(4800) as usize;
                        writer.audio(&mixer.read(trim_frame + audio_written, n)?, audio_written)?;
                        audio_written += n as u64;
                    }
                }
                progress((i + 1) as f32 / count as f32);
            }
            ensure!(!cancel.load(Ordering::Relaxed), "Export cancelled");
            Ok(())
        })();
        let result = if result.is_ok() {
            writer.finish()
        } else {
            drop(writer);
            result
        };
        if let Err(error) = result {
            let _ = std::fs::remove_file(&partial);
            return Err(error);
        }
        if let Err(error) = editor::publish_new(&partial, output) {
            let _ = std::fs::remove_file(&partial);
            return Err(error);
        }
        Ok(())
    }
}
#[cfg(windows)]
pub use foundation::{Writer, export};

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nv12_padding_and_luma_chroma_are_top_down() {
        let image = RgbImage::from_fn(65, 65, |_, y| {
            if y < 32 {
                image::Rgb([255, 255, 255])
            } else {
                image::Rgb([0, 0, 0])
            }
        });
        let data = nv12(&image);
        assert_eq!(padded_dimensions(65, 65), (66, 66));
        assert_eq!(data.len(), 66 * 66 * 3 / 2);
        assert_eq!(data[0], 235);
        assert_eq!(data[65 * 66], 16);
        assert!(data[66 * 66..].iter().all(|v| *v == 128));
    }
}
