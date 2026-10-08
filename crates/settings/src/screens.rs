//! The screens the Displays page lists (M5.7a), with no toolkit in it so a
//! test can hold every rule: which screens there are, the values each
//! shows, the scales offered, and where each is drawn in the arrangement.
//!
//! The list is what the compositor reports in the state file
//! (`edel::places::STATE_FILE`, its `[[outputs]]`), and the settings
//! files' `[displays]` for what the state cannot say: a screen turned off
//! is not in it, and without a session there is no state at all. So
//! nothing here assumes a screen is local or that there are two: a
//! device paired later (ADR-011, M7.10d) is one more entry that comes the
//! same way.

use std::collections::BTreeMap;

use edel::settings::Display;

/// The scales offered, in percent, as the roadmap names them.
pub const SCALES: [u32; 5] = [100, 125, 150, 175, 200];

/// One screen as the page shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Screen {
    /// The connector's name, such as `eDP-1`, which the settings file
    /// and the state file use.
    pub name: String,
    /// Whether the compositor reports it: lit now.
    pub lit: bool,
    /// Whether the files let it be on (`enabled`, true when absent).
    pub on: bool,
    /// The scale it has, or is asked to have.
    pub scale: f64,
    /// The scale it gets when the files name none, as the compositor
    /// worked it out from the screen's size.
    pub auto_scale: Option<f64>,
    /// The size in pixels it runs at, `WIDTHxHEIGHT`.
    pub mode: Option<String>,
    /// Every size it has, the largest first.
    pub modes: Vec<String>,
    /// The size it runs at when the files name none.
    pub preferred: Option<String>,
    /// Its top left in the layout and its size there, in logical pixels.
    pub place: Option<Place>,
}

/// A rectangle in the layout, in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Place {
    pub x: i64,
    pub y: i64,
    pub w: i64,
    pub h: i64,
}

fn text(table: &toml::Table, key: &str) -> Option<String> {
    table.get(key)?.as_str().map(String::from)
}

fn number(table: &toml::Table, key: &str) -> Option<i64> {
    table.get(key)?.as_integer()
}

fn float(table: &toml::Table, key: &str) -> Option<f64> {
    let value = table.get(key)?;
    value
        .as_float()
        .or_else(|| value.as_integer().map(|n| n as f64))
}

/// The screens to list, from the state file's text (none without a
/// session) and the `[displays]` the files give, the person's over the
/// machine's. Lit screens come first, left to right, then those the files
/// turned off, by name; with no state every screen the files name is
/// listed, since there is nothing else to say what exists. A screen in
/// the files that is on but not lit is unplugged, and is left out while a
/// session runs.
pub fn list(state: Option<&str>, files: &BTreeMap<String, Display>) -> Vec<Screen> {
    let table: Option<toml::Table> = state.and_then(|s| s.parse().ok());
    let mut lit: Vec<Screen> = table
        .iter()
        .filter_map(|t| t.get("outputs")?.as_array())
        .flatten()
        .filter_map(|o| o.as_table())
        .filter_map(|o| {
            let name = text(o, "name")?;
            let own = files.get(&name);
            let state_scale = float(o, "scale").unwrap_or(1.0);
            Some(Screen {
                on: own.and_then(|d| d.enabled).unwrap_or(true),
                scale: own.and_then(|d| d.scale).unwrap_or(state_scale),
                auto_scale: float(o, "auto_scale"),
                // What the files ask for, until the compositor has done it.
                mode: own.and_then(|d| d.resolution.clone()).or(text(o, "mode")),
                modes: o
                    .get("modes")
                    .and_then(|m| m.as_array())
                    .map(|m| {
                        m.iter()
                            .filter_map(|v| v.as_str())
                            .map(String::from)
                            .collect()
                    })
                    .unwrap_or_default(),
                preferred: text(o, "preferred_mode"),
                place: Some(Place {
                    x: own
                        .and_then(|d| d.position)
                        .map_or_else(|| number(o, "x").unwrap_or(0), |p| p[0]),
                    y: own
                        .and_then(|d| d.position)
                        .map_or_else(|| number(o, "y").unwrap_or(0), |p| p[1]),
                    w: number(o, "width").unwrap_or(0),
                    h: number(o, "height").unwrap_or(0),
                }),
                lit: true,
                name,
            })
        })
        .collect();
    lit.sort_by(|a, b| {
        let at = |s: &Screen| s.place.map_or(0, |p| p.x);
        (at(a), &a.name).cmp(&(at(b), &b.name))
    });
    let known = table.is_some();
    let mut dark: Vec<Screen> = files
        .iter()
        .filter(|(name, d)| {
            !lit.iter().any(|s| &s.name == *name) && (!known || d.enabled == Some(false))
        })
        .map(|(name, d)| Screen {
            name: name.clone(),
            lit: false,
            on: d.enabled.unwrap_or(true),
            scale: d.scale.unwrap_or(1.0),
            auto_scale: None,
            mode: d.resolution.clone(),
            modes: Vec::new(),
            preferred: None,
            place: None,
        })
        .collect();
    lit.append(&mut dark);
    lit
}

