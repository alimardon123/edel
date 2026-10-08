//! Outputs and window policies (ADR-002). Floating (M4.3) and dynamic
//! tiling (M4.5, `tiling.rs`) are two implementations of [`WindowPolicy`],
//! switchable per [`Workspace`], with title bars in both; nothing outside
//! this crate plugs into it (no extension API). A policy is one module and
//! one line in [`policies`]. The types here are plain data, so policies
//! are tested without a display.

use smithay::utils::{Logical, Physical, Point, Rectangle, Size};

use crate::tiling::{Style, Tiling};

/// Every policy, the default first: what `layout.tiling` and Super+T choose
/// between. Their names are what people see (ADR-008), never the
/// algorithm's, so a later tiling algorithm keeps everyone's setting.
pub fn policies<W: Clone + PartialEq + 'static>(gap: u32) -> Vec<Box<dyn WindowPolicy<W>>> {
    vec![Box::new(Floating::default()), Box::new(Tiling::new(gap))]
}

/// Where windows go on one workspace.
pub trait WindowPolicy<W> {
    /// The name the settings file and the state file use: `floating` or
    /// `tiling`.
    fn name(&self) -> &'static str;

    /// A window opened, asking for `wanted` (zero when it lets the
    /// compositor choose); returns its place in `area`, the workspace's
    /// usable part of the output.
    fn open(
        &mut self,
        window: W,
        wanted: Size<i32, Logical>,
        area: Rectangle<i32, Logical>,
    ) -> Rectangle<i32, Logical>;

    /// A window closed.
    fn close(&mut self, window: &W);

    /// A person moved or resized `window` to `to`; returns where it stays.
    /// Floating keeps it there; tiling puts it back in its tile.
    fn moved(&mut self, window: &W, to: Rectangle<i32, Logical>) -> Rectangle<i32, Logical>;

    /// Every window's place, bottom of the stack first, after `area`
    /// changed: an output added, removed, resized or rescaled, or after
    /// the workspace switched to this policy.
    fn arrange(&mut self, area: Rectangle<i32, Logical>) -> Vec<(W, Rectangle<i32, Logical>)>;

    /// Whether opening or closing a window moves the others, so the
    /// compositor arranges them all again.
    fn rearranges(&self) -> bool {
        false
    }

    /// `window` took the keyboard: tiling's `split` halves it next, and
    /// `scroll` scrolls to it; whether the windows must be laid out again.
    fn focused(&mut self, _window: &W) -> bool {
        false
    }

    /// `window` takes the next width a column steps through (`scroll`'s
    /// Super+R); whether it changed.
    fn widen(&mut self, _window: &W) -> bool {
        false
    }

    /// `a` and `b` trade places; false when the policy leaves windows
    /// where people put them.
    fn swap(&mut self, _a: &W, _b: &W) -> bool {
        false
    }

    /// The tiling style (M5.16); floating has none.
    fn set_style(&mut self, _style: Style) {}
}

/// The areas windows lie in, one per screen, by the screen's name: each
/// screen's part left to windows once panels have taken their room.
pub type Areas = [(String, Rectangle<i32, Logical>)];

/// One workspace: on each screen, every policy follows that screen's
/// windows, and one of them, the active one, places them (M5.2g: each
/// screen its own area, so a window tiles and maximizes on its own).
/// Switching re-lays the windows out at once (ADR-002), and back in
/// floating they are where they were.
pub struct Workspace<W> {
    gap: u32,
    /// The tiling style new screens' tiling starts in.
    style: Style,
    /// Each policy's name and whether it rearranges, in [`policies`]'
    /// order, asked once.
    kinds: Vec<(&'static str, bool)>,
    screens: Vec<Screen<W>>,
    active: usize,
}

/// One screen's windows, in the order they opened, its policies and the
/// area they last placed windows in.
struct Screen<W> {
    name: String,
    area: Rectangle<i32, Logical>,
    windows: Vec<W>,
    policies: Vec<Box<dyn WindowPolicy<W>>>,
}

impl<W: Clone + PartialEq + 'static> Workspace<W> {
    /// A floating workspace with every policy of [`policies`].
    pub fn new(gap: u32) -> Workspace<W> {
        Workspace {
            gap,
            style: Style::default(),
            kinds: policies::<W>(gap)
                .iter()
                .map(|p| (p.name(), p.rearranges()))
                .collect(),
            screens: Vec::new(),
            active: 0,
        }
    }

