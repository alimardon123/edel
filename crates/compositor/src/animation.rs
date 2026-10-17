//! Window animations (roadmap M5.11b, ADR-002 section 5): how long and how
//! far windows animate when they open, close and move, by effect tier and
//! the person's `appearance.animations`, how fast (`appearance.animation_speed`,
//! M5.12b4) and with what feel (`appearance.animation_curve`). Opening,
//! closing and popups run on the eased clock: their progress is the time since
//! they began over their length, read when a frame is drawn, so a late frame
//! skips ahead and never makes it run long. Slides and workspace switches run
//! on springs (M5.14) that carry their position and speed when interrupted, so
//! nothing jumps. Fades and growing keep the eased curves.

use std::f64::consts::PI;
use std::time::Duration;

use smithay::utils::{Logical, Point};

use crate::effects::Tier;

/// `appearance.animations`: everything (the default), fades only (nothing
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

/// `appearance.animation_curve` (M5.12b4): how an animation eases, one curve
/// for every kind of animation (windows and popups alike).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Curve {
    /// Eases out cubically, as windows have always settled.
    #[default]
    Smooth,
    /// Eases out quintically: quicker, and it settles sooner.
    Snappy,
    /// Overshoots a little, then rests.
    Springy,
}

impl Curve {
    /// `smooth`, `snappy` or `springy`.
    pub fn parse(name: &str) -> Option<Curve> {
        match name {
            "smooth" => Some(Curve::Smooth),
            "snappy" => Some(Curve::Snappy),
            "springy" => Some(Curve::Springy),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Curve::Smooth => "smooth",
            Curve::Snappy => "snappy",
            Curve::Springy => "springy",
        }
    }
}

/// The progress `curve` makes of the linear progress `t` (0 to 1): 0 at the
/// start and exactly 1 at the end, in between the curve's own shape.
pub fn curve(curve: Curve, t: f64) -> f64 {
    match curve {
        Curve::Smooth => 1.0 - (1.0 - t).powi(3),
        Curve::Snappy => 1.0 - (1.0 - t).powi(5),
        Curve::Springy => 1.0 - (-6.0 * t).exp() * (1.0 - t) * (t * 2.5 * PI).cos(),
    }
}

/// How windows animate at one tier and motion.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    /// How long a window takes to fade in, and grow if `zoom` is under 1.
    pub open: Duration,
    /// How long a closed window takes to fade out, and shrink.
    pub close: Duration,
    /// How long a window's slide to a new place or a workspace switch takes
    /// to settle: its spring settles over about this length. Zero jumps.
    pub slide: Duration,
    /// The share of its size an opening window grows from and a closing
    /// one shrinks to; 1 neither grows nor shrinks.
    pub zoom: f64,
    /// How each of them eases (M5.12b4).
    pub curve: Curve,
}

/// The style for `tier`, `motion`, `speed` (`appearance.animation_speed`,
/// a percent of the release's speed: 200 halves every length, 50 doubles
/// it) and `curve`: Full grows windows from 90 percent over 200 ms,
/// Balanced from 95 percent over 150 ms, Lite only fades, over 100 ms
/// (ADR-002: "flat surfaces, short fades"); closing takes three quarters
/// as long. Reduced motion keeps the fades and drops the rest; off drops
/// everything.
pub fn style(tier: Tier, motion: Motion, speed: u32, curve: Curve) -> Style {
    let (open, zoom, slide) = match tier {
        Tier::Full => (200, 0.9, 200),
        Tier::Balanced => (150, 0.95, 150),
        Tier::Lite => (100, 1.0, 0),
    };
    // A length in milliseconds at `speed`, rounded to a whole millisecond;
    // zero stays zero.
    let speed = u64::from(speed.max(1));
    let ms = |length: u64| Duration::from_millis((length * 100 + speed / 2) / speed);
    match motion {
        Motion::Full => Style {
            open: ms(open),
            close: ms(open * 3 / 4),
            slide: ms(slide),
            zoom,
            curve,
        },
        Motion::Reduced => Style {
            open: ms(open),
            close: ms(open * 3 / 4),
            slide: Duration::ZERO,
            zoom: 1.0,
            curve,
        },
        Motion::Off => Style {
            open: Duration::ZERO,
            close: Duration::ZERO,
            slide: Duration::ZERO,
            zoom: 1.0,
            curve,
        },
    }
}

