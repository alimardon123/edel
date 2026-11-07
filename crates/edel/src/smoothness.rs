//! Each game's smoothness (roadmap M7.7d): while a game plays in game
//! mode, the compositor counts each of its frames as it reaches the
//! screen and keeps the time between them in [`Frames`], a fixed
//! histogram, so an hour's play costs no more memory than a minute's.
//! When the game ends, its [`Record`] (the frames shown, the average frame
//! rate and the slowest 1 and 0.1 percent of frames) replaces that app's
//! in [`places::GAMES`] in the person's state directory, and `edel report`
//! shows them, so every tuning step is judged on numbers. Only the last
//! game of each app is kept, and the file is a few lines a game:
//!
//! ```toml
//! format = 1
//!
//! [apps."com.valvesoftware.Steam"]
//! frames = 3600
//! seconds = 60.0
//! average_fps = 60.0
//! slowest_1_percent_ms = 18.4
//! slowest_0_1_percent_ms = 25.0
//! ```
//!
//! Std, serde and toml only, so the compositor writes it without `os`.

use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// How fine the frames' histogram is: a tenth of a millisecond.
const STEP_US: u64 = 100;
/// The frames' histogram's buckets: a frame time of a second or more is
/// counted in the last.
const BUCKETS: usize = 10_000;
/// How fine [`Work`]'s histogram is, 10 microseconds, and its buckets: a
/// frame drawn in 100 ms or more is counted in the last.
const WORK_STEP_US: u64 = 10;
const WORK_BUCKETS: usize = 10_000;
/// How many frames a game shows before it has a record: a window that
/// went fullscreen for a moment is no game.
pub const LEAST_FRAMES: u64 = 60;

/// Durations counted in fixed steps, so an hour costs no more memory
/// than a minute.
#[derive(Debug, Clone)]
struct Histogram {
    /// How many durations fell in each step of `step_us`.
    buckets: Vec<u32>,
    step_us: u64,
    count: u64,
}

impl Histogram {
    fn new(step_us: u64, buckets: usize) -> Self {
        Histogram {
            buckets: vec![0; buckets],
            step_us,
            count: 0,
        }
    }

    fn add(&mut self, took: Duration) {
        let last = self.buckets.len() - 1;
        let step = usize::try_from(took.as_micros() / u128::from(self.step_us)).unwrap_or(last);
        self.buckets[step.min(last)] += 1;
        self.count += 1;
    }

    /// The duration in ms that `part` in `whole` of the counted ones reach
    /// or pass: the nearest rank from the slow end, as the compositor's
    /// telemetry takes its percentiles, at the top of its step.
    fn slowest(&self, part: u64, whole: u64) -> f64 {
        let from_top = (self.count * part).div_ceil(whole).max(1);
        let mut seen = 0;
        for (step, count) in self.buckets.iter().enumerate().rev() {
            seen += u64::from(*count);
            if seen >= from_top {
                return (step as u64 * self.step_us + self.step_us) as f64 / 1000.0;
            }
        }
        0.0
    }
}

/// One game's frames as they reached the screen.
#[derive(Debug, Clone)]
pub struct Frames {
    gaps: Histogram,
    shown: u64,
    last: Option<Duration>,
    total: Duration,
}

impl Default for Frames {
    fn default() -> Self {
        Frames {
            gaps: Histogram::new(STEP_US, BUCKETS),
            shown: 0,
            last: None,
            total: Duration::ZERO,
        }
    }
}

impl Frames {
    /// A frame of the game reached the screen at `at` (a monotonic time).
    /// A time before the last one is counted as shown with no gap.
    pub fn shown(&mut self, at: Duration) {
        self.shown += 1;
        if let Some(last) = self.last.replace(at) {
            let gap = at.saturating_sub(last);
            self.total += gap;
            self.gaps.add(gap);
        }
    }

    /// The game's record, or none when it showed fewer than
    /// [`LEAST_FRAMES`] frames.
    pub fn record(&self) -> Option<Record> {
        let gaps = self.shown.saturating_sub(1);
        if self.shown < LEAST_FRAMES || self.total.is_zero() {
            return None;
        }
        let seconds = self.total.as_secs_f64();
        Some(Record {
            frames: self.shown,
            seconds: round(seconds, 1),
            average_fps: round(gaps as f64 / seconds, 1),
            slowest_1_percent_ms: round(self.gaps.slowest(1, 100), 1),
            slowest_0_1_percent_ms: round(self.gaps.slowest(1, 1000), 1),
        })
    }
}

/// How long the compositor took to draw each frame of a game's screen
/// (M7.7e): our own work, apart from the game's pace and the machine's.
/// CI's smoothness gate reads its slowest 1 percent.
#[derive(Debug, Clone)]
pub struct Work {
    took: Histogram,
}

impl Default for Work {
    fn default() -> Self {
        Work {
            took: Histogram::new(WORK_STEP_US, WORK_BUCKETS),
        }
    }
}

impl Work {
    /// The compositor drew one frame in `took`.
    pub fn drawn(&mut self, took: Duration) {
        self.took.add(took);
    }

    /// How many frames were drawn.
    pub fn frames(&self) -> u64 {
        self.took.count
    }

    /// The time the median frame took, in ms, to 10 microseconds.
    pub fn median_ms(&self) -> f64 {
        round(self.took.slowest(1, 2), 2)
    }

