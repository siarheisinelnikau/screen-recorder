//! Safe end-to-end fixture generator. Never imports or constructs a desktop Source.
use anyhow::{Result, ensure};
use image::{Rgb, RgbImage};
use screen_recorder::{
    audio::{Kind, Packet, TrackWriter},
    editor::{self, Edit},
    project::{Cursor, Project, ProjectWriter},
};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::atomic::AtomicBool,
    time::{SystemTime, UNIX_EPOCH},
};

fn main() -> Result<()> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".test-output")
        .join(format!(
            "synthetic-{}",
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
    fs::create_dir_all(&root)?;
    let path = root.join("tutorial.srec");
    let mut writer = ProjectWriter::create(&path, 320, 180, 10)?;
    for i in 0..20 {
        let frame = RgbImage::from_fn(320, 180, |x, y| {
            if x > 40 && x < 280 && y > 30 && y < 150 {
                Rgb([45 + (i * 3) as u8, 87, 108])
            } else {
                Rgb([20, 26, 36])
            }
        });
        writer.append(
            &frame,
            i * 100_000,
            Cursor {
                x: 0.4 + i as f32 * 0.01,
                y: 0.5,
                visible: true,
                click: i == 7 || i == 13,
            },
        )?;
    }
    writer.finish()?;
    // Two synthetic silent tracks exercise mixing/AAC without opening endpoints.
    for kind in [Kind::Microphone, Kind::System] {
        let mut audio = TrackWriter::create(&kind.path(&path))?;
        for i in 0..2 {
            audio.append(&Packet {
                start: i * 48_000,
                samples: vec![[0.0; 2]; 48_000],
            })?;
        }
        audio.finish()?;
    }
    let project = Project::open(&path)?;
    let mut edit = Edit::new(&project);
    edit.trim_start = 0.2;
    edit.trim_end = 1.8;
    edit.save(&project)?;
    editor::export(
        &project,
        &edit,
        &root.join("tutorial-edited.avi"),
        &AtomicBool::new(false),
        |_| {},
    )?;
    #[cfg(windows)]
    screen_recorder::mp4::export(
        &project,
        &edit,
        &root.join("tutorial-edited.mp4"),
        &AtomicBool::new(false),
        |_| {},
    )?;
    // Standalone silence is also kept as a PCM WAV fixture; AVI remains video-only.
    let samples = 32_000u32;
    let mut wav = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join("silence.wav"))?;
    wav.write_all(b"RIFF")?;
    wav.write_all(&(36 + samples * 2).to_le_bytes())?;
    wav.write_all(b"WAVEfmt ")?;
    wav.write_all(&16u32.to_le_bytes())?;
    wav.write_all(&1u16.to_le_bytes())?;
    wav.write_all(&1u16.to_le_bytes())?;
    wav.write_all(&16_000u32.to_le_bytes())?;
    wav.write_all(&32_000u32.to_le_bytes())?;
    wav.write_all(&2u16.to_le_bytes())?;
    wav.write_all(&16u16.to_le_bytes())?;
    wav.write_all(b"data")?;
    wav.write_all(&(samples * 2).to_le_bytes())?;
    wav.write_all(&vec![0; samples as usize * 2])?;
    wav.sync_all()?;
    ensure!(project.frames.len() == 20, "Unexpected fixture frame count");
    println!("Synthetic fixtures: {}", root.display());
    Ok(())
}
