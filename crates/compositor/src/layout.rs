//! Outputs and window policies (ADR-002). Floating (M4.3) and dynamic
//! tiling (M4.5, `tiling.rs`) are two implementations of [`WindowPolicy`],
//! switchable per [`Workspace`], with title bars in both; nothing outside
//! this crate plugs into it (no extension API). A policy is one module and
//! one line in [`policies`]. The types here are plain data, so policies
//! are tested without a display.

use smithay::utils::{Logical, Physical, Point, Rectangle, Size};

use crate::tiling::Tiling;

/// Every policy, the default first: what `shell.tiling` and Super+T choose
/// between. Their names are what people see (ADR-008), never the
/// algorithm's, so a later tiling algorithm keeps everyone's setting.
pub fn policies<W: Clone + PartialEq + 'static>(gap: u32) -> Vec<Box<dyn WindowPolicy<W>>> {
    vec![Box::new(Floating::default()), Box::new(Tiling::new(gap))]
}

/// Where windows go on one workspace.
pub trait WindowPolicy<W> {
    /// The name `system.toml` and the state file use: `floating` or
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
}

/// One workspace: every policy follows its windows, and one of them, the
/// active one, places them. Switching re-lays the windows out at once
/// (ADR-002), and back in floating they are where they were.
pub struct Workspace<W> {
    policies: Vec<Box<dyn WindowPolicy<W>>>,
    active: usize,
}

impl<W: Clone + PartialEq + 'static> Workspace<W> {
    /// A floating workspace with every policy of [`policies`].
    pub fn new(gap: u32) -> Workspace<W> {
        Workspace {
            policies: policies(gap),
            active: 0,
        }
    }

    /// The active policy's name.
    pub fn name(&self) -> &'static str {
        self.policies[self.active].name()
    }

    /// Makes the policy called `name` the active one; false if there is
    /// none, or it already is.
    pub fn switch(&mut self, name: &str) -> bool {
        match self.policies.iter().position(|p| p.name() == name) {
            Some(i) if i != self.active => {
                self.active = i;
                true
            }
            _ => false,
        }
    }

    /// The next policy after the active one, for Super+T.
    pub fn next(&self) -> &'static str {
        self.policies[(self.active + 1) % self.policies.len()].name()
    }

    pub fn open(
        &mut self,
        window: W,
        wanted: Size<i32, Logical>,
        area: Rectangle<i32, Logical>,
    ) -> Rectangle<i32, Logical> {
        let mut place = Rectangle::default();
        for (i, policy) in self.policies.iter_mut().enumerate() {
            let at = policy.open(window.clone(), wanted, area);
            if i == self.active {
                place = at;
            }
        }
        place
    }

    pub fn close(&mut self, window: &W) {
        for policy in &mut self.policies {
            policy.close(window);
        }
    }

    pub fn moved(&mut self, window: &W, to: Rectangle<i32, Logical>) -> Rectangle<i32, Logical> {
        self.policies[self.active].moved(window, to)
    }

    pub fn arrange(&mut self, area: Rectangle<i32, Logical>) -> Vec<(W, Rectangle<i32, Logical>)> {
        self.policies[self.active].arrange(area)
    }

    pub fn rearranges(&self) -> bool {
        self.policies[self.active].rearranges()
    }
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

/// One screen as the compositor lays it out. M4.6 adds its position,
/// transform and whether it is on, from `[outputs.NAME]` in `system.toml`.
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
        let scale = if self.scale.is_finite() {
            self.scale
        } else {
            1.0
        };
        (scale.clamp(1.0, 4.0) * 120.0).round() as u32
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

    #[test]
    fn a_workspace_switches_policies_and_floating_keeps_its_places() {
        let mut workspace = Workspace::new(8);
        assert_eq!(workspace.name(), "floating");
        assert_eq!(workspace.next(), "tiling");
        let one = workspace.open(1, (300, 200).into(), screen());
        workspace.open(2, (200, 150).into(), screen());
        let dragged = Rectangle::new((10, 10).into(), (300, 200).into());
        workspace.moved(&1, dragged);
        assert!(workspace.switch("tiling"));
        assert!(!workspace.switch("tiling"), "already tiling");
        assert!(!workspace.switch("spiral"), "no such policy");
        assert!(workspace.rearranges());
        let tiled = workspace.arrange(screen());
        assert_eq!(tiled.len(), 2);
        assert!(!tiled[0].1.overlaps(tiled[1].1));
        // Dragging in tiling puts the window back; floating never hears it.
        assert_eq!(workspace.moved(&1, one), tiled[0].1);
        assert!(workspace.switch("floating"));
        let floating = workspace.arrange(screen());
        assert_eq!(floating[0], (1, dragged));
        assert_eq!(
            floating[1],
            (2, Rectangle::new((572, 357).into(), (200, 150).into()))
        );
        workspace.close(&1);
        assert!(workspace.switch("tiling"));
        assert_eq!(
            workspace.arrange(screen()),
            [(2, Rectangle::new((8, 8).into(), (1264, 784).into()))]
        );
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
