//! The game's last frames, for the frame rate overlay (roadmap M8.27),
//! pure and tested. While a game plays, each of its frames that reaches
//! the screen is noted here as well as in the whole game's record
//! (`edel::smoothness::Frames`, M7.7d): the last [`KEPT`] seconds of
//! frame times, from which the overlay reads the frame rate over the last
//! second, its average frame time, the slowest 1 percent of the kept
//! frames and the times of the last [`GRAPHED`] frames for its graph.
//! Nothing here reads a clock: the newest frame's time is "now".

use std::collections::VecDeque;
use std::time::Duration;

/// How long frames are kept: the slowest 1 percent is of these.
pub const KEPT: Duration = Duration::from_secs(5);
/// The most frames kept, whatever the rate, so memory stays fixed.
const MOST: usize = 2048;
/// How many frames the graph shows.
pub const GRAPHED: usize = 120;

/// The last frames of a game: when each reached the screen and how long
/// after the one before.
#[derive(Debug, Clone, Default)]
pub struct Recent {
    /// Each frame's time and the gap before it, oldest first.
    frames: VecDeque<(Duration, Duration)>,
    last: Option<Duration>,
}

/// What the overlay shows.
#[derive(Debug, Clone, PartialEq)]
pub struct Shown {
    /// Frames that reached the screen in the last second.
    pub fps: u32,
    /// Their average frame time, in milliseconds.
    pub frame_ms: f32,
    /// The frame time the slowest 1 percent of the kept frames reach.
    pub slowest_1_percent_ms: f32,
    /// The last frames' times, oldest first, in milliseconds.
    pub graph: Vec<f32>,
}

impl Recent {
    /// A frame reached the screen at `at`, a monotonic time; one before the
    /// last counts as shown with no gap.
    pub fn shown(&mut self, at: Duration) {
        if let Some(last) = self.last.replace(at.max(self.last.unwrap_or(at))) {
            self.frames
                .push_back((at.max(last), at.saturating_sub(last)));
        }
        let newest = self.last.unwrap_or(at);
        while self
            .frames
            .front()
            .is_some_and(|(when, _)| newest.saturating_sub(*when) > KEPT)
            || self.frames.len() > MOST
        {
            self.frames.pop_front();
        }
    }

    /// What the overlay shows now, or none before two frames were shown.
    pub fn shown_now(&self) -> Option<Shown> {
        let newest = self.last?;
        if self.frames.is_empty() {
            return None;
        }
        let second: Vec<Duration> = self
            .frames
            .iter()
            .filter(|(when, _)| newest.saturating_sub(*when) < Duration::from_secs(1))
            .map(|(_, gap)| *gap)
            .collect();
        let fps = second.len() as u32;
        let frame_ms = if second.is_empty() {
            0.0
        } else {
            ms(second.iter().sum::<Duration>()) / second.len() as f32
        };
        let mut gaps: Vec<Duration> = self.frames.iter().map(|(_, gap)| *gap).collect();
        gaps.sort_unstable();
        // The nearest rank from the slow end, as `edel::smoothness` takes it.
        let from_top = gaps.len().div_ceil(100).max(1);
        let slowest_1_percent_ms = ms(gaps[gaps.len() - from_top]);
        let graph = self
            .frames
            .iter()
            .skip(self.frames.len().saturating_sub(GRAPHED))
            .map(|(_, gap)| ms(*gap))
            .collect();
        Some(Shown {
            fps,
            frame_ms,
            slowest_1_percent_ms,
            graph,
        })
    }
}

fn ms(d: Duration) -> f32 {
    d.as_secs_f32() * 1000.0
}

impl Shown {
    /// The overlay's lines: the frame rate, then the average frame time
    /// and the slowest 1 percent, each to a tenth of a millisecond. Units
    /// and numbers only, read alike in every language.
    pub fn lines(&self) -> (String, String) {
        (
            format!("{} fps", self.fps),
            format!(
                "{:.1} ms   1% {:.1} ms",
                self.frame_ms, self.slowest_1_percent_ms
            ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(ms: u64) -> Duration {
        Duration::from_millis(ms)
    }

    #[test]
    fn nothing_shows_before_two_frames() {
        let mut recent = Recent::default();
        assert_eq!(recent.shown_now(), None);
        recent.shown(at(10));
        assert_eq!(recent.shown_now(), None);
        recent.shown(at(27));
        assert!(recent.shown_now().is_some());
    }

    #[test]
    fn sixty_even_frames_a_second_read_sixty_fps() {
        let mut recent = Recent::default();
        for i in 0..=180 {
            recent.shown(Duration::from_micros(i * 16_667));
        }
        let shown = recent.shown_now().unwrap();
        assert_eq!(shown.fps, 60);
        assert!((shown.frame_ms - 16.667).abs() < 0.01, "{}", shown.frame_ms);
        assert!((shown.slowest_1_percent_ms - 16.667).abs() < 0.01);
        assert_eq!(shown.graph.len(), GRAPHED);
        assert_eq!(
            shown.lines(),
            ("60 fps".into(), "16.7 ms   1% 16.7 ms".into())
        );
    }

    #[test]
    fn a_stutter_shows_in_the_slowest_one_percent_but_not_the_average() {
        let mut recent = Recent::default();
        let mut t = 0;
        for i in 0..300 {
            t += if i == 150 { 50 } else { 10 };
            recent.shown(at(t));
        }
        let shown = recent.shown_now().unwrap();
        // 299 gaps: 1 percent from the top is the 3rd slowest, still 10 ms
        // with one stutter; with four of 303 gaps it is the 4th, 50 ms.
        assert_eq!(shown.slowest_1_percent_ms, 10.0);
        assert!((shown.frame_ms - 10.0).abs() < 0.01);
        for _ in 0..3 {
            t += 50;
            recent.shown(at(t));
        }
        assert_eq!(recent.shown_now().unwrap().slowest_1_percent_ms, 50.0);
    }

    #[test]
    fn only_the_last_five_seconds_are_kept() {
        let mut recent = Recent::default();
        recent.shown(at(0));
        recent.shown(at(500));
        for i in 1..=600 {
            recent.shown(at(500 + i * 10));
        }
        // The 500 ms gap is more than five seconds old now.
        let shown = recent.shown_now().unwrap();
        assert_eq!(shown.slowest_1_percent_ms, 10.0);
        assert!(recent.frames.len() <= 501);
    }

    #[test]
    fn a_time_before_the_last_counts_as_no_gap() {
        let mut recent = Recent::default();
        recent.shown(at(100));
        recent.shown(at(90));
        let shown = recent.shown_now().unwrap();
        assert_eq!(shown.graph, vec![0.0]);
    }

    #[test]
    fn memory_stays_fixed_at_any_rate() {
        let mut recent = Recent::default();
        for i in 0..10_000 {
            recent.shown(Duration::from_micros(i * 100));
        }
        assert!(recent.frames.len() <= MOST);
    }
}
