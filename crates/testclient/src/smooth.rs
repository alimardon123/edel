//! `--smooth N` with `--busy` (roadmap M7.7e): the window asks
//! `wp_presentation` for feedback on every frame it commits, and once it
//! has N intervals between presented frames, it prints one line
//! (`smooth frames=N median_ms=... p99_ms=... slowest_ms=... direct=...
//! discarded=...`)
//! and exits. A presented frame that had the zero-copy flag went straight
//! to the screen (`direct`). A discarded frame adds no interval, so the
//! next presented frame's interval spans the gap and counts as slow, which
//! is right. The first presented frame gives no interval, so N intervals
//! need N+1 presented frames. The client asks for feedback only after the
//! first frame callback, so the window's start, its fullscreen configure
//! and game mode's switch are not counted, and the intervals of the first
//! `SETTLE` after the first presented frame are kept apart: the window's
//! open animation fades it in over the first 100 ms or so, which happens
//! once when a game starts and is not a stutter while it plays, yet on a
//! runner drawing a frame in 8 ms those are the 12 slowest of 1200, its
//! whole p99. The line says how many there were and the slowest of them
//! (`settle_frames=... settle_slowest_ms=...`), so a slow start still
//! shows. CI's smoothness gate reads this line.

use std::time::Duration;

use anyhow::{Context, Result};
use smithay_client_toolkit::reexports::client::globals::GlobalList;
use smithay_client_toolkit::reexports::client::protocol::wl_surface::WlSurface;
use smithay_client_toolkit::reexports::client::{Connection, Dispatch, QueueHandle};
use smithay_client_toolkit::reexports::protocols::wp::presentation_time::client::{
    wp_presentation::{self, WpPresentation},
    wp_presentation_feedback::{self, Kind, WpPresentationFeedback},
};

use crate::Client;

/// How long after its first presented frame the window is still starting.
const SETTLE: Duration = Duration::from_millis(500);

/// The presentation feedback `--smooth` asks for, and what it measured.
pub struct Smooth {
    presentation: WpPresentation,
    frames: Frames,
}

/// The intervals between presented frames, and how they were presented.
struct Frames {
    want: usize,
    first: Option<Duration>,
    last: Option<Duration>,
    settling: Vec<Duration>,
    intervals: Vec<Duration>,
    direct: u32,
    discarded: u32,
}

/// Binds `wp_presentation`, which `--smooth` needs to see when frames reach
/// the screen, and measures `want` intervals.
pub fn bind(globals: &GlobalList, qh: &QueueHandle<Client>, want: usize) -> Result<Smooth> {
    let presentation: WpPresentation = globals.bind(qh, 1..=1, ()).context("no wp_presentation")?;
    Ok(Smooth {
        presentation,
        frames: Frames::new(want),
    })
}

impl Smooth {
    /// Asks for feedback on the commit that is about to show `surface`.
    pub fn ask(&self, surface: &WlSurface, qh: &QueueHandle<Client>) {
        self.presentation.feedback(surface, qh, ());
    }

    /// How many intervals have been measured so far.
    pub fn seen(&self) -> usize {
        self.frames.intervals.len()
    }

    pub fn done(&self) -> bool {
        self.frames.done()
    }

    /// The line `--smooth` prints when it is done.
    pub fn line(&self) -> String {
        self.frames.line()
    }
}

impl Frames {
    fn new(want: usize) -> Frames {
        Frames {
            want,
            first: None,
            last: None,
            settling: Vec::new(),
            intervals: Vec::new(),
            direct: 0,
            discarded: 0,
        }
    }

    fn presented(&mut self, at: Duration, direct: bool) {
        let first = *self.first.get_or_insert(at);
        if let Some(prev) = self.last {
            let interval = at.saturating_sub(prev);
            if at < first + SETTLE {
                self.settling.push(interval);
            } else {
                self.intervals.push(interval);
            }
        }
        self.last = Some(at);
        if direct {
            self.direct += 1;
        }
    }

    fn discarded(&mut self) {
        self.discarded += 1;
    }

    fn done(&self) -> bool {
        self.intervals.len() >= self.want
    }

