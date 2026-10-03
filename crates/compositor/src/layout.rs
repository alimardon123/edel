//! Outputs and window policies (ADR-002). Floating (M4.3) and dynamic
//! tiling (M4.5) are two implementations of [`WindowPolicy`], switchable
//! per workspace, with title bars in both; nothing outside this crate
//! plugs into it (no extension API). The types here are plain data, so
//! policies are tested without a display.

use smithay::utils::{Logical, Physical, Rectangle, Size};

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

    /// Every window's place, bottom of the stack first, after `area`
    /// changed: an output added, removed, resized or rescaled.
    fn arrange(&mut self, area: Rectangle<i32, Logical>) -> Vec<(W, Rectangle<i32, Logical>)>;
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
        fn arrange(
            &mut self,
            area: Rectangle<i32, Logical>,
        ) -> Vec<(u32, Rectangle<i32, Logical>)> {
            self.0.iter().map(|w| (*w, area)).collect()
        }
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
