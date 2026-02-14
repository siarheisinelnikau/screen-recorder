#![cfg(windows)]
//! Explicit opt-in test. Encodes/decodes artificial pixels and silence, never devices.
use anyhow::{Context, Result, ensure};
use screen_recorder::{
    audio::{Kind, Packet, TrackWriter},
    editor::Edit,
    mp4,
    project::{Cursor, Project, ProjectWriter},
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};
use windows::{
    Win32::{Media::MediaFoundation::*, System::Com::*},
    core::HSTRING,
};

struct Runtime;
impl Runtime {
    fn new() -> Result<Self> {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
            if let Err(e) = MFStartup(MF_VERSION, MFSTARTUP_LITE) {
                CoUninitialize();
                return Err(e.into());
            }
        }
        Ok(Self)
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        unsafe {
            let _ = MFShutdown();
            CoUninitialize();
        }
    }
}

fn decode(path: &Path, audio: bool) -> Result<(usize, i64)> {
    let _runtime = Runtime::new()?;
    unsafe {
        let mut attributes = None;
        MFCreateAttributes(&mut attributes, 2)?;
        let attributes = attributes.unwrap();
        attributes.SetUINT32(&MF_SOURCE_READER_DISABLE_DXVA, 1)?;
        attributes.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 0)?;
        let reader = MFCreateSourceReaderFromURL(&HSTRING::from(path.as_os_str()), &attributes)?;
        reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)?;
        let stream = if audio {
            MF_SOURCE_READER_FIRST_AUDIO_STREAM.0
        } else {
            MF_SOURCE_READER_FIRST_VIDEO_STREAM.0
        } as u32;
        reader.SetStreamSelection(stream, true)?;
        let ty = MFCreateMediaType()?;
        ty.SetGUID(
            &MF_MT_MAJOR_TYPE,
            if audio {
                &MFMediaType_Audio
            } else {
                &MFMediaType_Video
            },
        )?;
        ty.SetGUID(
            &MF_MT_SUBTYPE,
            if audio {
                &MFAudioFormat_PCM
            } else {
                &MFVideoFormat_NV12
            },
        )?;
        if audio {
            ty.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
            ty.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, 2)?;
            ty.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, 48_000)?;
        }
        reader.SetCurrentMediaType(stream, None, &ty)?;
        let mut count = 0;
        let mut end = 0;
        let mut last = -1;
        for _ in 0..200 {
            let mut flags = 0;
            let mut timestamp = 0;
            let mut sample = None;
            reader.ReadSample(
                stream,
                0,
                None,
                Some(&mut flags),
                Some(&mut timestamp),
                Some(&mut sample),
            )?;
            ensure!(
                flags & MF_SOURCE_READERF_ERROR.0 as u32 == 0,
                "Source reader error"
            );
            if let Some(sample) = sample {
                ensure!(timestamp >= last, "Decoded timestamps went backwards");
                last = timestamp;
                end = end.max(timestamp + sample.GetSampleDuration()?);
                let buffer = sample.ConvertToContiguousBuffer()?;
                let mut data = std::ptr::null_mut();
                let mut length = 0;
                buffer.Lock(&mut data, None, Some(&mut length))?;
                let bytes = std::slice::from_raw_parts(data, length as usize);
                let silent = !audio
                    || bytes
                        .chunks_exact(2)
                        .all(|p| i16::from_le_bytes(p.try_into().unwrap()).unsigned_abs() <= 2);
                buffer.Unlock()?;
                ensure!(silent, "Decoded AAC fixture is not silent");
                count += 1;
            }
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                ensure!(count > 0, "No decoded samples");
                return Ok((count, end));
            }
        }
        anyhow::bail!("Decoder did not terminate within the bounded fixture sample count")
    }
}

#[test]
#[ignore = "Explicit synthetic-only Media Foundation encoder/decoder test; no capture endpoints"]
fn h264_aac_round_trip_trim_cancel_and_existing_files() -> Result<()> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".test-output");
    fs::create_dir_all(&root)?;
    let directory = tempfile::tempdir_in(root)?;
    let path = directory.path().join("artificial.srec");
    let mut writer = ProjectWriter::create(&path, 127, 71, 10)?;
    for i in 0..5 {
        writer.append(
            &image::RgbImage::from_fn(127, 71, |x, y| image::Rgb([x as u8, y as u8, i as u8 * 25])),
            i * 100_000,
            Cursor::default(),
        )?;
    }
    writer.finish()?;
    for kind in [Kind::Microphone, Kind::System] {
        let mut audio = TrackWriter::create(&kind.path(&path))?;
        audio.append(&Packet {
            start: 0,
            samples: vec![[0.0; 2]; 24_000],
        })?;
        audio.finish()?;
    }
    let project = Project::open(&path)?;
    let mut edit = Edit::new(&project);
    edit.trim_start = 0.1;
    edit.trim_end = 0.4;
    edit.show_cursor = false;
    edit.highlight = false;
    edit.clicks = false;
    let output = directory.path().join("fixture.mp4");
    let cancel = AtomicBool::new(false);
    mp4::export(&project, &edit, &output, &cancel, |_| {})
        .context("Encoding synthetic H.264/AAC MP4")?;
    let data = fs::read(&output)?;
    ensure!(
        data.windows(4).any(|b| b == b"avc1") && data.windows(4).any(|b| b == b"mp4a"),
        "MP4 codec entries are absent"
    );
    let (video_frames, video_end) = decode(&output, false).context("Decoding synthetic video")?;
    ensure!(
        video_frames == 3,
        "Unexpected trimmed video frame count: {video_frames}"
    );
    let (audio_samples, audio_end) = decode(&output, true).context("Decoding synthetic audio")?;
    ensure!(
        (video_end - 3_000_000).abs() < 200_000,
        "Unexpected video duration: {video_end}"
    );
    // AAC encoders may add a priming/padding frame. Verify bounded synchronization.
    ensure!(
        (audio_end - video_end).abs() < 700_000,
        "AAC/video duration mismatch: {audio_end} vs {video_end}"
    );
    ensure!(mp4::export(&project, &edit, &output, &cancel, |_| {}).is_err());
    ensure!(fs::read(&output)? == data, "Existing output changed");
    let cancelled = directory.path().join("cancelled.mp4");
    cancel.store(true, Ordering::Relaxed);
    ensure!(mp4::export(&project, &edit, &cancelled, &cancel, |_| {}).is_err());
    ensure!(
        !cancelled.exists() && !cancelled.with_extension("partial.mp4").exists(),
        "Cancelled output was retained"
    );
    cancel.store(false, Ordering::Relaxed);
    let blocked = directory.path().join("blocked.mp4");
    let partial = blocked.with_extension("partial.mp4");
    fs::write(&partial, b"preserve fixture")?;
    ensure!(mp4::export(&project, &edit, &blocked, &cancel, |_| {}).is_err());
    ensure!(fs::read(&partial)? == b"preserve fixture");
    edit.include_audio = false;
    let silent = directory.path().join("video-only.mp4");
    mp4::export(&project, &edit, &silent, &cancel, |_| {})?;
    ensure!(
        !fs::read(&silent)?.windows(4).any(|b| b == b"mp4a"),
        "Muted export unexpectedly has AAC"
    );
    eprintln!(
        "Synthetic Media Foundation round trip: {video_frames} H.264 frames, {audio_samples} decoded silent AAC buffers, video end {video_end}, audio end {audio_end}; odd size padded to 128x72"
    );
    Ok(())
}
