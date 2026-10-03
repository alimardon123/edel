//! Window animations (roadmap M5.11b, ADR-002 section 5): how long and how
//! far windows animate when they open, close and move, by effect tier and
//! the person's `appearance.motion`. Every animation is short and tied to
//! the display clock: its progress is the time since it began over its
//! length, read when a frame is drawn, so a late frame skips ahead and
//! never makes it run long. A slide that interrupts a slide starts from
//! where the window is drawn, so nothing jumps.

use std::time::Duration;

use smithay::utils::{Logical, Point};

use crate::effects::Tier;

/// `appearance.motion`: everything (the default), fades only (nothing
/// grows, shrinks or slides), or nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Motion {
    #[default]
    Full,
    Reduced,
    Off,
}

impl Motion {
    /// `full`, `reduced` or `off`.
    pub fn parse(name: &str) -> Option<Motion> {
        match name {
            "full" => Some(Motion::Full),
            "reduced" => Some(Motion::Reduced),
            "off" => Some(Motion::Off),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Motion::Full => "full",
            Motion::Reduced => "reduced",
            Motion::Off => "off",
        }
    }
}

/// How windows animate at one tier and motion.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    /// How long a window takes to fade in, and grow if `zoom` is under 1.
    pub open: Duration,
    /// How long a closed window takes to fade out, and shrink.
    pub close: Duration,
    /// How long a window takes to slide to a new place; zero jumps.
    pub slide: Duration,
    /// The share of its size an opening window grows from and a closing
    /// one shrinks to; 1 neither grows nor shrinks.
    pub zoom: f64,
}

/// The style for `tier` and `motion`: Full grows windows from 90 percent
/// over 200 ms, Balanced from 95 percent over 150 ms, Lite only fades,
/// over 100 ms (ADR-002: "flat surfaces, short fades"); closing takes
/// three quarters as long. Reduced motion keeps the fades and drops the
/// rest; off drops everything.
pub fn style(tier: Tier, motion: Motion) -> Style {
    let (open, zoom, slide) = match tier {
        Tier::Full => (200, 0.9, 200),
        Tier::Balanced => (150, 0.95, 150),
        Tier::Lite => (100, 1.0, 0),
    };
    let ms = Duration::from_millis;
    match motion {
        Motion::Full => Style {
            open: ms(open),
            close: ms(open * 3 / 4),
            slide: ms(slide),
            zoom,
        },
        Motion::Reduced => Style {
            open: ms(open),
            close: ms(open * 3 / 4),
            slide: Duration::ZERO,
            zoom: 1.0,
        },
        Motion::Off => Style {
            open: Duration::ZERO,
            close: Duration::ZERO,
            slide: Duration::ZERO,
            zoom: 1.0,
        },
    }
}

/// One animation on the clock frames are drawn by: when it began and how
/// long it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Anim {
    pub start: Duration,
    pub length: Duration,
}

impl Anim {
    pub fn new(start: Duration, length: Duration) -> Anim {
        Anim { start, length }
    }

    /// Whether it has ended by `now`.
    pub fn done(&self, now: Duration) -> bool {
        now >= self.start + self.length
    }

    /// How far along it is at `now`, eased out: 0 at the start, 1 at the
    /// end and after; quick at first and settling gently, as things that
    /// stop do.
    pub fn eased(&self, now: Duration) -> f64 {
        if self.length.is_zero() {
            return 1.0;
        }
        let t = (now.saturating_sub(self.start).as_secs_f64() / self.length.as_secs_f64())
            .clamp(0.0, 1.0);
        1.0 - (1.0 - t).powi(3)
    }
}

/// An opening window `eased` of the way in: its alpha and its size as a
/// share of its own.
pub fn opening(style: &Style, eased: f64) -> (f32, f64) {
    (eased as f32, style.zoom + (1.0 - style.zoom) * eased)
}

/// A closed window `eased` of the way out: its alpha and its size as a
/// share of its own.
pub fn closing(style: &Style, eased: f64) -> (f32, f64) {
    ((1.0 - eased) as f32, 1.0 - (1.0 - style.zoom) * eased)
}

/// A window drawn away from its place by an offset that shrinks to
/// nothing: a person sees it slide from where it was to where it is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Slide {
    pub anim: Anim,
    pub offset: Point<f64, Logical>,
}