/// How stiff a slide's spring is: its natural frequency is this over the
/// slide's length (M5.14), so a smooth slide from rest is within half a
/// pixel of a 4K screen's width after about 1.3 lengths.
const STIFFNESS: f64 = 9.0;
/// Snappy's spring is this much stiffer than smooth's.
const SNAPPY_STIFFER: f64 = 1.5;
/// Springy's damping ratio: under 1, so it overshoots by about 5 percent,
/// swings back once barely, then rests; smooth and snappy are critically
/// damped (1), which never overshoots from rest.
const SPRINGY_DAMPING: f64 = 0.7;
/// A spring rests once it is this close to its place on each axis, in
/// logical pixels. It stops within half a logical pixel, so at scale 2 the
/// last step can be one physical pixel.
const REST_OFFSET: f64 = 0.5;
/// A spring rests once it is slower than this on each axis, in logical
/// pixels a second (half a pixel a frame at 60 Hz).
const REST_SPEED: f64 = 30.0;

/// A damped spring pulling an offset to nothing (M5.14): where it is and
/// how fast it moves follow from when it began, so a late frame reads it
/// further on and nothing runs long, and a spring retargeted mid-way starts
/// from the position and speed it had.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spring {
    /// When it began, on the frame clock.
    start: Duration,
    /// Where it began, as an offset from its place.
    offset: Point<f64, Logical>,
    /// How fast it moved then, in logical px a second.
    velocity: Point<f64, Logical>,
    /// Its natural frequency, radians a second.
    omega: f64,
    /// Its damping ratio: 1 critically damped, under 1 it overshoots.
    zeta: f64,
}

impl Spring {
    /// A spring that starts at `offset` from its place, moving at `velocity`
    /// (logical px a second), at `now`, and settles over about `length` with
    /// `curve`'s stiffness and damping. A zero length counts as a millisecond.
    pub fn new(
        now: Duration,
        offset: Point<f64, Logical>,
        velocity: Point<f64, Logical>,
        length: Duration,
        curve: Curve,
    ) -> Spring {
        let secs = length.as_secs_f64().max(1e-3);
        let stiffer = if curve == Curve::Snappy {
            SNAPPY_STIFFER
        } else {
            1.0
        };
        Spring {
            start: now,
            offset,
            velocity,
            omega: STIFFNESS / secs * stiffer,
            zeta: if curve == Curve::Springy {
                SPRINGY_DAMPING
            } else {
                1.0
            },
        }
    }

    /// Position and velocity of one axis `t` seconds after the start, from
    /// its position `x0` and velocity `v0` there.
    fn axis(&self, x0: f64, v0: f64, t: f64) -> (f64, f64) {
        let w = self.omega;
        if self.zeta >= 1.0 {
            let b = v0 + w * x0;
            let e = (-w * t).exp();
            let x = (x0 + b * t) * e;
            let v = (b - w * (x0 + b * t)) * e;
            (x, v)
        } else {
            let z = self.zeta;
            let wd = w * (1.0 - z * z).sqrt();
            let s = (v0 + z * w * x0) / wd;
            let e = (-z * w * t).exp();
            let (sin, cos) = (wd * t).sin_cos();
            let x = e * (x0 * cos + s * sin);
            let v = e * (wd * (s * cos - x0 * sin) - z * w * (x0 * cos + s * sin));
            (x, v)
        }
    }

    /// Position and velocity on both axes at `now`.
    fn at(&self, now: Duration) -> (Point<f64, Logical>, Point<f64, Logical>) {
        let t = now.saturating_sub(self.start).as_secs_f64();
        let (x, vx) = self.axis(self.offset.x, self.velocity.x, t);
        let (y, vy) = self.axis(self.offset.y, self.velocity.y, t);
        ((x, y).into(), (vx, vy).into())
    }

    /// Where the spring pulls to at `now`, as an offset from its place.
    pub fn offset_at(&self, now: Duration) -> Point<f64, Logical> {
        self.at(now).0
    }

    /// How fast the spring moves at `now`, in logical px a second.
    pub fn velocity_at(&self, now: Duration) -> Point<f64, Logical> {
        self.at(now).1
    }

