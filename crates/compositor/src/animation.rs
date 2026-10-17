//! Window animations (roadmap M5.11b, ADR-002 section 5): how long and how
//! far windows animate when they open, close and move, by effect tier and
//! the person's `appearance.animations`, how fast (`appearance.animation_speed`,
//! M5.12b4) and with what feel (`appearance.animation_curve`). Opening,
//! closing and popups run on the eased clock: their progress is the time since
//! they began over their length, read when a frame is drawn, so a late frame
//! skips ahead and never makes it run long. Slides run on springs (M5.14) that
//! carry their position and speed when interrupted, so nothing jumps. A
//! workspace switch is one spring per screen that the incoming windows and the
//! leaving pictures share, which three fingers on a touchpad can hold (M5.14).
//! Fades and growing keep the eased curves.

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

/// How an animation eases (M5.12b4). `appearance.animation_curve` names one,
/// the feel, and `Feel` gives each kind of animation its own from it.
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
    /// How each kind eases (M5.12b4, a curve per kind in M8.21 part 21e).
    pub feel: Feel,
}

/// The curve each kind of animation follows for one feel
/// (`appearance.animation_curve`, M8.21 part 21e): a preset, not three keys.
/// Pop-ups are small and asked for, so they answer snappily under the smooth
/// feel; a workspace moves the whole screen, so under the springy feel it
/// settles snappily rather than swinging past its place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Feel {
    /// The curve the feel was named by, which the log says.
    pub name: Curve,
    /// Windows opening, closing and sliding to a new place.
    pub windows: Curve,
    /// A workspace switch, and a swipe let go.
    pub workspaces: Curve,
    /// Pop-ups popping up.
    pub popups: Curve,
}

impl Feel {
    /// The curves of the feel `name`.
    pub fn of(name: Curve) -> Feel {
        let (windows, workspaces, popups) = match name {
            Curve::Smooth => (Curve::Smooth, Curve::Smooth, Curve::Snappy),
            Curve::Snappy => (Curve::Snappy, Curve::Snappy, Curve::Snappy),
            Curve::Springy => (Curve::Springy, Curve::Snappy, Curve::Springy),
        };
        Feel {
            name,
            windows,
            workspaces,
            popups,
        }
    }
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
            feel: Feel::of(curve),
        },
        Motion::Reduced => Style {
            open: ms(open),
            close: ms(open * 3 / 4),
            slide: Duration::ZERO,
            zoom: 1.0,
            feel: Feel::of(curve),
        },
        Motion::Off => Style {
            open: Duration::ZERO,
            close: Duration::ZERO,
            slide: Duration::ZERO,
            zoom: 1.0,
            feel: Feel::of(curve),
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

/// How far fingers slide across a touchpad, in its own pixels, to move
/// the workspaces by one whole screen (M5.14), as GNOME's swipe does.
pub const SWIPE_SPAN: f64 = 400.0;
/// How far three fingers slide across before they take the workspaces,
/// so a swipe meant up or down never switches.
pub const SWIPE_START: f64 = 16.0;

/// A workspace switch on one screen (M5.14): the workspace coming in is
/// drawn `offset_at` from its place and every picture leaving with the one
/// before is drawn from there too; a spring takes the offset to nothing,
/// unless a person's fingers hold it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Switch {
    /// What moves the switch now.
    drive: Drive,
}

/// What moves a switch: its spring, or fingers on a touchpad.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Drive {
    /// A spring pulling the offset to nothing, as a switch runs by itself.
    Spring(Spring),
    /// Fingers hold the switch at an offset, moving at a speed.
    Held {
        /// Where the switch is held, as an offset from its place.
        offset: Point<f64, Logical>,
        /// How fast the fingers moved it, in logical px a second.
        velocity: Point<f64, Logical>,
    },
}

impl Switch {
    /// A switch that begins at `now`: the leaving workspace goes `by`, the
    /// other comes in from `-by`. A switch that interrupts `previous`
    /// carries what was coming in from where it was drawn, at its speed. A
    /// zero `length` makes a 1 ms spring.
    pub fn start(
        previous: Option<&Switch>,
        by: Point<f64, Logical>,
        now: Duration,
        length: Duration,
        curve: Curve,
    ) -> Switch {
        let (carried, speed) = previous.map_or_else(
            || (Point::default(), Point::default()),
            |p| (p.offset_at(now), p.velocity_at(now)),
        );
        Switch {
            drive: Drive::Spring(Spring::new(now, carried - by, speed, length, curve)),
        }
    }

