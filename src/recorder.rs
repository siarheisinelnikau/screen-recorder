use crate::{capture::FrameSource, project::ProjectWriter};
use anyhow::Result;
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    time::{Duration, Instant},
};

#[derive(Default)]
pub struct Control {
    stop: AtomicBool,
    paused: AtomicBool,
    wake: Option<SyncSender<()>>,
}
impl Control {
    pub fn channel() -> (Arc<Self>, Receiver<()>) {
        let (tx, rx) = mpsc::sync_channel(1);
        (
            Arc::new(Self {
                wake: Some(tx),
                ..Default::default()
            }),
            rx,
        )
    }
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
        self.wake();
    }
    pub fn pause(&self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
        self.wake();
    }
    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }
    fn wake(&self) {
        if let Some(tx) = &self.wake {
            let _ = tx.try_send(());
        }
    }
}

pub fn record(
    source: &mut impl FrameSource,
    path: &Path,
    fps: u32,
    control: &Control,
    wake: &Receiver<()>,
    progress: impl Fn(u64, f64),
) -> Result<()> {
    let (width, height) = source.dimensions();
    let mut writer = ProjectWriter::create(path, width, height, fps)?;
    let result = (|| -> Result<()> {
        let interval = Duration::from_secs_f64(1.0 / fps as f64);
        let start = Instant::now();
        let mut paused = Duration::ZERO;
        let mut deadline = start;
        let mut frames = 0;
        let mut last_status = start;
        loop {
            if control.stop.load(Ordering::Relaxed) || source.stop_shortcut() {
                break;
            }
            if control.is_paused() {
                let pause_start = Instant::now();
                while control.is_paused() && !control.stop.load(Ordering::Relaxed) {
                    if source.stop_shortcut() {
                        control.stop();
                        break;
                    }
                    if matches!(
                        wake.recv_timeout(Duration::from_millis(250)),
                        Err(mpsc::RecvTimeoutError::Disconnected)
                    ) {
                        return Ok(());
                    }
                }
                paused += pause_start.elapsed();
                deadline = Instant::now();
                continue;
            }
            let now = Instant::now();
            if now < deadline {
                let _ = wake.recv_timeout(deadline - now);
                continue;
            }
            let timestamp = now.duration_since(start).saturating_sub(paused).as_micros() as u64;
            let (frame, cursor) = source.sample()?;
            writer.append(&frame, timestamp, cursor)?;
            frames += 1;
            if last_status.elapsed() >= Duration::from_millis(250) || frames == 1 {
                progress(frames, timestamp as f64 / 1e6);
                last_status = Instant::now();
            }
            deadline += interval;
            if deadline < Instant::now() {
                deadline = Instant::now();
            }
        }
        Ok(())
    })();
    // Even a source/disk failure attempts to flush every completed frame.
    let finalization = writer.finish();
    result.and(finalization)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{Cursor, Project};
    struct Synthetic {
        n: u8,
    }
    impl FrameSource for Synthetic {
        fn dimensions(&self) -> (u32, u32) {
            (64, 64)
        }
        fn sample(&mut self) -> Result<(image::RgbImage, Cursor)> {
            self.n += 1;
            if self.n == 3 {
                anyhow::bail!("Injected capture failure");
            }
            Ok((
                image::RgbImage::from_pixel(64, 64, image::Rgb([self.n, 0, 0])),
                Cursor::default(),
            ))
        }
    }
    #[test]
    fn source_failure_preserves_completed_frames() -> Result<()> {
        let dir = crate::test_directory()?;
        let path = dir.path().join("failure.srec");
        let (control, wake) = Control::channel();
        assert!(
            record(
                &mut Synthetic { n: 0 },
                &path,
                30,
                &control,
                &wake,
                |_, _| {}
            )
            .is_err()
        );
        let p = Project::open(&path)?;
        assert_eq!(p.frames.len(), 2);
        assert!(!p.recovered_tail);
        Ok(())
    }
    #[test]
    fn paused_stop_never_samples_a_source() -> Result<()> {
        let dir = crate::test_directory()?;
        let (control, wake) = Control::channel();
        control.pause(true);
        control.stop();
        let mut source = Synthetic { n: 0 };
        record(
            &mut source,
            &dir.path().join("empty.srec"),
            10,
            &control,
            &wake,
            |_, _| {},
        )?;
        assert_eq!(source.n, 0);
        Ok(())
    }
}
