//! Frame telemetry (roadmap M4.2a): how long each frame took to render,
//! and how many frames were drawn although nothing on screen changed.
//! Principle 2's budget is checked on these numbers: the 99th percentile
//! of render time (`frame_p99_ms`) and idle frames, which must be 0.

use std::fmt;
use std::time::Duration;

/// Frames recorded so far.
#[derive(Debug, Default)]
pub struct Telemetry {
    render: Vec<Duration>,
    idle: u64,
}

/// What [`Telemetry`] measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Summary {
    pub frames: usize,
    pub p50: Duration,
    pub p99: Duration,
    /// Frames drawn while nothing changed: wasted work, and battery.
    pub idle_frames: u64,
}

impl Telemetry {
    /// Records one drawn frame: the time from starting to render it to
    /// handing it to the display, and whether anything had changed.
    pub fn frame(&mut self, took: Duration, damaged: bool) {
        self.render.push(took);
        if !damaged {
            self.idle += 1;
        }
    }

    pub fn summary(&self) -> Summary {
        let mut sorted = self.render.clone();
        sorted.sort_unstable();
        Summary {
            frames: sorted.len(),
            p50: percentile(&sorted, 50),
            p99: percentile(&sorted, 99),
            idle_frames: self.idle,
        }
    }
}

/// The `p`th percentile of `sorted` by nearest rank: the smallest value
/// with at least `p` percent of the values at or below it, as CI's
/// desktop test computes it from the session (M4.1). Zero when empty.
pub fn percentile(sorted: &[Duration], p: u32) -> Duration {
    if sorted.is_empty() {
        return Duration::ZERO;
    }
    let rank = (sorted.len() * p as usize).div_ceil(100).max(1);
    sorted[rank.min(sorted.len()) - 1]
}

impl fmt::Display for Summary {
    /// One line for people and for CI's grep: `frames 300, render p50
    /// 1.20 ms, p99 3.40 ms, idle frames 0`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        write!(
            f,
            "frames {}, render p50 {:.2} ms, p99 {:.2} ms, idle frames {}",
            self.frames,
            ms(self.p50),
            ms(self.p99),
            self.idle_frames
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn percentiles_by_nearest_rank() {
        let sorted: Vec<Duration> = (1..=100).map(ms).collect();
        assert_eq!(percentile(&sorted, 50), ms(50));
        assert_eq!(percentile(&sorted, 99), ms(99));
        assert_eq!(percentile(&sorted, 100), ms(100));
        let few = [ms(1), ms(2), ms(3)];
        assert_eq!(percentile(&few, 50), ms(2));
        assert_eq!(percentile(&few, 99), ms(3));
        assert_eq!(percentile(&few, 0), ms(1));
        assert_eq!(percentile(&[], 99), Duration::ZERO);
        assert_eq!(percentile(&[ms(7)], 99), ms(7));
    }

    #[test]
    fn a_summary_counts_frames_and_idle_ones() {
        let mut t = Telemetry::default();
        for n in [5, 1, 3, 2, 4] {
            t.frame(ms(n), true);
        }
        t.frame(ms(9), false);
        let s = t.summary();
        assert_eq!(s.frames, 6);
        assert_eq!(s.p50, ms(3));
        assert_eq!(s.p99, ms(9));
        assert_eq!(s.idle_frames, 1);
        assert_eq!(
            s.to_string(),
            "frames 6, render p50 3.00 ms, p99 9.00 ms, idle frames 1"
        );
    }

    #[test]
    fn no_frames_is_a_quiet_summary() {
        let s = Telemetry::default().summary();
        assert_eq!(
            s.to_string(),
            "frames 0, render p50 0.00 ms, p99 0.00 ms, idle frames 0"
        );
    }
}