    /// Where the switch draws its workspace at `now`, as an offset from its
    /// place: the spring's, or the one fingers hold.
    pub fn offset_at(&self, now: Duration) -> Point<f64, Logical> {
        match self.drive {
            Drive::Spring(spring) => spring.offset_at(now),
            Drive::Held { offset, .. } => offset,
        }
    }

    /// How fast the switch moves at `now`, in logical px a second: the
    /// spring's, or the speed fingers gave it.
    pub fn velocity_at(&self, now: Duration) -> Point<f64, Logical> {
        match self.drive {
            Drive::Spring(spring) => spring.velocity_at(now),
            Drive::Held { velocity, .. } => velocity,
        }
    }

    /// Fingers hold the switch at `offset`, moving at `velocity`, until
    /// `release`.
    pub fn hold(&mut self, offset: Point<f64, Logical>, velocity: Point<f64, Logical>) {
        self.drive = Drive::Held { offset, velocity };
    }

    /// Fingers let go: a held switch springs on from where it is held, at
    /// the speed it was held with. A switch that is not held stays as it is.
    pub fn release(&mut self, now: Duration, length: Duration, curve: Curve) {
        if let Drive::Held { offset, velocity } = self.drive {
            self.drive = Drive::Spring(Spring::new(now, offset, velocity, length, curve));
        }
    }

    /// Whether fingers hold the switch.
    pub fn held(&self) -> bool {
        matches!(self.drive, Drive::Held { .. })
    }

    /// Whether the switch has come to rest by `now`. A held switch is never
    /// done: it waits for the fingers.
    pub fn done(&self, now: Duration) -> bool {
        match self.drive {
            Drive::Spring(spring) => spring.done(now),
            Drive::Held { .. } => false,
        }
    }
}

/// Where three fingers that slid `dx` across (touchpad pixels, from where
/// they began) put the incoming workspace of a switch of `by_x` on a
/// screen `width` wide: one to one, `SWIPE_SPAN` for the whole screen,
/// never past either workspace's place (M5.14).
pub fn swipe_offset(by_x: f64, dx: f64, width: f64) -> f64 {
    let x = -by_x + dx * width / SWIPE_SPAN;
    let (lo, hi) = if by_x < 0.0 {
        (0.0, -by_x)
    } else {
        (-by_x, 0.0)
    };
    x.clamp(lo, hi)
}