    /// The active policy's name.
    pub fn name(&self) -> &'static str {
        self.kinds[self.active].0
    }

    /// Makes the policy called `name` the active one, on every screen;
    /// false if there is none, or it already is.
    pub fn switch(&mut self, name: &str) -> bool {
        match self.kinds.iter().position(|(n, _)| *n == name) {
            Some(i) if i != self.active => {
                self.active = i;
                true
            }
            _ => false,
        }
    }

    /// The next policy after the active one, for Super+T.
    pub fn next(&self) -> &'static str {
        self.kinds[(self.active + 1) % self.kinds.len()].0
    }

    /// The screen `window` lies on, by name.
    pub fn screen_of(&self, window: &W) -> Option<&str> {
        self.screens
            .iter()
            .find(|s| s.windows.contains(window))
            .map(|s| s.name.as_str())
    }

    /// `window` opened on the screen called `screen`, whose area is
    /// `area`; returns its place there.
    pub fn open(
        &mut self,
        window: W,
        wanted: Size<i32, Logical>,
        screen: &str,
        area: Rectangle<i32, Logical>,
    ) -> Rectangle<i32, Logical> {
        self.close(&window);
        let i = match self.screens.iter().position(|s| s.name == screen) {
            Some(i) => i,
            None => {
                let mut policies = policies(self.gap);
                for policy in &mut policies {
                    policy.set_style(self.style);
                }
                self.screens.push(Screen {
                    name: screen.to_string(),
                    area,
                    windows: Vec::new(),
                    policies,
                });
                self.screens.len() - 1
            }
        };
        let screen = &mut self.screens[i];
        screen.area = area;
        screen.windows.push(window.clone());
        let mut place = Rectangle::default();
        for (j, policy) in screen.policies.iter_mut().enumerate() {
            let at = policy.open(window.clone(), wanted, area);
            if j == self.active {
                place = at;
            }
        }
        place
    }

    pub fn close(&mut self, window: &W) {
        for screen in &mut self.screens {
            if screen.windows.contains(window) {
                screen.windows.retain(|w| w != window);
                for policy in &mut screen.policies {
                    policy.close(window);
                }
            }
        }
    }

    /// A person moved or resized `window` to `to`, on the screen called
    /// `screen` with area `area` (where its middle now lies); returns
    /// where it stays. A floating window moved to another screen moves to
    /// that screen's policies; a tiled one goes back to its tile.
    pub fn moved(
        &mut self,
        window: &W,
        to: Rectangle<i32, Logical>,
        screen: &str,
        area: Rectangle<i32, Logical>,
    ) -> Rectangle<i32, Logical> {
        let from = self.screen_of(window).map(str::to_string);
        if !self.rearranges() && from.as_deref().is_some_and(|f| f != screen) {
            self.open(window.clone(), to.size, screen, area);
        }
        let active = self.active;
        match self.screens.iter_mut().find(|s| s.windows.contains(window)) {
            Some(screen) => screen.policies[active].moved(window, to),
            None => to,
        }
    }

    /// Every window's place, after a screen came, went, or changed its
    /// area, or after the workspace switched policies: each screen's
    /// windows in its area from `areas`. The windows of a screen no longer
    /// there move to the first screen, at their sizes.
    pub fn arrange(&mut self, areas: &Areas) -> Vec<(W, Rectangle<i32, Logical>)> {
        let Some((first, first_area)) = areas.first() else {
            return Vec::new();
        };
        let active = self.active;
        let (gone, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut self.screens)
            .into_iter()
            .partition(|s| areas.iter().all(|(name, _)| *name != s.name));
        self.screens = kept;
        for mut screen in gone {
            let was = screen.area;
            for (window, place) in screen.policies[active].arrange(was) {
                self.open(window, place.size, first, *first_area);
            }
        }
        let mut placed = Vec::new();
        for screen in &mut self.screens {
            let Some((_, area)) = areas.iter().find(|(name, _)| *name == screen.name) else {
                continue;
            };
            screen.area = *area;
            placed.extend(screen.policies[active].arrange(*area));
        }
        placed
    }

    pub fn rearranges(&self) -> bool {
        self.kinds[self.active].1
    }

    /// `window` took the keyboard; whether the active policy must lay the
    /// windows out again, as `scroll` does to bring it into view.
    pub fn focused(&mut self, window: &W) -> bool {
        let active = self.active;
        let mut moved = false;
        for screen in &mut self.screens {
            if screen.windows.contains(window) {
                for (i, policy) in screen.policies.iter_mut().enumerate() {
                    moved |= policy.focused(window) && i == active;
                }
            }
        }
        moved
    }

    /// `window` takes the next width a column steps through, under the
    /// active policy; whether it changed.
    pub fn widen(&mut self, window: &W) -> bool {
        let active = self.active;
        self.screens
            .iter_mut()
            .find(|s| s.windows.contains(window))
            .is_some_and(|s| s.policies[active].widen(window))
    }

    /// `a` and `b`, on the same screen, trade places under the active
    /// policy and every other that keeps an order; false if they could not.
    pub fn swap(&mut self, a: &W, b: &W) -> bool {
        let active = self.active;
        let Some(screen) = self
            .screens
            .iter_mut()
            .find(|s| s.windows.contains(a) && s.windows.contains(b))
        else {
            return false;
        };
        let mut swapped = false;
        for (i, policy) in screen.policies.iter_mut().enumerate() {
            let traded = policy.swap(a, b);
            swapped |= traded && i == active;
        }
        swapped
    }

    /// Every screen's tiling lays its windows out in `style` from now on.
    pub fn set_style(&mut self, style: Style) {
        self.style = style;
        for screen in &mut self.screens {
            for policy in &mut screen.policies {
                policy.set_style(style);
            }
        }
    }
}