/// The scales to offer a screen at `current`: the five percentages, and
/// `current` as well when it is none of them (a file may say 2.5), in order.
pub fn scale_choices(current: f64) -> Vec<f64> {
    let mut scales: Vec<f64> = SCALES.iter().map(|p| f64::from(*p) / 100.0).collect();
    if !scales.iter().any(|s| same(*s, current)) {
        scales.push(current);
        scales.sort_by(f64::total_cmp);
    }
    scales
}

/// Whether two scales are the same to the compositor, which keeps them to
/// 1/120.
pub fn same(a: f64, b: f64) -> bool {
    (a - b).abs() < 1.0 / 240.0
}

/// A scale as people read it: `125%`.
pub fn percent(scale: f64) -> String {
    format!("{}%", (scale * 100.0).round() as i64)
}

/// A scale as `edel settings set` takes it: `1.25`, `2`.
pub fn scale_value(scale: f64) -> String {
    format!("{}", (scale * 120.0).round() / 120.0)
}

/// The sizes to offer: those the screen reports, plus `current` when it
/// is not one of them (the files may name one the screen only gains
/// later); just `current` when the screen reports none.
pub fn mode_choices(screen: &Screen) -> Vec<String> {
    let mut modes = screen.modes.clone();
    if let Some(current) = &screen.mode {
        if !modes.contains(current) {
            modes.push(current.clone());
        }
    }
    modes
}

/// Where each placed screen is drawn in a box of `width` by `height`
/// pixels, all together scaled by one factor so their shapes and gaps
/// keep their proportions, centred, with `pad` pixels round them. Screens
/// without a place are not drawn; the result has one entry per screen, in
/// order.
pub fn arrange(screens: &[Screen], width: f64, height: f64, pad: f64) -> Vec<Option<[f64; 4]>> {
    let places: Vec<Place> = screens.iter().filter_map(|s| s.place).collect();
    let Some(left) = places.iter().map(|p| p.x).min() else {
        return vec![None; screens.len()];
    };
    let top = places.iter().map(|p| p.y).min().unwrap_or(0);
    let right = places.iter().map(|p| p.x + p.w).max().unwrap_or(left);
    let bottom = places.iter().map(|p| p.y + p.h).max().unwrap_or(top);
    let (span_x, span_y) = (
        ((right - left).max(1)) as f64,
        ((bottom - top).max(1)) as f64,
    );
    let factor = ((width - 2.0 * pad) / span_x).min((height - 2.0 * pad) / span_y);
    let factor = factor.max(0.0);
    let (used_x, used_y) = (span_x * factor, span_y * factor);
    let (start_x, start_y) = ((width - used_x) / 2.0, (height - used_y) / 2.0);
    screens
        .iter()
        .map(|s| {
            let p = s.place?;
            Some([
                start_x + (p.x - left) as f64 * factor,
                start_y + (p.y - top) as f64 * factor,
                p.w as f64 * factor,
                p.h as f64 * factor,
            ])
        })
        .collect()
}