    /// The time the slowest 1 percent of frames reach, in ms.
    pub fn slowest_1_percent_ms(&self) -> f64 {
        round(self.took.slowest(1, 100), 2)
    }
}

/// `value` to `places` decimals, so the file reads plainly.
fn round(value: f64, places: i32) -> f64 {
    let scale = 10f64.powi(places);
    (value * scale).round() / scale
}

/// One game's smoothness, the last time it played.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    /// The frames that reached the screen.
    pub frames: u64,
    /// How long it played, from the first frame to the last.
    pub seconds: f64,
    pub average_fps: f64,
    /// The time between frames that the slowest 1 percent reach or pass.
    pub slowest_1_percent_ms: f64,
    /// The same for the slowest 0.1 percent.
    pub slowest_0_1_percent_ms: f64,
}

/// The games file's format.
const FORMAT: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
struct File {
    format: u32,
    #[serde(default)]
    apps: BTreeMap<String, Record>,
}

/// The records a games file holds, by app; none when it does not parse or
/// has a format this release does not know, as a lenient reader keeps it
/// (ADR-008).
pub fn read(text: &str) -> BTreeMap<String, Record> {
    toml::from_str::<File>(text)
        .ok()
        .filter(|f| f.format == FORMAT)
        .map(|f| f.apps)
        .unwrap_or_default()
}

/// The games file `text` with `app`'s record replaced by `record`; a file
/// that does not parse starts anew.
pub fn with(text: &str, app: &str, record: Record) -> String {
    let mut apps = read(text);
    apps.insert(app.to_string(), record);
    toml::to_string(&File {
        format: FORMAT,
        apps,
    })
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: f64) -> Duration {
        Duration::from_secs_f64(n / 1000.0)
    }

    /// `n` frames `gap` ms apart, from 1 s.
    fn steady(frames: &mut Frames, n: u64, gap: f64, from: Duration) -> Duration {
        let mut at = from;
        for _ in 0..n {
            frames.shown(at);
            at += ms(gap);
        }
        at - ms(gap)
    }

    #[test]
    fn a_steady_game_has_its_rate_and_no_slow_frames() {
        let mut frames = Frames::default();
        steady(&mut frames, 601, 16.0, Duration::from_secs(1));
        let record = frames.record().unwrap();
        assert_eq!(record.frames, 601);
        assert_eq!(record.seconds, 9.6);
        assert_eq!(record.average_fps, 62.5);
        assert_eq!(record.slowest_1_percent_ms, 16.1);
        assert_eq!(record.slowest_0_1_percent_ms, 16.1);
    }

    #[test]
    fn the_slowest_frames_are_the_ones_one_in_a_hundred_reach() {
        let mut frames = Frames::default();
        // 990 frames 10 ms apart, then 9 of 30 ms and 1 of 100 ms.
        let mut at = steady(&mut frames, 991, 10.0, Duration::ZERO);
        for _ in 0..9 {
            at += ms(30.0);
            frames.shown(at);
        }
        frames.shown(at + ms(100.0));
        let record = frames.record().unwrap();
        assert_eq!(record.frames, 1001);
        assert_eq!(record.slowest_1_percent_ms, 30.1);
        assert_eq!(record.slowest_0_1_percent_ms, 100.1);
    }

    #[test]
    fn a_moment_of_fullscreen_is_no_game() {
        let mut frames = Frames::default();
        steady(&mut frames, LEAST_FRAMES - 1, 16.0, Duration::ZERO);
        assert_eq!(frames.record(), None);
        frames.shown(Duration::from_secs(5));
        assert!(frames.record().is_some());
    }

    #[test]
    fn a_gap_of_a_second_or_more_counts_in_the_last_step() {
        let mut frames = Frames::default();
        let at = steady(&mut frames, 100, 16.0, Duration::ZERO);
        frames.shown(at + Duration::from_secs(5));
        let record = frames.record().unwrap();
        assert_eq!(record.slowest_0_1_percent_ms, 1000.0);
    }

    #[test]
    fn our_work_counts_each_frame_to_ten_microseconds() {
        let mut work = Work::default();
        assert_eq!(work.slowest_1_percent_ms(), 0.0);
        // 990 frames drawn in 0.8 ms, 9 in 3 ms and 1 in 250 ms.
        for _ in 0..990 {
            work.drawn(ms(0.8));
        }
        for _ in 0..9 {
            work.drawn(ms(3.0));
        }
        work.drawn(ms(250.0));
        assert_eq!(work.frames(), 1000);
        assert_eq!(work.median_ms(), 0.81);
        assert_eq!(work.slowest_1_percent_ms(), 3.01);
    }

    #[test]
    fn each_app_keeps_its_last_record_and_a_bad_file_starts_anew() {
        let one = Record {
            frames: 100,
            seconds: 2.0,
            average_fps: 50.0,
            slowest_1_percent_ms: 25.0,
            slowest_0_1_percent_ms: 30.0,
        };
        let two = Record {
            frames: 200,
            ..one.clone()
        };
        let text = with("", "game.one", one.clone());
        let text = with(&text, "game.two", one.clone());
        let text = with(&text, "game.one", two.clone());
        let apps = read(&text);
        assert_eq!(apps.len(), 2);
        assert_eq!(apps["game.one"], two);
        assert_eq!(apps["game.two"], one);
        let fresh = read(&with("not = [toml", "game.one", one.clone()));
        assert_eq!(fresh.len(), 1);
    }
}