/// A way from one window to the next, for Super with an arrow (M5.16a).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

impl Direction {
    /// `left`, `right`, `up` or `down`, as the shortcuts' names end.
    pub fn parse(name: &str) -> Option<Direction> {
        match name {
            "left" => Some(Direction::Left),
            "right" => Some(Direction::Right),
            "up" => Some(Direction::Up),
            "down" => Some(Direction::Down),
            _ => None,
        }
    }
}

/// The window of `others` that lies `way` from `from`: of those whose
/// middle is past `from`'s middle that way, first those beside it (their
/// span across that way meets `from`'s), then the nearest edge, then the
/// middle most in line; an older window wins a tie. None when nothing
/// lies that way.
pub fn toward<W: Clone>(
    from: Rectangle<i32, Logical>,
    others: &[(W, Rectangle<i32, Logical>)],
    way: Direction,
) -> Option<W> {
    let across = matches!(way, Direction::Left | Direction::Right);
    // A rectangle's span across the way, and its middle there, doubled.
    let span = |r: &Rectangle<i32, Logical>| {
        if across {
            (r.loc.y, r.loc.y + r.size.h)
        } else {
            (r.loc.x, r.loc.x + r.size.w)
        }
    };
    let middle = |r: &Rectangle<i32, Logical>| (r.loc.x * 2 + r.size.w, r.loc.y * 2 + r.size.h);
    let (fx, fy) = middle(&from);
    let (a0, a1) = span(&from);
    others
        .iter()
        .filter_map(|(w, r)| {
            let (x, y) = middle(r);
            let (past, gap) = match way {
                Direction::Left => (fx - x, from.loc.x - (r.loc.x + r.size.w)),
                Direction::Right => (x - fx, r.loc.x - (from.loc.x + from.size.w)),
                Direction::Up => (fy - y, from.loc.y - (r.loc.y + r.size.h)),
                Direction::Down => (y - fy, r.loc.y - (from.loc.y + from.size.h)),
            };
            let (b0, b1) = span(r);
            let beside = b0 < a1 && a0 < b1;
            let off = ((b0 + b1) - (a0 + a1)).abs();
            (past > 0).then_some(((!beside, gap.max(0), off), w))
        })
        .min_by_key(|(key, _)| *key)
        .map(|(_, w)| w.clone())
}

/// How far each new window moves down and right when its centre would
/// meet an open window's centre.
pub const CASCADE: i32 = 32;

/// Floating windows (M4.3): a new window opens centred in the area at the
/// size it asks for (two thirds of the area when it lets the compositor
/// choose), [`CASCADE`] down and right of any open window whose centre it
/// would share, and inside the area; then it stays where people put it.
/// The same windows in the same order always land in the same places, which
/// CI's pixel checks rely on.
#[derive(Debug)]
pub struct Floating<W> {
    windows: Vec<(W, Rectangle<i32, Logical>)>,
}