    /// Whether the spring has come to rest by `now`: each axis is within
    /// `REST_OFFSET` of its place and slower than `REST_SPEED`.
    pub fn done(&self, now: Duration) -> bool {
        let (offset, velocity) = self.at(now);
        offset.x.abs() < REST_OFFSET
            && offset.y.abs() < REST_OFFSET
            && velocity.x.abs() < REST_SPEED
            && velocity.y.abs() < REST_SPEED
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

    /// How far along it is at `now`, eased by `curve`: 0 at the start, 1 at
    /// the end and after; by default quick at first and settling gently, as
    /// things that stop do.
    pub fn eased(&self, now: Duration, curve: Curve) -> f64 {
        if self.length.is_zero() {
            return 1.0;
        }
        let t = (now.saturating_sub(self.start).as_secs_f64() / self.length.as_secs_f64())
            .clamp(0.0, 1.0);
        self::curve(curve, t)
    }
}

/// An opening window `eased` of the way in: its alpha and its size as a
/// share of its own. A springy curve overshoots the size, which is the
/// spring, but the alpha stays within 0 to 1.
pub fn opening(style: &Style, eased: f64) -> (f32, f64) {
    (
        (eased as f32).clamp(0.0, 1.0),
        style.zoom + (1.0 - style.zoom) * eased,
    )
}

/// A closed window `eased` of the way out: its alpha and its size as a
/// share of its own.
pub fn closing(style: &Style, eased: f64) -> (f32, f64) {
    (
        ((1.0 - eased) as f32).clamp(0.0, 1.0),
        1.0 - (1.0 - style.zoom) * eased,
    )
}

/// A window drawn away from its place by an offset that a spring pulls to
/// nothing: a person sees it slide from where it was to where it is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Slide {
    /// The spring carrying the window to its place (M5.14).
    pub spring: Spring,
}

impl Slide {
    /// The slide of a window moved by the layout from `from` to `to` at
    /// `now`. A slide that interrupts another starts from where the window
    /// is drawn, at the speed it moved. None when there is nothing to slide
    /// or no time to do it in.
    pub fn start(
        previous: Option<&Slide>,
        from: Point<i32, Logical>,
        to: Point<i32, Logical>,
        now: Duration,
        length: Duration,
        curve: Curve,
    ) -> Option<Slide> {
        if length.is_zero() {
            return None;
        }
        let (carried, speed) = previous.map_or_else(
            || (Point::default(), Point::default()),
            |s| (s.offset_at(now), s.velocity_at(now)),
        );
        let offset = (from - to).to_f64() + carried;
        if offset.x.abs() < REST_OFFSET
            && offset.y.abs() < REST_OFFSET
            && speed.x.abs() < REST_SPEED
            && speed.y.abs() < REST_SPEED
        {
            return None;
        }
        Some(Slide {
            spring: Spring::new(now, offset, speed, length, curve),
        })
    }

    /// How far from its place the window is drawn at `now`.
    pub fn offset_at(&self, now: Duration) -> Point<f64, Logical> {
        self.spring.offset_at(now)
    }

    /// How fast the window moves at `now`, in logical px a second.
    pub fn velocity_at(&self, now: Duration) -> Point<f64, Logical> {
        self.spring.velocity_at(now)
    }

    /// Whether the slide has come to rest by `now`.
    pub fn done(&self, now: Duration) -> bool {
        self.spring.done(now)
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
        let full = style(Tier::Full, Motion::Full, 100, Curve::Smooth);
        let balanced = style(Tier::Balanced, Motion::Full, 100, Curve::Smooth);
        let lite = style(Tier::Lite, Motion::Full, 100, Curve::Smooth);
        assert!(full.open > balanced.open && balanced.open > lite.open);
        assert!(full.zoom < balanced.zoom && balanced.zoom < lite.zoom);
        // Lite only fades: nothing grows or slides.
        assert_eq!((lite.zoom, lite.slide), (1.0, Duration::ZERO));
        assert_eq!(full.close, MS(150));
    }

    #[test]
    fn reduced_motion_keeps_fades_only_and_off_keeps_nothing() {
        for tier in [Tier::Lite, Tier::Balanced, Tier::Full] {
            let reduced = style(tier, Motion::Reduced, 100, Curve::Smooth);
            assert_eq!(
                reduced.open,
                style(tier, Motion::Full, 100, Curve::Smooth).open
            );
            assert_eq!((reduced.zoom, reduced.slide), (1.0, Duration::ZERO));
            let off = style(tier, Motion::Off, 100, Curve::Smooth);
            assert!(off.open.is_zero() && off.close.is_zero() && off.slide.is_zero());
        }
    }

    #[test]
    fn progress_eases_out_and_ends_on_time() {
        let anim = Anim::new(MS(1000), MS(200));
        assert_eq!(anim.eased(MS(900), Curve::Smooth), 0.0, "before it starts");
        assert_eq!(anim.eased(MS(1000), Curve::Smooth), 0.0);
        let half = anim.eased(MS(1100), Curve::Smooth);
        assert!(half > 0.5 && half < 1.0, "quicker than linear: {half}");
        assert_eq!(anim.eased(MS(1200), Curve::Smooth), 1.0);
        assert_eq!(
            anim.eased(MS(5000), Curve::Smooth),
            1.0,
            "a late frame skips to the end"
        );
        assert!(!anim.done(MS(1199)) && anim.done(MS(1200)));
        assert_eq!(
            Anim::new(MS(0), Duration::ZERO).eased(MS(0), Curve::Smooth),
            1.0
        );
    }

