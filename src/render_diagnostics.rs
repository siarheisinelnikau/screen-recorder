//! ScreenRecorder's opt-in root-viewport timing hooks. Modified extension, 2026.
#![allow(missing_docs)]
use std::{
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};
static ENABLED: AtomicBool = AtomicBool::new(false);
static DATA: OnceLock<Mutex<Snapshot>> = OnceLock::new();
#[derive(Clone, Debug, Default)]
pub struct Timings {
    pub count: u64,
    pub errors: u64,
    pub total_us: u64,
    pub max_us: u64,
    pub samples_us: Vec<u64>,
}
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub frame: Timings,
    pub layout: Timings,
    pub paint: Timings,
    pub present: Timings,
}
pub enum Stage {
    Frame,
    Layout,
    Paint,
    Present,
}
pub struct FrameGuard(Option<Instant>);
pub fn frame() -> FrameGuard {
    FrameGuard(begin())
}
impl Drop for FrameGuard {
    fn drop(&mut self) {
        finish(Stage::Frame, self.0.take(), !std::thread::panicking());
    }
}
pub fn enable() {
    *DATA.get_or_init(Default::default).lock().unwrap() = Snapshot::default();
    ENABLED.store(true, Ordering::Relaxed);
}
pub fn begin() -> Option<Instant> {
    ENABLED.load(Ordering::Relaxed).then(Instant::now)
}
pub fn finish(stage: Stage, start: Option<Instant>, success: bool) {
    let Some(start) = start else {
        return;
    };
    let us = start.elapsed().as_micros() as u64;
    let mut data = DATA.get_or_init(Default::default).lock().unwrap();
    let timing = match stage {
        Stage::Frame => &mut data.frame,
        Stage::Layout => &mut data.layout,
        Stage::Paint => &mut data.paint,
        Stage::Present => &mut data.present,
    };
    timing.count += 1;
    timing.errors += u64::from(!success);
    timing.total_us += us;
    timing.max_us = timing.max_us.max(us);
    if timing.samples_us.len() < 512 {
        timing.samples_us.push(us);
    }
}
pub fn snapshot() -> Snapshot {
    DATA.get_or_init(Default::default).lock().unwrap().clone()
}