impl<W> Default for Floating<W> {
    fn default() -> Self {
        Floating {
            windows: Vec::new(),
        }
    }
}

impl<W: Clone + PartialEq> WindowPolicy<W> for Floating<W> {
    fn name(&self) -> &'static str {
        "floating"
    }

    fn open(
        &mut self,
        window: W,
        wanted: Size<i32, Logical>,
        area: Rectangle<i32, Logical>,
    ) -> Rectangle<i32, Logical> {
        let size = if wanted.w > 0 && wanted.h > 0 {
            wanted
        } else {
            (area.size.w * 2 / 3, area.size.h * 2 / 3).into()
        };
        let size: Size<i32, Logical> = (size.w.min(area.size.w), size.h.min(area.size.h)).into();
        let centre = |r: &Rectangle<i32, Logical>| (r.loc.x * 2 + r.size.w, r.loc.y * 2 + r.size.h);
        let mut place = Rectangle::new(
            area.loc + Point::from(((area.size.w - size.w) / 2, (area.size.h - size.h) / 2)),
            size,
        );
        while self
            .windows
            .iter()
            .any(|(_, r)| centre(r) == centre(&place))
        {
            let moved = place.loc + Point::from((CASCADE, CASCADE));
            let fits = moved.x + size.w <= area.loc.x + area.size.w
                && moved.y + size.h <= area.loc.y + area.size.h;
            if !fits {
                break;
            }
            place.loc = moved;
        }
        self.windows.retain(|(w, _)| *w != window);
        self.windows.push((window, place));
        place
    }

    fn close(&mut self, window: &W) {
        self.windows.retain(|(w, _)| w != window);
    }

    fn moved(&mut self, window: &W, to: Rectangle<i32, Logical>) -> Rectangle<i32, Logical> {
        if let Some((_, place)) = self.windows.iter_mut().find(|(w, _)| w == window) {
            *place = to;
        }
        to
    }

    /// Keeps every window's size and moves it just enough to be inside the
    /// new area, so no window is left off screen.
    fn arrange(&mut self, area: Rectangle<i32, Logical>) -> Vec<(W, Rectangle<i32, Logical>)> {
        for (_, place) in &mut self.windows {
            let max_x = area.loc.x + (area.size.w - place.size.w).max(0);
            let max_y = area.loc.y + (area.size.h - place.size.h).max(0);
            place.loc.x = place.loc.x.clamp(area.loc.x, max_x);
            place.loc.y = place.loc.y.clamp(area.loc.y, max_y);
        }
        self.windows.clone()
    }
}

/// Logical pixels per inch a scale aims for: built-in panels are seen from
/// closer than monitors on a desk.
pub const BUILT_IN_PPI: f64 = 125.0;
pub const EXTERNAL_PPI: f64 = 110.0;

/// The scale a screen gets when `displays.NAME.scale` is absent (M4.6,
/// ADR-008): its pixels per inch, from the EDID's physical width, over
/// [`BUILT_IN_PPI`] or [`EXTERNAL_PPI`], rounded to 0.25 and kept from 1
/// to 3; 1 when the screen does not say its size. Worked out whenever a
/// screen appears and never written to the settings file.
pub fn auto_scale(mode: Size<i32, Physical>, size_mm: Size<i32, Physical>, built_in: bool) -> f64 {
    if mode.w <= 0 || size_mm.w <= 0 || size_mm.h <= 0 {
        return 1.0;
    }
    let ppi = f64::from(mode.w) / (f64::from(size_mm.w) / 25.4);
    let target = if built_in { BUILT_IN_PPI } else { EXTERNAL_PPI };
    ((ppi / target * 4.0).round() / 4.0).clamp(1.0, 3.0)
}

/// `scale` snapped to the nearest 1/120 and kept between 1 and 4, in
/// 120ths, as `wp_fractional_scale_v1` sends it.
pub fn scale_120(scale: f64) -> u32 {
    let scale = if scale.is_finite() { scale } else { 1.0 };
    (scale.clamp(1.0, 4.0) * 120.0).round() as u32
}

/// `scale` as the compositor uses it: [`scale_120`] back as a number.
pub fn snap_scale(scale: f64) -> f64 {
    f64::from(scale_120(scale)) / 120.0
}