    #[test]
    fn windows_fade_and_grow_in_and_out() {
        let full = style(Tier::Full, Motion::Full, 100, Curve::Smooth);
        assert_eq!(opening(&full, 0.0), (0.0, 0.9));
        assert_eq!(opening(&full, 1.0), (1.0, 1.0));
        assert_eq!(closing(&full, 0.0), (1.0, 1.0));
        let (alpha, zoom) = closing(&full, 1.0);
        assert_eq!(alpha, 0.0);
        assert!((zoom - 0.9).abs() < 1e-9);
        let lite = style(Tier::Lite, Motion::Full, 100, Curve::Smooth);
        assert_eq!(opening(&lite, 0.5).1, 1.0, "Lite never grows");
    }

    #[test]
    fn each_curve_starts_at_zero_and_ends_on_one() {
        for c in [Curve::Smooth, Curve::Snappy, Curve::Springy] {
            assert_eq!(curve(c, 0.0), 0.0, "{}", c.name());
            assert_eq!(curve(c, 1.0), 1.0, "{}", c.name());
            assert_eq!(Curve::parse(c.name()), Some(c));
        }
        assert_eq!(Curve::parse("bouncy"), None);
        assert_eq!(Curve::default(), Curve::Smooth);
    }

    #[test]
    fn snappy_settles_sooner_than_smooth() {
        assert!(curve(Curve::Snappy, 0.3) > curve(Curve::Smooth, 0.3));
    }

    #[test]
    fn springy_overshoots_a_little_and_never_dips_below_zero() {
        let mut largest = f64::MIN;
        let mut lowest = f64::MAX;
        for i in 0..=1000 {
            let value = curve(Curve::Springy, f64::from(i) / 1000.0);
            largest = largest.max(value);
            lowest = lowest.min(value);
        }
        assert!(largest > 1.01 && largest < 1.10, "overshoot {largest}");
        assert!(lowest >= 0.0, "dips to {lowest}");
    }

    #[test]
    fn speed_halves_or_doubles_every_length() {
        let full = style(Tier::Full, Motion::Full, 100, Curve::Smooth);
        let quick = style(Tier::Full, Motion::Full, 200, Curve::Smooth);
        let slow = style(Tier::Full, Motion::Full, 50, Curve::Smooth);
        assert_eq!(quick.open, MS(100));
        assert_eq!(quick.close, MS(75));
        assert_eq!(quick.slide, MS(100));
        assert_eq!(slow.open, MS(400));
        assert_eq!(slow.close, MS(300));
        assert_eq!(full.open, MS(200));
        assert_eq!(full.zoom, quick.zoom, "speed changes no size");
        for tier in [Tier::Lite, Tier::Balanced, Tier::Full] {
            let at_100 = style(tier, Motion::Full, 100, Curve::Smooth);
            for (speed, quicker) in [(50, false), (200, true)] {
                let other = style(tier, Motion::Full, speed, Curve::Smooth);
                if quicker {
                    assert!(other.open < at_100.open || at_100.open.is_zero());
                } else {
                    assert!(other.open > at_100.open || at_100.open.is_zero());
                }
                assert_eq!(other.zoom, at_100.zoom);
            }
            let off = style(tier, Motion::Off, 50, Curve::Snappy);
            assert!(off.open.is_zero() && off.close.is_zero() && off.slide.is_zero());
            assert_eq!(off.curve, Curve::Snappy);
        }
    }

    #[test]
    fn the_curve_is_carried_into_the_style() {
        let springy = style(Tier::Full, Motion::Full, 100, Curve::Springy);
        assert_eq!(springy.curve, Curve::Springy);
        assert_eq!(springy.open, MS(200));
    }

    #[test]
    fn a_springy_window_overshoots_its_size_but_not_its_alpha() {
        let springy = style(Tier::Full, Motion::Full, 100, Curve::Springy);
        let mut grew_past_one = false;
        for i in 0..=100 {
            let eased = Anim::new(MS(0), MS(200)).eased(MS(i * 2), springy.curve);
            let (alpha, zoom) = opening(&springy, eased);
            assert!((0.0..=1.0).contains(&alpha));
            grew_past_one |= zoom > 1.0;
        }
        assert!(grew_past_one, "the spring overshoots the size");
    }