    fn line(&self) -> String {
        let (median, p99, slowest) = summary(&self.intervals);
        let (_, _, settle_slowest) = summary(&self.settling);
        format(
            self.intervals.len(),
            median,
            p99,
            slowest,
            self.direct,
            self.discarded,
        ) + &format!(
            " settle_frames={} settle_slowest_ms={:.1}",
            self.settling.len(),
            settle_slowest.as_secs_f64() * 1000.0
        )
    }
}

/// The median and the 99th percentile, each by nearest rank (the smallest
/// value with at least half, or 99 percent, of the values at or below
/// it), and the largest value. The median is the pace the machine draws
/// at, so the gate can weigh the slow frames against it. All are zero for
/// no values.
pub fn summary(intervals: &[Duration]) -> (Duration, Duration, Duration) {
    let mut sorted = intervals.to_vec();
    sorted.sort();
    let Some(&slowest) = sorted.last() else {
        return (Duration::ZERO, Duration::ZERO, Duration::ZERO);
    };
    let at = |percent: usize| sorted[(sorted.len() * percent).div_ceil(100) - 1];
    (at(50), at(99), slowest)
}

fn format(
    frames: usize,
    median: Duration,
    p99: Duration,
    slowest: Duration,
    direct: u32,
    discarded: u32,
) -> String {
    format!(
        "smooth frames={frames} median_ms={:.1} p99_ms={:.1} slowest_ms={:.1} direct={direct} discarded={discarded}",
        median.as_secs_f64() * 1000.0,
        p99.as_secs_f64() * 1000.0,
        slowest.as_secs_f64() * 1000.0,
    )
}

impl Dispatch<WpPresentation, ()> for Client {
    /// The clock the times are on, which the client does not need.
    fn event(
        _: &mut Self,
        _: &WpPresentation,
        _: wp_presentation::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WpPresentationFeedback, ()> for Client {
    fn event(
        client: &mut Self,
        _: &WpPresentationFeedback,
        event: wp_presentation_feedback::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wp_presentation_feedback::Event::Presented {
                tv_sec_hi,
                tv_sec_lo,
                tv_nsec,
                flags,
                ..
            } => {
                let secs = (u64::from(tv_sec_hi) << 32) | u64::from(tv_sec_lo);
                let at = Duration::new(secs, tv_nsec);
                let direct = flags
                    .into_result()
                    .is_ok_and(|kind| kind.contains(Kind::ZeroCopy));
                if let Some(smooth) = client.smooth.as_mut() {
                    smooth.frames.presented(at, direct);
                }
            }
            wp_presentation_feedback::Event::Discarded => {
                if let Some(smooth) = client.smooth.as_mut() {
                    smooth.frames.discarded();
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_of_a_hundred_values() {
        let values: Vec<Duration> = (1..=100).map(Duration::from_millis).collect();
        assert_eq!(
            summary(&values),
            (
                Duration::from_millis(50),
                Duration::from_millis(99),
                Duration::from_millis(100)
            )
        );
    }

    #[test]
    fn summary_of_a_slow_frame() {
        let values: Vec<Duration> = [16, 16, 16, 50]
            .into_iter()
            .map(Duration::from_millis)
            .collect();
        assert_eq!(
            summary(&values),
            (
                Duration::from_millis(16),
                Duration::from_millis(50),
                Duration::from_millis(50)
            )
        );
    }

    #[test]
    fn summary_of_nothing_is_zero() {
        assert_eq!(
            summary(&[]),
            (Duration::ZERO, Duration::ZERO, Duration::ZERO)
        );
    }

    #[test]
    fn the_first_half_second_is_kept_apart() {
        let mut smooth = Frames::new(3);
        for ms in [0, 40, 80, 520, 528, 536] {
            smooth.presented(Duration::from_millis(ms), false);
        }
        assert_eq!(smooth.settling.len(), 2);
        assert!(smooth.done());
        assert!(smooth
            .line()
            .ends_with("median_ms=8.0 p99_ms=440.0 slowest_ms=440.0 direct=0 discarded=0 settle_frames=2 settle_slowest_ms=40.0"));
    }

    #[test]
    fn the_line_has_one_decimal_and_the_counts() {
        assert_eq!(
            format(
                300,
                Duration::from_micros(16_660),
                Duration::from_micros(16_660),
                Duration::from_micros(33_400),
                0,
                2
            ),
            "smooth frames=300 median_ms=16.7 p99_ms=16.7 slowest_ms=33.4 direct=0 discarded=2"
        );
    }
}