/// `displays.NAME.resolution` and `refresh_rate` read: `1920x1080` or `1920x1080@60` (Hz, which
/// may have decimals).
pub fn parse_mode(text: &str) -> Option<(i32, i32, Option<f64>)> {
    let (size, refresh) = match text.trim().split_once('@') {
        Some((size, hz)) => (size, Some(hz.trim().parse::<f64>().ok()?)),
        None => (text.trim(), None),
    };
    let (w, h) = size.split_once('x')?;
    let (w, h) = (w.trim().parse().ok()?, h.trim().parse().ok()?);
    (w > 0 && h > 0 && refresh.is_none_or(|r| r > 0.0)).then_some((w, h, refresh))
}

/// The sizes a screen offers as the state file lists them (M5.7a), `WIDTHxHEIGHT`
/// as `displays.NAME.resolution` is written, each once, the largest first,
/// whatever refresh rates it comes in.
pub fn mode_sizes(modes: impl IntoIterator<Item = (i32, i32)>) -> Vec<String> {
    let mut sizes: Vec<(i32, i32)> = modes.into_iter().collect();
    sizes.sort_by_key(|&(w, h)| std::cmp::Reverse((i64::from(w) * i64::from(h), w)));
    sizes.dedup();
    sizes.into_iter().map(|(w, h)| format!("{w}x{h}")).collect()
}

/// Which of a screen's `modes` (width, height, refresh in mHz) `wanted`
/// names: its size at the refresh nearest the one asked for, or at the
/// highest when none is asked for; none when the screen lacks the size.
pub fn pick_mode(modes: &[(i32, i32, i32)], wanted: (i32, i32, Option<f64>)) -> Option<usize> {
    let (w, h, hz) = wanted;
    let same = modes
        .iter()
        .enumerate()
        .filter(|(_, m)| m.0 == w && m.1 == h);
    match hz {
        Some(hz) => same
            .min_by_key(|(_, m)| (f64::from(m.2) - hz * 1000.0).abs() as i64)
            .map(|(i, _)| i),
        None => same.max_by_key(|(_, m)| m.2).map(|(i, _)| i),
    }
}

/// A screen's logical size and the place the settings file gives it.
pub type ScreenPlace = (Size<i32, Logical>, Option<Point<i32, Logical>>);

/// Where screens go: each at its `displays.NAME.position` if it has one,
/// else to the right of the screens before it, tops at 0, in the order
/// given (the GPU's connector order).
pub fn place_screens(screens: &[ScreenPlace]) -> Vec<Point<i32, Logical>> {
    let mut right = 0;
    screens
        .iter()
        .map(|(size, position)| {
            let at = position.unwrap_or_else(|| Point::from((right, 0)));
            right = right.max(at.x + size.w);
            at
        })
        .collect()
}

/// One screen as the compositor lays it out. M4.6 adds its position,
/// transform and whether it is on, from `[displays.NAME]` in the settings file.
#[derive(Debug, Clone, PartialEq)]
pub struct OutputLayout {
    /// The connector's name, such as `eDP-1` or `Virtual-1`.
    pub name: String,
    /// The mode, in pixels.
    pub mode: Size<i32, Physical>,
    /// 1 or more; fractions in steps of 1/120, as `wp_fractional_scale_v1`
    /// sends them.
    pub scale: f64,
}

impl OutputLayout {
    /// `scale` snapped to the nearest 1/120 and kept between 1 and 4, the
    /// value every other number here is computed from.
    pub fn scale_120(&self) -> u32 {
        scale_120(self.scale)
    }