/// Whether fingers that let go at `offset_x`, moving at `velocity_x`
/// (logical px a second), go on to the incoming workspace of a switch of
/// `by_x`: where it would be a tenth of a second on is less than half the
/// way still to go, as a share of `by_x`. Else the switch goes back.
pub fn swipe_goes(offset_x: f64, velocity_x: f64, by_x: f64) -> bool {
    (offset_x + velocity_x * 0.1) / -by_x < 0.5
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
            assert_eq!(off.feel, Feel::of(Curve::Snappy));
        }
    }

    #[test]
    fn the_curve_is_carried_into_the_style() {
        let springy = style(Tier::Full, Motion::Full, 100, Curve::Springy);
        assert_eq!(springy.feel, Feel::of(Curve::Springy));
        assert_eq!(springy.open, MS(200));
    }

    #[test]
    fn each_feel_gives_each_kind_its_curve() {
        let smooth = Feel::of(Curve::Smooth);
        assert_eq!(
            (smooth.windows, smooth.workspaces, smooth.popups),
            (Curve::Smooth, Curve::Smooth, Curve::Snappy)
        );
        let snappy = Feel::of(Curve::Snappy);
        assert_eq!(
            (snappy.windows, snappy.workspaces, snappy.popups),
            (Curve::Snappy, Curve::Snappy, Curve::Snappy)
        );
        let springy = Feel::of(Curve::Springy);
        assert_eq!(
            (springy.windows, springy.workspaces, springy.popups),
            (Curve::Springy, Curve::Snappy, Curve::Springy)
        );
        for name in [Curve::Smooth, Curve::Snappy, Curve::Springy] {
            assert_eq!(Feel::of(name).name, name);
        }
    }

    #[test]
    fn a_springy_window_overshoots_its_size_but_not_its_alpha() {
        let springy = style(Tier::Full, Motion::Full, 100, Curve::Springy);
        let mut grew_past_one = false;
        for i in 0..=100 {
            let eased = Anim::new(MS(0), MS(200)).eased(MS(i * 2), springy.feel.windows);
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

    #[test]
    fn a_new_switch_starts_its_workspace_away_and_settles_in_300_ms() {
        let length = style(Tier::Full, Motion::Full, 100, Curve::Smooth).slide;
        let by = Point::from((-1280.0, 0.0));
        let switch = Switch::start(None, by, MS(0), length, Curve::Smooth);
        assert!(near(switch.offset_at(MS(0)), (1280.0, 0.0).into()));
        assert!(near(switch.velocity_at(MS(0)), (0.0, 0.0).into()));
        assert!(switch.done(MS(300)), "still moving at 300 ms");
    }

    #[test]
    fn a_switch_that_interrupts_another_keeps_its_place_and_speed() {
        let length = style(Tier::Full, Motion::Full, 100, Curve::Smooth).slide;
        let by1 = Point::from((-1280.0, 0.0));
        let by2 = Point::from((-1280.0, 0.0));
        let first = Switch::start(None, by1, MS(0), length, Curve::Smooth);
        let second = Switch::start(Some(&first), by2, MS(60), length, Curve::Smooth);
        assert!(near(
            second.offset_at(MS(60)),
            first.offset_at(MS(60)) - by2
        ));
        assert!(near(second.velocity_at(MS(60)), first.velocity_at(MS(60))));
    }

    #[test]
    fn a_held_switch_waits_for_the_fingers_then_springs_on_from_there() {
        let length = style(Tier::Full, Motion::Full, 100, Curve::Smooth).slide;
        let mut switch = Switch::start(
            None,
            Point::from((-1280.0, 0.0)),
            MS(0),
            length,
            Curve::Smooth,
        );
        switch.hold((300.0, 0.0).into(), (-2000.0, 0.0).into());
        assert!(switch.held());
        assert!(!switch.done(MS(0)) && !switch.done(MS(10_000)), "held");
        assert!(near(switch.offset_at(MS(50)), (300.0, 0.0).into()));
        switch.release(MS(100), length, Curve::Smooth);
        assert!(!switch.held());
        assert!(near(switch.offset_at(MS(100)), (300.0, 0.0).into()));
        assert!(near(switch.velocity_at(MS(100)), (-2000.0, 0.0).into()));
        assert!(switch.done(MS(100 + 400)), "settles after the release");
    }

    #[test]
    fn releasing_a_switch_that_is_not_held_changes_nothing() {
        let length = style(Tier::Full, Motion::Full, 100, Curve::Smooth).slide;
        let mut switch = Switch::start(
            None,
            Point::from((-1280.0, 0.0)),
            MS(0),
            length,
            Curve::Smooth,
        );
        let before = switch;
        switch.release(MS(100), length, Curve::Smooth);
        assert_eq!(switch, before);
    }

    #[test]
    fn swipe_offset_follows_the_fingers_one_to_one_within_both_places() {
        let width = 1280.0;
        // Forward: the next workspace comes in from the right.
        assert_eq!(swipe_offset(-1280.0, 0.0, width), 1280.0);
        assert_eq!(swipe_offset(-1280.0, -400.0, width), 0.0, "a whole screen");
        assert_eq!(
            swipe_offset(-1280.0, -800.0, width),
            0.0,
            "clamped at its place"
        );
        assert_eq!(
            swipe_offset(-1280.0, 100.0, width),
            1280.0,
            "clamped at its start"
        );
        assert_eq!(swipe_offset(-1280.0, -200.0, width), 640.0, "half a screen");
        // Back: the previous workspace comes in from the left.
        assert_eq!(swipe_offset(1280.0, 200.0, width), -640.0);
    }

    #[test]
    fn swipe_goes_on_past_half_a_screen_or_on_a_flick() {
        assert!(swipe_goes(500.0, 0.0, -1280.0));
        assert!(
            !swipe_goes(700.0, 0.0, -1280.0),
            "more than half way still to go"
        );
        assert!(swipe_goes(700.0, -2000.0, -1280.0), "a flick goes on");
        assert!(
            swipe_goes(1200.0, -20000.0, -1280.0),
            "a strong flick goes on"
        );
    }
}