impl Slide {
    /// The slide of a window moved by the layout from `from` to `to` at
    /// `now`; one that was still sliding starts from where it is drawn.
    /// None when there is nothing to slide or no time to do it in.
    pub fn start(
        previous: Option<&Slide>,
        from: Point<i32, Logical>,
        to: Point<i32, Logical>,
        now: Duration,
        length: Duration,
    ) -> Option<Slide> {
        if length.is_zero() {
            return None;
        }
        let carried = previous.map_or_else(Point::default, |s| s.offset_at(now));
        let offset = (from - to).to_f64() + carried;
        if offset.x.abs() < 0.5 && offset.y.abs() < 0.5 {
            return None;
        }
        Some(Slide {
            anim: Anim::new(now, length),
            offset,
        })
    }

    /// How far from its place the window is drawn at `now`.
    pub fn offset_at(&self, now: Duration) -> Point<f64, Logical> {
        self.offset.upscale(1.0 - self.anim.eased(now))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: fn(u64) -> Duration = Duration::from_millis;

    #[test]
    fn motion_names_parse_back() {
        for motion in [Motion::Full, Motion::Reduced, Motion::Off] {
            assert_eq!(Motion::parse(motion.name()), Some(motion));
        }
        assert_eq!(Motion::parse("fast"), None);
        assert_eq!(Motion::default(), Motion::Full);
    }

    #[test]
    fn each_tier_animates_less_than_the_one_above() {
        let full = style(Tier::Full, Motion::Full);
        let balanced = style(Tier::Balanced, Motion::Full);
        let lite = style(Tier::Lite, Motion::Full);
        assert!(full.open > balanced.open && balanced.open > lite.open);
        assert!(full.zoom < balanced.zoom && balanced.zoom < lite.zoom);
        // Lite only fades: nothing grows or slides.
        assert_eq!((lite.zoom, lite.slide), (1.0, Duration::ZERO));
        assert_eq!(full.close, MS(150));
    }

    #[test]
    fn reduced_motion_keeps_fades_only_and_off_keeps_nothing() {
        for tier in [Tier::Lite, Tier::Balanced, Tier::Full] {
            let reduced = style(tier, Motion::Reduced);
            assert_eq!(reduced.open, style(tier, Motion::Full).open);
            assert_eq!((reduced.zoom, reduced.slide), (1.0, Duration::ZERO));
            let off = style(tier, Motion::Off);
            assert!(off.open.is_zero() && off.close.is_zero() && off.slide.is_zero());
        }
    }

    #[test]
    fn progress_eases_out_and_ends_on_time() {
        let anim = Anim::new(MS(1000), MS(200));
        assert_eq!(anim.eased(MS(900)), 0.0, "before it starts");
        assert_eq!(anim.eased(MS(1000)), 0.0);
        let half = anim.eased(MS(1100));
        assert!(half > 0.5 && half < 1.0, "quicker than linear: {half}");
        assert_eq!(anim.eased(MS(1200)), 1.0);
        assert_eq!(anim.eased(MS(5000)), 1.0, "a late frame skips to the end");
        assert!(!anim.done(MS(1199)) && anim.done(MS(1200)));
        assert_eq!(Anim::new(MS(0), Duration::ZERO).eased(MS(0)), 1.0);
    }

    #[test]
    fn windows_fade_and_grow_in_and_out() {
        let full = style(Tier::Full, Motion::Full);
        assert_eq!(opening(&full, 0.0), (0.0, 0.9));
        assert_eq!(opening(&full, 1.0), (1.0, 1.0));
        assert_eq!(closing(&full, 0.0), (1.0, 1.0));
        let (alpha, zoom) = closing(&full, 1.0);
        assert_eq!(alpha, 0.0);
        assert!((zoom - 0.9).abs() < 1e-9);
        let lite = style(Tier::Lite, Motion::Full);
        assert_eq!(opening(&lite, 0.5).1, 1.0, "Lite never grows");
    }

    #[test]
    fn a_slide_ends_at_the_place_and_an_interrupted_one_never_jumps() {
        let a = Point::from((0, 0));
        let b = Point::from((100, 0));
        let c = Point::from((100, 200));
        let first = Slide::start(None, a, b, MS(0), MS(200)).expect("it moved");
        assert_eq!(first.offset_at(MS(0)), Point::from((-100.0, 0.0)));
        assert_eq!(first.offset_at(MS(200)), Point::from((0.0, 0.0)));
        // Halfway there, the layout moves it again, to c.
        let drawn = b.to_f64() + first.offset_at(MS(100));
        let second = Slide::start(Some(&first), b, c, MS(100), MS(200)).expect("it moved");
        assert_eq!(c.to_f64() + second.offset_at(MS(100)), drawn);
        assert_eq!(second.offset_at(MS(300)), Point::from((0.0, 0.0)));
        // Nothing to slide, or no time to slide in.
        assert_eq!(Slide::start(None, b, b, MS(0), MS(200)), None);
        assert_eq!(Slide::start(None, a, b, MS(0), Duration::ZERO), None);
    }
}