    /// The size windows are laid out in: the mode divided by the scale,
    /// rounded to whole logical pixels.
    pub fn logical_size(&self) -> Size<i32, Logical> {
        let scale = f64::from(self.scale_120()) / 120.0;
        let w = (f64::from(self.mode.w) / scale).round() as i32;
        let h = (f64::from(self.mode.h) / scale).round() as i32;
        (w, h).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_screen_lists_each_size_once_largest_first() {
        assert_eq!(
            mode_sizes([
                (1280, 720),
                (1920, 1080),
                (1280, 720),
                (1024, 768),
                (1920, 1080)
            ]),
            ["1920x1080", "1280x720", "1024x768"]
        );
        assert!(mode_sizes([]).is_empty());
    }

    fn output(w: i32, h: i32, scale: f64) -> OutputLayout {
        OutputLayout {
            name: "eDP-1".into(),
            mode: (w, h).into(),
            scale,
        }
    }

    #[test]
    fn logical_sizes_at_common_scales() {
        assert_eq!(output(1920, 1080, 1.0).logical_size(), (1920, 1080).into());
        assert_eq!(output(2560, 1600, 1.25).logical_size(), (2048, 1280).into());
        assert_eq!(output(1920, 1080, 1.5).logical_size(), (1280, 720).into());
        assert_eq!(output(2880, 1800, 2.0).logical_size(), (1440, 900).into());
        // 1.333... snaps to 160/120.
        let third = output(1920, 1200, 4.0 / 3.0);
        assert_eq!(third.scale_120(), 160);
        assert_eq!(third.logical_size(), (1440, 900).into());
    }

    /// The roadmap's table (M4.6): sizes as EDIDs give them, in mm.
    #[test]
    fn automatic_scales_for_common_screens() {
        let scale =
            |w, h, mm_w, mm_h, built_in| auto_scale((w, h).into(), (mm_w, mm_h).into(), built_in);
        // A 13.3-inch 1920x1080 laptop panel.
        assert_eq!(scale(1920, 1080, 294, 165, true), 1.25);
        // A 14-inch 2880x1800 laptop panel.
        assert_eq!(scale(2880, 1800, 302, 189, true), 2.0);
        // A 24-inch 1080p monitor.
        assert_eq!(scale(1920, 1080, 531, 299, false), 1.0);
        // A 27-inch 4K monitor.
        assert_eq!(scale(3840, 2160, 597, 336, false), 1.5);
        // No size: a projector, a VM's virtual screen.
        assert_eq!(scale(1280, 800, 0, 0, false), 1.0);
        // Never above 3, even on a phone-sized 4K panel.
        assert_eq!(scale(3840, 2160, 110, 62, true), 3.0);
    }

    #[test]
    fn modes_are_read_and_found_on_the_screen() {
        assert_eq!(parse_mode("1920x1080"), Some((1920, 1080, None)));
        assert_eq!(
            parse_mode("2560x1440@59.95"),
            Some((2560, 1440, Some(59.95)))
        );
        for bad in [
            "1920",
            "x1080",
            "0x1080",
            "1920x1080@",
            "1920x1080@-60",
            "big",
        ] {
            assert_eq!(parse_mode(bad), None, "{bad}");
        }
        let modes = [
            (1920, 1080, 60000),
            (1920, 1080, 144000),
            (1280, 720, 60000),
        ];
        assert_eq!(
            pick_mode(&modes, (1920, 1080, None)),
            Some(1),
            "the highest refresh"
        );
        assert_eq!(pick_mode(&modes, (1920, 1080, Some(59.94))), Some(0));
        assert_eq!(pick_mode(&modes, (3840, 2160, None)), None);
    }

    #[test]
    fn screens_go_left_to_right_unless_placed() {
        let laptop = Size::from((1280, 800));
        let monitor = Size::from((1920, 1080));
        assert_eq!(
            place_screens(&[(laptop, None), (monitor, None)]),
            [Point::from((0, 0)), Point::from((1280, 0))]
        );
        // A monitor placed left of the laptop; the next unplaced one goes
        // right of everything.
        assert_eq!(
            place_screens(&[
                (laptop, Some(Point::from((1920, 0)))),
                (monitor, Some(Point::from((0, 0)))),
                (laptop, None),
            ]),
            [
                Point::from((1920, 0)),
                Point::from((0, 0)),
                Point::from((3200, 0))
            ]
        );
    }

    #[test]
    fn scales_out_of_range_are_kept_usable() {
        assert_eq!(output(1280, 800, 0.5).scale_120(), 120);
        assert_eq!(output(1280, 800, 9.0).scale_120(), 480);
        assert_eq!(
            output(1280, 800, f64::NAN).logical_size(),
            (1280, 800).into()
        );
    }

    /// The smallest policy: every window fills the area, last on top.
    struct Fill(Vec<u32>);

    impl WindowPolicy<u32> for Fill {
        fn name(&self) -> &'static str {
            "fill"
        }
        fn open(
            &mut self,
            window: u32,
            _wanted: Size<i32, Logical>,
            area: Rectangle<i32, Logical>,
        ) -> Rectangle<i32, Logical> {
            self.0.push(window);
            area
        }
        fn close(&mut self, window: &u32) {
            self.0.retain(|w| w != window);
        }
        fn moved(
            &mut self,
            _window: &u32,
            _to: Rectangle<i32, Logical>,
        ) -> Rectangle<i32, Logical> {
            Rectangle::default()
        }
        fn arrange(
            &mut self,
            area: Rectangle<i32, Logical>,
        ) -> Vec<(u32, Rectangle<i32, Logical>)> {
            self.0.iter().map(|w| (*w, area)).collect()
        }
    }