    #[test]
    fn a_popup_fades_and_grows_from_its_zoom_to_its_own_size() {
        let full = style(Tier::Full, Motion::Full, 100, Curve::Smooth);
        assert_eq!(opening(&full, 0.0), (0.0, 0.9), "its start");
        assert_eq!(opening(&full, 1.0), (1.0, 1.0), "its end");
    }

    /// Whether two points are the same to within a millionth of a pixel.
    fn near(p: Point<f64, Logical>, q: Point<f64, Logical>) -> bool {
        (p.x - q.x).abs() < 1e-6 && (p.y - q.y).abs() < 1e-6
    }

    #[test]
    fn a_slide_needs_a_move_and_some_time() {
        let a = Point::from((0, 0));
        let b = Point::from((100, 0));
        // Nothing to slide, or no time to slide in.
        assert_eq!(
            Slide::start(None, b, b, MS(0), MS(200), Curve::Smooth),
            None
        );
        assert_eq!(
            Slide::start(None, a, b, MS(0), Duration::ZERO, Curve::Smooth),
            None
        );
    }

    #[test]
    fn a_smooth_slide_settles_within_300_ms_without_overshoot() {
        let full = style(Tier::Full, Motion::Full, 100, Curve::Smooth).slide;
        let balanced = style(Tier::Balanced, Motion::Full, 100, Curve::Smooth).slide;
        for length in [full, balanced] {
            for d in [1, 1280, 3840] {
                let slide = Slide::start(
                    None,
                    Point::from((d, 0)),
                    Point::from((0, 0)),
                    MS(0),
                    length,
                    Curve::Smooth,
                )
                .expect("it moved");
                let mut last = f64::MAX;
                for ms in 0..=300 {
                    let x = slide.offset_at(MS(ms)).x;
                    assert!(x >= 0.0, "{d} px: past its place at {ms} ms: {x}");
                    assert!(x <= last + 1e-9, "{d} px: grew again at {ms} ms");
                    last = x;
                }
                assert!(slide.done(MS(300)), "{d} px: still moving at 300 ms");
            }
        }
    }

    #[test]
    fn an_interrupted_slide_keeps_its_place_and_speed() {
        let a = Point::from((0, 0));
        let b = Point::from((1000, 0));
        let c = Point::from((1500, 0));
        let length = style(Tier::Full, Motion::Full, 100, Curve::Smooth).slide;
        let first = Slide::start(None, a, b, MS(0), length, Curve::Smooth).expect("it moved");
        // At 60 ms the layout moves it on from b to c, mid-slide.
        let second =
            Slide::start(Some(&first), b, c, MS(60), length, Curve::Smooth).expect("it moved");
        let drawn = b.to_f64() + first.offset_at(MS(60));
        let carried = c.to_f64() + second.offset_at(MS(60));
        assert!(near(drawn, carried), "drawn {drawn:?}, carried {carried:?}");
        assert!(
            near(first.velocity_at(MS(60)), second.velocity_at(MS(60))),
            "the speed carries over"
        );
        assert!(second.done(MS(60 + 300)));
    }

    #[test]
    fn springy_overshoots_a_little_then_rests() {
        let length = style(Tier::Full, Motion::Full, 100, Curve::Springy).slide;
        let slide = Slide::start(
            None,
            Point::from((1000, 0)),
            Point::from((0, 0)),
            MS(0),
            length,
            Curve::Springy,
        )
        .expect("it moved");
        let mut lowest = f64::MAX;
        for ms in 0..=600 {
            lowest = lowest.min(slide.offset_at(MS(ms)).x);
        }
        assert!((-100.0..0.0).contains(&lowest), "lowest {lowest}");
        assert!(slide.done(MS(600)));
    }

    #[test]
    fn a_snappy_slide_settles_sooner_than_a_smooth_one() {
        let length = style(Tier::Full, Motion::Full, 100, Curve::Smooth).slide;
        let settled = |curve| {
            let slide = Slide::start(
                None,
                Point::from((1280, 0)),
                Point::from((0, 0)),
                MS(0),
                length,
                curve,
            )
            .expect("it moved");
            (0..=1000)
                .find(|&ms| slide.done(MS(ms)))
                .expect("it settles")
        };
        assert!(settled(Curve::Snappy) < settled(Curve::Smooth));
    }

    #[test]
    fn a_spring_at_rest_stays_at_rest() {
        let spring = Spring::new(
            MS(0),
            (0.0, 0.0).into(),
            (0.0, 0.0).into(),
            MS(200),
            Curve::Smooth,
        );
        assert!(spring.done(MS(0)));
        assert_eq!(spring.offset_at(MS(0)), Point::from((0.0, 0.0)));
    }
}
