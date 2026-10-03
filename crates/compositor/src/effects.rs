//! Effect tiers (roadmap M5.11, ADR-002 section 5): how much the
//! compositor animates and decorates, picked from what the machine can
//! do. Full has every effect, Balanced the cheap ones, Lite none. The
//! starting tier comes from the GPU (a software renderer starts at Lite)
//! and the battery (unplugged, at most Balanced); then [`Deadline`] drops
//! one tier whenever frames keep missing the screen's refresh, so the
//! desktop always reacts on the next frame, on old hardware too.

use std::fmt;
use std::time::Duration;

/// The effect tiers, from the cheapest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    Lite,
    Balanced,
    Full,
}

impl Tier {
    pub fn name(self) -> &'static str {
        match self {
            Tier::Lite => "lite",
            Tier::Balanced => "balanced",
            Tier::Full => "full",
        }
    }

    /// `lite`, `balanced` or `full`.
    pub fn parse(name: &str) -> Option<Tier> {
        match name {
            "lite" => Some(Tier::Lite),
            "balanced" => Some(Tier::Balanced),
            "full" => Some(Tier::Full),
            _ => None,
        }
    }

    /// The tier below, if any.
    pub fn lower(self) -> Option<Tier> {
        match self {
            Tier::Full => Some(Tier::Balanced),
            Tier::Balanced => Some(Tier::Lite),
            Tier::Lite => None,
        }
    }
}

impl fmt::Display for Tier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Names a GL renderer gives when the CPU draws: llvmpipe, softpipe,
/// swrast and the like, with nothing to spare for effects.
const SOFTWARE: [&str; 4] = ["llvmpipe", "softpipe", "swrast", "software rasterizer"];

/// The tier a session starts at: Lite on a software renderer, at most
/// Balanced on battery, else Full.
pub fn starting_tier(renderer: &str, on_battery: bool) -> Tier {
    let renderer = renderer.to_lowercase();
    if SOFTWARE.iter().any(|s| renderer.contains(s)) {
        Tier::Lite
    } else if on_battery {
        Tier::Balanced
    } else {
        Tier::Full
    }
}

/// A frame that took longer than this share of the refresh interval to
/// draw is counted as missing it: the rest is the display's.
const MISS_SHARE: f64 = 0.75;
/// Frames looked at together.
pub const WINDOW: usize = 60;
/// Missed frames among the last [`WINDOW`] that drop a tier.
pub const MISSES: usize = 5;

/// Watches frame times and drops one tier when [`MISSES`] of the last
/// [`WINDOW`] frames missed the screen's refresh. After a drop it starts
/// counting afresh, so one slow moment drops one tier, not all of them;
/// it never raises the tier again within a session.
#[derive(Debug, Clone)]
pub struct Deadline {
    tier: Tier,
    /// The last frames, oldest first: whether each missed.
    recent: std::collections::VecDeque<bool>,
}

impl Deadline {
    pub fn new(tier: Tier) -> Deadline {
        Deadline {
            tier,
            recent: std::collections::VecDeque::with_capacity(WINDOW),
        }
    }

    pub fn tier(&self) -> Tier {
        self.tier
    }

    /// One frame drawn in `took`, on a screen refreshing every `interval`;
    /// returns the new tier when this frame dropped it.
    pub fn frame(&mut self, took: Duration, interval: Duration) -> Option<Tier> {
        let missed = took.as_secs_f64() > interval.as_secs_f64() * MISS_SHARE;
        if self.recent.len() == WINDOW {
            self.recent.pop_front();
        }
        self.recent.push_back(missed);
        if self.recent.iter().filter(|m| **m).count() < MISSES {
            return None;
        }
        let lower = self.tier.lower()?;
        self.tier = lower;
        self.recent.clear();
        Some(lower)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HZ60: Duration = Duration::from_micros(16_667);

    #[test]
    fn the_gpu_and_the_battery_pick_the_starting_tier() {
        assert_eq!(
            starting_tier("llvmpipe (LLVM 22.1.3, 256 bits)", false),
            Tier::Lite
        );
        assert_eq!(starting_tier("Software Rasterizer", false), Tier::Lite);
        assert_eq!(
            starting_tier("Mesa Intel(R) UHD Graphics 620 (KBL GT2)", false),
            Tier::Full
        );
        assert_eq!(
            starting_tier("AMD Radeon Graphics (radeonsi, renoir)", true),
            Tier::Balanced
        );
        // A software renderer stays Lite on mains power too.
        assert_eq!(starting_tier("LLVMPIPE", true), Tier::Lite);
    }

    #[test]
    fn tiers_name_and_order_themselves() {
        for tier in [Tier::Lite, Tier::Balanced, Tier::Full] {
            assert_eq!(Tier::parse(tier.name()), Some(tier));
        }
        assert_eq!(Tier::parse("ultra"), None);
        assert!(Tier::Lite < Tier::Balanced && Tier::Balanced < Tier::Full);
        assert_eq!(Tier::Full.lower(), Some(Tier::Balanced));
        assert_eq!(Tier::Lite.lower(), None);
    }

    #[test]
    fn frames_on_time_never_drop_the_tier() {
        let mut deadline = Deadline::new(Tier::Full);
        for _ in 0..1000 {
            assert_eq!(deadline.frame(Duration::from_millis(4), HZ60), None);
        }
        assert_eq!(deadline.tier(), Tier::Full);
    }

    #[test]
    fn a_few_missed_frames_among_many_are_forgiven() {
        let mut deadline = Deadline::new(Tier::Full);
        // Four slow frames in every sixty: under the threshold for ever.
        for i in 0..600 {
            let took = if i % 15 == 0 { 20 } else { 4 };
            assert_eq!(deadline.frame(Duration::from_millis(took), HZ60), None);
        }
    }

    #[test]
    fn missed_frames_drop_one_tier_then_count_afresh() {
        let mut deadline = Deadline::new(Tier::Full);
        let slow = Duration::from_millis(14);
        let drops: Vec<_> = (0..MISSES * 3)
            .filter_map(|_| deadline.frame(slow, HZ60))
            .collect();
        // Five slow frames drop to Balanced, five more to Lite, then no
        // lower tier is left.
        assert_eq!(drops, [Tier::Balanced, Tier::Lite]);
        assert_eq!(deadline.tier(), Tier::Lite);
    }

    #[test]
    fn a_faster_screen_has_a_tighter_deadline() {
        let hz144 = Duration::from_micros(6_944);
        let mut deadline = Deadline::new(Tier::Full);
        let took = Duration::from_millis(6);
        let drop = (0..MISSES).find_map(|_| deadline.frame(took, hz144));
        assert_eq!(drop, Some(Tier::Balanced), "6 ms misses at 144 Hz");
        let mut at60 = Deadline::new(Tier::Full);
        assert!(
            (0..100).all(|_| at60.frame(took, HZ60).is_none()),
            "but not at 60 Hz"
        );
    }
}