/// The logical size of a mode at a scale, as the compositor divides it:
/// `1920x1080` at 1.5 is 1280 by 720.
pub fn logical(mode: &str, scale: f64) -> Option<(i64, i64)> {
    let (w, h) = mode.split_once('x')?;
    let (w, h): (f64, f64) = (w.parse().ok()?, h.parse().ok()?);
    Some(((w / scale).round() as i64, (h / scale).round() as i64))
}

/// How many screens are on, so the last one is never turned off.
pub fn on_count(screens: &[Screen]) -> usize {
    screens.iter().filter(|s| s.on).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATE: &str = r#"
format = 1
policy = "floating"

[[outputs]]
name = "Virtual-2"
scale = 1.0
x = 1280
y = 0
width = 1024
height = 768
mode = "1024x768"
modes = ["1280x800", "1024x768"]

[[outputs]]
name = "Virtual-1"
scale = 1.0
auto_scale = 1.0
x = 0
y = 0
width = 1280
height = 800
mode = "1280x800"
preferred_mode = "1280x800"
modes = ["1920x1200", "1280x800", "1024x768"]
"#;

    fn files(text: &str) -> BTreeMap<String, Display> {
        edel::settings::read(text).unwrap().file.displays
    }

    #[test]
    fn the_screens_are_what_the_compositor_reports_left_to_right() {
        let screens = list(Some(STATE), &BTreeMap::new());
        let names: Vec<&str> = screens.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Virtual-1", "Virtual-2"]);
        let first = &screens[0];
        assert!(first.lit && first.on);
        assert_eq!(first.mode.as_deref(), Some("1280x800"));
        assert_eq!(first.modes, ["1920x1200", "1280x800", "1024x768"]);
        assert_eq!(first.preferred.as_deref(), Some("1280x800"));
        assert_eq!(first.auto_scale, Some(1.0));
        assert_eq!(
            first.place,
            Some(Place {
                x: 0,
                y: 0,
                w: 1280,
                h: 800
            })
        );
    }

    #[test]
    fn the_files_say_what_the_state_cannot() {
        let files = files(
            "format = 1\n[displays.Virtual-1]\nscale = 2\nposition = [10, 20]\n\
             [displays.Virtual-3]\nenabled = false\nresolution = \"800x600\"\n\
             [displays.Gone-1]\nscale = 1.5\n",
        );
        let screens = list(Some(STATE), &files);
        let names: Vec<&str> = screens.iter().map(|s| s.name.as_str()).collect();
        // Virtual-1 is moved by its file; Virtual-3 is off, so only the
        // files know it; Gone-1 is unplugged and left out.
        assert_eq!(names, ["Virtual-1", "Virtual-2", "Virtual-3"]);
        assert_eq!(screens[0].scale, 2.0);
        assert_eq!(screens[0].mode.as_deref(), Some("1280x800"));
        assert_eq!(screens[0].place.map(|p| (p.x, p.y)), Some((10, 20)));
        let off = &screens[2];
        assert!(!off.lit && !off.on);
        assert_eq!(off.mode.as_deref(), Some("800x600"));
        assert_eq!(on_count(&screens), 2);
    }

    #[test]
    fn without_a_session_the_files_are_all_there_is() {
        let files = files("format = 1\n[displays.eDP-1]\nscale = 1.25\n[displays.DP-1]\n");
        let screens = list(None, &files);
        let names: Vec<&str> = screens.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["DP-1", "eDP-1"]);
        assert!(screens.iter().all(|s| !s.lit && s.place.is_none()));
        assert_eq!(screens[1].scale, 1.25);
        assert!(list(None, &BTreeMap::new()).is_empty());
        // A state that is not TOML is no state.
        assert!(list(Some("not toml ["), &BTreeMap::new()).is_empty());
    }

    #[test]
    fn the_scales_are_five_percentages_and_the_current_one() {
        assert_eq!(scale_choices(1.0), [1.0, 1.25, 1.5, 1.75, 2.0]);
        assert_eq!(scale_choices(2.5), [1.0, 1.25, 1.5, 1.75, 2.0, 2.5]);
        assert_eq!(scale_choices(1.375), [1.0, 1.25, 1.375, 1.5, 1.75, 2.0]);
        // The compositor keeps 1/120, so 1.2501 is 1.25.
        assert_eq!(scale_choices(1.2501), [1.0, 1.25, 1.5, 1.75, 2.0]);
        let shown: Vec<String> = scale_choices(2.5).into_iter().map(percent).collect();
        assert_eq!(shown, ["100%", "125%", "150%", "175%", "200%", "250%"]);
    }

    #[test]
    fn a_scale_is_written_as_edel_settings_set_takes_it() {
        assert_eq!(scale_value(2.0), "2");
        assert_eq!(scale_value(1.25), "1.25");
        assert_eq!(scale_value(1.75), "1.75");
        assert_eq!(scale_value(1.0), "1");
        for scale in scale_choices(2.5) {
            let value = scale_value(scale);
            assert!(
                edel::settings::set("format = 1\n", "displays.Virtual-1.scale", &value).is_ok(),
                "{value}"
            );
        }
    }

    #[test]
    fn a_size_the_screen_lacks_is_still_offered_when_it_is_in_use() {
        let mut screen = list(Some(STATE), &BTreeMap::new()).remove(0);
        assert_eq!(mode_choices(&screen), screen.modes);
        screen.mode = Some("800x600".into());
        assert_eq!(
            mode_choices(&screen).last().map(String::as_str),
            Some("800x600")
        );
        screen.modes.clear();
        assert_eq!(mode_choices(&screen), ["800x600"]);
        screen.mode = None;
        assert!(mode_choices(&screen).is_empty());
    }

    #[test]
    fn the_arrangement_keeps_proportions_and_centres() {
        let screens = list(Some(STATE), &BTreeMap::new());
        let drawn = arrange(&screens, 600.0, 150.0, 10.0);
        let [x1, y1, w1, h1] = drawn[0].unwrap();
        let [x2, y2, w2, h2] = drawn[1].unwrap();
        // 2304 by 800 logical pixels in 580 by 130: the height decides.
        let factor = 130.0 / 800.0;
        assert!((w1 - 1280.0 * factor).abs() < 1e-9 && (h1 - 800.0 * factor).abs() < 1e-9);
        assert!((w2 - 1024.0 * factor).abs() < 1e-9);
        // Side by side, touching, with the same top.
        assert!((x1 + w1 - x2).abs() < 1e-9 && (y1 - y2).abs() < 1e-9);
        // Centred across and down.
        let used = 2304.0 * factor;
        assert!((x1 - (600.0 - used) / 2.0).abs() < 1e-9);
        assert!((y1 - (150.0 - h1.max(h2)) / 2.0).abs() < 1e-9);
        // No screen placed: nothing to draw.
        let none = arrange(
            &list(None, &files("format = 1\n[displays.A-1]\n")),
            600.0,
            150.0,
            10.0,
        );
        assert_eq!(none, [None]);
        assert!(arrange(&[], 600.0, 150.0, 10.0).is_empty());
        // A box too small for its padding draws nothing larger than a point.
        let tiny = arrange(&screens, 10.0, 10.0, 10.0);
        assert_eq!(tiny[0].map(|r| r[2]), Some(0.0));
    }

    #[test]
    fn a_mode_at_a_scale_is_the_logical_size() {
        assert_eq!(logical("1920x1080", 1.5), Some((1280, 720)));
        assert_eq!(logical("1280x800", 2.0), Some((640, 400)));
        assert_eq!(logical("wide", 1.0), None);
    }
}
