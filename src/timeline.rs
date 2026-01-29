//! Shared monotonic clock mapping for video and timestamped audio packets.
pub const TICKS_PER_SECOND: u64 = 10_000_000;

#[derive(Clone, Copy, Debug)]
struct Boundary {
    at: u64,
    active: u64,
    paused: bool,
}
#[derive(Debug)]
pub struct Timeline {
    origin: u64,
    boundaries: Vec<Boundary>,
    end: Option<u64>,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Span {
    pub begin: u64,
    pub end: u64,
    pub active_begin: u64,
}
impl Timeline {
    pub fn new(origin: u64, paused: bool) -> Self {
        Self {
            origin,
            boundaries: vec![Boundary {
                at: origin,
                active: 0,
                paused,
            }],
            end: None,
        }
    }
    pub fn elapsed(&self, at: u64) -> u64 {
        let at = at.min(self.end.unwrap_or(u64::MAX));
        if at < self.origin {
            return 0;
        }
        let boundary = &self.boundaries[self.boundaries.partition_point(|b| b.at <= at) - 1];
        boundary.active + if boundary.paused { 0 } else { at - boundary.at }
    }
    pub fn pause(&mut self, at: u64, paused: bool) {
        if self.end.is_some() || self.boundaries.last().is_some_and(|b| b.paused == paused) {
            return;
        }
        let at = at.max(self.boundaries.last().unwrap().at);
        self.boundaries.push(Boundary {
            at,
            active: self.elapsed(at),
            paused,
        });
    }
    pub fn stop(&mut self, at: u64) {
        self.end.get_or_insert(at.max(self.origin));
    }
    pub fn spans(&self, begin: u64, end: u64) -> Vec<Span> {
        let mut begin = begin.max(self.origin);
        let end = end.min(self.end.unwrap_or(u64::MAX));
        let mut result = Vec::new();
        while begin < end {
            let index = self.boundaries.partition_point(|b| b.at <= begin) - 1;
            let b = self.boundaries[index];
            let limit = self
                .boundaries
                .get(index + 1)
                .map_or(end, |b| b.at.min(end));
            if !b.paused {
                result.push(Span {
                    begin,
                    end: limit,
                    active_begin: self.elapsed(begin),
                });
            }
            begin = limit;
        }
        result
    }
}
pub fn now() -> u64 {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::System::Performance::{
            QueryPerformanceCounter, QueryPerformanceFrequency,
        };
        static FREQUENCY: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
        let frequency = *FREQUENCY.get_or_init(|| {
            let mut n = 0;
            QueryPerformanceFrequency(&mut n).expect("Windows performance counter unavailable");
            n as u64
        });
        let mut n = 0;
        QueryPerformanceCounter(&mut n).expect("Windows performance counter unavailable");
        (n as u128 * TICKS_PER_SECOND as u128 / frequency as u128) as u64
    }
    #[cfg(not(windows))]
    {
        static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        (START
            .get_or_init(std::time::Instant::now)
            .elapsed()
            .as_nanos()
            / 100) as u64
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn excludes_pause_and_clips_packets_at_every_boundary() {
        let mut clock = Timeline::new(100, false);
        clock.pause(200, true);
        clock.pause(400, false);
        clock.stop(500);
        assert_eq!(clock.elapsed(600), 200);
        assert_eq!(
            clock.spans(0, 600),
            vec![
                Span {
                    begin: 100,
                    end: 200,
                    active_begin: 0
                },
                Span {
                    begin: 400,
                    end: 500,
                    active_begin: 100
                }
            ]
        );
        assert_eq!(clock.spans(250, 350), vec![]);
    }
}