    fn screen() -> Rectangle<i32, Logical> {
        Rectangle::from_size((1280, 800).into())
    }

    #[test]
    fn floating_windows_open_centred_and_cascade() {
        let mut floating = Floating::default();
        let one = floating.open(1, (300, 200).into(), screen());
        assert_eq!(one, Rectangle::new((490, 300).into(), (300, 200).into()));
        // Same centre as one: down and right, whatever its size.
        let two = floating.open(2, (200, 150).into(), screen());
        assert_eq!(two, Rectangle::new((572, 357).into(), (200, 150).into()));
        let three = floating.open(3, (400, 300).into(), screen());
        assert_eq!(three.loc, (504, 314).into());
        // A window that lets the compositor choose gets two thirds.
        let four = floating.open(4, (0, 0).into(), screen());
        assert_eq!(four.size, (853, 533).into());
        // Larger than the screen: the screen.
        let big = floating.open(5, (5000, 300).into(), screen());
        assert_eq!(big.size, (1280, 300).into());
        assert_eq!(big.loc.x, 0);
    }

    #[test]
    fn a_cascade_stops_at_the_edge_and_a_closed_window_frees_its_place() {
        let mut floating = Floating::default();
        let area = Rectangle::from_size((100, 100).into());
        let first = floating.open(1, (80, 80).into(), area);
        let second = floating.open(2, (80, 80).into(), area);
        assert_eq!(
            first, second,
            "no room to cascade: same place, still inside"
        );
        floating.close(&1);
        floating.close(&2);
        let third = floating.open(3, (300, 200).into(), screen());
        assert_eq!(third.loc, (490, 300).into());
    }

    #[test]
    fn moved_windows_stay_and_a_smaller_screen_pulls_them_in() {
        let mut floating = Floating::default();
        floating.open(1, (300, 200).into(), screen());
        let to = Rectangle::new((1000, 700).into(), (300, 200).into());
        assert_eq!(floating.moved(&1, to), to);
        let small = Rectangle::from_size((1024, 768).into());
        let arranged = floating.arrange(small);
        assert_eq!(
            arranged,
            [(1, Rectangle::new((724, 568).into(), (300, 200).into()))]
        );
        assert_eq!(floating.name(), "floating");
    }

    fn one_screen() -> Vec<(String, Rectangle<i32, Logical>)> {
        vec![("one".into(), screen())]
    }

    #[test]
    fn a_workspace_switches_policies_and_floating_keeps_its_places() {
        let mut workspace = Workspace::new(8);
        assert_eq!(workspace.name(), "floating");
        assert_eq!(workspace.next(), "tiling");
        let one = workspace.open(1, (300, 200).into(), "one", screen());
        workspace.open(2, (200, 150).into(), "one", screen());
        let dragged = Rectangle::new((10, 10).into(), (300, 200).into());
        workspace.moved(&1, dragged, "one", screen());
        assert!(workspace.switch("tiling"));
        assert!(!workspace.switch("tiling"), "already tiling");
        assert!(!workspace.switch("spiral"), "no such policy");
        assert!(workspace.rearranges());
        let tiled = workspace.arrange(&one_screen());
        assert_eq!(tiled.len(), 2);
        assert!(!tiled[0].1.overlaps(tiled[1].1));
        // Dragging in tiling puts the window back; floating never hears it.
        assert_eq!(workspace.moved(&1, one, "one", screen()), tiled[0].1);
        assert!(workspace.switch("floating"));
        let floating = workspace.arrange(&one_screen());
        assert_eq!(floating[0], (1, dragged));
        assert_eq!(
            floating[1],
            (2, Rectangle::new((572, 357).into(), (200, 150).into()))
        );
        workspace.close(&1);
        assert!(workspace.switch("tiling"));
        assert_eq!(
            workspace.arrange(&one_screen()),
            [(2, Rectangle::new((8, 8).into(), (1264, 784).into()))]
        );
    }

    #[test]
    fn each_screen_places_its_own_windows() {
        let left = screen();
        let right = Rectangle::new((1280, 0).into(), (1024, 768).into());
        let both = vec![("one".to_string(), left), ("two".to_string(), right)];
        let mut workspace = Workspace::new(8);
        // Each opens centred on its own screen.
        let a = workspace.open(1, (300, 200).into(), "one", left);
        let b = workspace.open(2, (300, 200).into(), "two", right);
        assert_eq!(a.loc, (490, 300).into());
        assert_eq!(b.loc, (1280 + 362, 284).into());
        assert_eq!(workspace.screen_of(&2), Some("two"));
        // Tiled, each fills its own screen.
        workspace.switch("tiling");
        let tiled = workspace.arrange(&both);
        assert!(tiled.contains(&(1, Rectangle::new((8, 8).into(), (1264, 784).into()))));
        assert!(tiled.contains(&(2, Rectangle::new((1288, 8).into(), (1008, 752).into()))));
        // Floating, a window dragged onto the other screen moves there and
        // stays where it was put.
        workspace.switch("floating");
        let there = Rectangle::new((1400, 100).into(), (300, 200).into());
        assert_eq!(workspace.moved(&1, there, "two", right), there);
        assert_eq!(workspace.screen_of(&1), Some("two"));
        workspace.switch("tiling");
        assert_eq!(
            workspace
                .arrange(&both)
                .iter()
                .filter(|(_, at)| at.loc.x >= 1280)
                .count(),
            2
        );
        // The second screen goes: its windows join the first.
        workspace.switch("floating");
        let left_only = vec![("one".to_string(), left)];
        let placed = workspace.arrange(&left_only);
        assert_eq!(placed.len(), 2);
        assert!(placed.iter().all(|(_, at)| at.loc.x + at.size.w <= 1280));
        assert_eq!(workspace.screen_of(&2), Some("one"));
        assert!(workspace.arrange(&[]).is_empty(), "no screen, no places");
    }

    #[test]
    fn an_arrow_finds_the_window_beside_that_way() {
        let r = |x: i32, y: i32, w: i32, h: i32| Rectangle::new((x, y).into(), (w, h).into());
        // Split's four: 1 on the left half, 2 top right, 3 and 4 below it.
        let windows = [
            (1, r(8, 8, 628, 784)),
            (2, r(644, 8, 628, 388)),
            (3, r(644, 404, 310, 388)),
            (4, r(962, 404, 310, 388)),
        ];
        let from = |w: usize| windows[w - 1].1;
        let others = |w: u32| -> Vec<(u32, Rectangle<i32, Logical>)> {
            windows.iter().filter(|(x, _)| *x != w).cloned().collect()
        };
        assert_eq!(toward(from(4), &others(4), Direction::Left), Some(3));
        assert_eq!(toward(from(3), &others(3), Direction::Left), Some(1));
        assert_eq!(toward(from(4), &others(4), Direction::Up), Some(2));
        assert_eq!(toward(from(1), &others(1), Direction::Right), Some(2));
        assert_eq!(
            toward(from(2), &others(2), Direction::Down),
            Some(3),
            "a tie goes to the older"
        );
        assert_eq!(toward(from(4), &others(4), Direction::Right), None);
        assert_eq!(toward(from(1), &others(1), Direction::Left), None);
        assert_eq!(Direction::parse("up"), Some(Direction::Up));
        assert_eq!(Direction::parse("north"), None);
    }

    #[test]
    fn a_workspace_holds_any_policy() {
        let area = Rectangle::from_size((1280, 800).into());
        let mut policy: Box<dyn WindowPolicy<u32>> = Box::new(Fill(Vec::new()));
        assert_eq!(policy.open(1, Size::default(), area), area);
        policy.open(2, (300, 200).into(), area);
        policy.close(&1);
        let smaller = Rectangle::from_size((1024, 768).into());
        assert_eq!(policy.arrange(smaller), [(2, smaller)]);
        assert_eq!(policy.name(), "fill");
    }
}
