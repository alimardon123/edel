//! The widget table (M5.1c): every widget a preset's panel can hold, one
//! module and one line each. A widget says what it shows now, from what
//! shell-ui knows ([`Live`]), how wide it is and how it draws, and what a
//! click or a scroll on it does (M5.2c); the panel lays them out from the
//! preset and draws again only when what one shows changes. Code that talks to an OS
//! feature lives in `src/features/NAME.rs`, and its widget's line carries
//! `needs: Some("NAME")`: on a machine without
//! `/usr/share/edel/features/NAME.toml` the widget is skipped with a log
//! line (ADR-008). Its role and label are what screen readers say of it
//! (M5.1d). The tests fail when a line needs a feature with no file under
//! `features/`, and when a built-in preset names a widget missing here.

pub mod apps;
pub mod clock;
pub mod layout;
pub mod menu;
pub mod search;
pub mod separator;
pub mod status;
pub mod title;
pub mod tray;
pub mod windows;
pub mod workspaces;

use std::path::Path;

use accesskit::Role;
use tiny_skia::Pixmap;

use edel::tokens::Tokens;

use crate::paint::Text;

/// Where a widget draws: the panel's pixmap, with the panel's row in it.
pub struct Canvas<'a> {
    pub pixmap: &'a mut Pixmap,
    pub tokens: &'a Tokens,
    /// The fonts, if loaded; without them text measures and draws nothing.
    pub text: Option<&'a mut Text>,
    /// App icons, if read; without them apps show their initial (M5.4c).
    pub icons: Option<&'a mut edel::app_icons::Icons>,
    /// Buffer pixels per logical pixel.
    pub scale: f32,
    /// The panel's top row in the pixmap and its height, in its pixels.
    pub top: f32,
    pub height: f32,
    /// Whether the panel is a dock (M5.4d), whose widgets may draw bigger.
    pub dock: bool,
}

pub struct Widget {
    pub name: &'static str,
    /// The OS feature it talks to, if any.
    pub needs: Option<&'static str>,
    /// What it shows now: the panel draws again when this changes.
    pub shows: fn(&Live) -> String,
    /// Its width showing `shown`, in the pixmap's pixels.
    pub width: fn(&mut Canvas, shown: &str) -> f32,
    /// Draws it showing `shown`, its left edge at `x`.
    pub draw: fn(&mut Canvas, shown: &str, x: f32),
    /// What `input` on it does while it shows `shown`, if anything. The
    /// canvas is the panel's, to measure on as `width` does, for a
    /// widget whose parts' places depend on what text measures (the
    /// window list's buttons); it is drawn on by no one here.
    pub input: fn(&mut Canvas, shown: &str, input: Input) -> Option<Action>,
    /// The parts of it a screen reader reads one by one, such as each
    /// icon of the tray (M5.2e); most widgets are one thing, `no_parts`.
    pub parts: fn(shown: &str) -> Vec<Part>,
    /// What it is to a screen reader.
    pub role: Role,
    /// What a screen reader says it is, showing `shown`.
    pub label: fn(shown: &str) -> String,
}

/// What shell-ui knows that widgets show: the workspaces, by name, with
/// the shown one marked (ext-workspace-v1), where a scroll left the
/// workspace switcher's view, the windows on a screen
/// (wlr-foreign-toplevel-management), the shown workspace's policy
/// (edel-shell-v1), and the apps the preset pins and every app installed,
/// read at start (M5.4c).
#[derive(Debug, Default)]
pub struct Live {
    /// The tray's items, as their apps last said them (M5.2e).
    pub tray: Vec<crate::tray::Item>,
    pub workspaces: Vec<(String, bool)>,
    pub view: Option<usize>,
    pub windows: Vec<Task>,
    pub policy: String,
    /// Whether the launcher is open, for the menu button's lit tile.
    pub launcher: bool,
    /// Whether quick settings are open, for the status area's lit pill
    /// (M5.9a).
    pub quick: bool,
    /// Whether the notification centre is open, for the clock's lit
    /// button (M5.9b).
    pub centre: bool,
    /// How the machine is connected, how loud it is and how full its
    /// battery is (M5.9a); each part only where the machine has it.
    pub status: crate::status::Status,
    pub pinned: Vec<Pin>,
    pub installed: Vec<Pin>,
    /// What plays now, read while quick settings is open (M5.9d).
    pub player: Option<crate::mpris::Player>,
}

/// One part of a widget for a screen reader: what it is called and where
/// it lies, from the widget's left edge, in logical pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub label: String,
    pub x: f32,
    pub width: f32,
}

/// An app as the apps widget shows it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Pin {
    /// Its desktop file id.
    pub id: String,
    pub name: String,
    /// Its icon's name or path.
    pub icon: String,
}

impl From<&edel::apps::App> for Pin {
    /// An installed app; its id stands for its icon when it names none.
    fn from(app: &edel::apps::App) -> Pin {
        Pin {
            id: app.id.clone(),
            name: app.name.clone(),
            icon: app.icon.clone().unwrap_or_else(|| app.id.clone()),
        }
    }
}

/// A window, as the window list shows it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Task {
    pub title: String,
    /// Its app's id, to find its app's icon and pinned button (M5.4c).
    pub app_id: String,
    pub focused: bool,
    pub minimized: bool,
}

/// A person's input on a widget.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Input {
    /// A click this many logical pixels from its left edge, on the widget
    /// as wide as the second number.
    Click(f32, f32),
    /// A right click, the same way: a widget's menu (M5.16b).
    Menu(f32, f32),
    /// A scroll, in steps: positive towards the end.
    Scroll(i32),
}

/// What a widget asks shell-ui to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Show the workspace called this.
    Show(String),
    /// Start the workspace switcher's view at this button.
    View(usize),
    /// Bring the window at this place in [`Live::windows`] forward, back
    /// if it is minimized.
    Activate(usize),
    /// Minimize the window at this place in [`Live::windows`].
    Minimize(usize),
    /// Switch the shown workspace's policy, as Super+T.
    TogglePolicy,
    /// Open the launcher, or close it (M5.3b).
    Launcher,
    /// Open the tiling styles' menu beside the widget (M5.16b).
    Styles,
    /// Open quick settings above the status area, or close them (M5.9a).
    Quick,
    /// Open the notification centre over the clock, or close it (M5.9b).
    Centre,
    /// The app with this desktop file id: bring its window forward, or
    /// minimize it if it is the focused one, or start the app (M5.4c).
    App(String),
    /// The tray item with this id (`service/path`): its app is asked to
    /// do what a click does, or with `true` to open its menu (M5.2e).
    Tray(String, bool),
}

/// For widgets that take no input.
pub fn no_input(_: &mut Canvas, _: &str, _: Input) -> Option<Action> {
    None
}

/// For widgets that are one thing to a screen reader.
pub fn no_parts(_: &str) -> Vec<Part> {
    Vec::new()
}

/// Every widget, by name.
pub const TABLE: &[Widget] = &[
    menu::WIDGET,
    windows::WIDGET,
    workspaces::WIDGET,
    layout::WIDGET,
    tray::WIDGET,
    clock::WIDGET,
    title::WIDGET,
    apps::WIDGET,
    search::WIDGET,
    separator::WIDGET,
    status::WIDGET,
];

/// Scrolling added up but not yet a step: a high-resolution wheel's
/// 120ths and a touchpad's pixels, which come a little at a time.
#[derive(Debug, Default)]
pub struct Scrolled {
    v120: i32,
    pixels: f64,
}

impl Scrolled {
    /// The whole steps a scroll makes (a wheel's 120ths, else its notches,
    /// else 40 pixels a step), keeping the rest for the next.
    pub fn steps(&mut self, v120: i32, discrete: i32, pixels: f64) -> i32 {
        if v120 != 0 {
            self.v120 += v120;
            let n = self.v120 / 120;
            self.v120 -= n * 120;
            n
        } else if discrete != 0 {
            discrete
        } else {
            self.pixels += pixels;
            let n = (self.pixels / 40.0).trunc() as i32;
            self.pixels -= f64::from(n) * 40.0;
            n
        }
    }

    /// The pointer left: nothing carries over.
    pub fn reset(&mut self) {
        *self = Scrolled::default();
    }
}

/// The widget called `name`.
#[cfg(test)]
pub fn find(name: &str) -> Option<&'static Widget> {
    TABLE.iter().find(|w| w.name == name)
}

/// The widgets `names` lists that this machine can show, with a line for
/// each one skipped: one the table lacks (a preset from a later release)
/// or one needing a feature `features` (the machine's
/// `/usr/share/edel/features/`) has no file for.
pub fn usable(names: &[String], features: &Path) -> (Vec<&'static Widget>, Vec<String>) {
    pick(TABLE, names, features)
}

fn pick(
    table: &'static [Widget],
    names: &[String],
    features: &Path,
) -> (Vec<&'static Widget>, Vec<String>) {
    let mut found = Vec::new();
    let mut notes = Vec::new();
    for name in names {
        let Some(widget) = table.iter().find(|w| w.name == name) else {
            notes.push(format!(
                "the preset names a widget {name:?} this shell-ui does not have; it is skipped"
            ));
            continue;
        };
        match widget.needs {
            Some(feature) if !features.join(format!("{feature}.toml")).is_file() => {
                notes.push(format!(
                    "the widget {name} needs the feature {feature}, which this machine does not have; it is skipped"
                ));
            }
            _ => found.push(widget),
        }
    }
    (found, notes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_scrolls_add_up_to_steps() {
        let mut scrolled = Scrolled::default();
        // A high-resolution wheel: four quarter notches make one step.
        let wheel: i32 = (0..4).map(|_| scrolled.steps(30, 0, 0.0)).sum();
        assert_eq!(wheel, 1);
        assert_eq!(scrolled.steps(-120, 0, 0.0), -1);
        // A touchpad: twelve pixels at a time, a step every 40.
        let pad: Vec<i32> = (0..7).map(|_| scrolled.steps(0, 0, 12.0)).collect();
        assert_eq!(pad.iter().sum::<i32>(), 2);
        assert_eq!(scrolled.steps(0, 2, 0.0), 2, "notches as they are");
        scrolled.reset();
        assert_eq!(scrolled.steps(0, 0, 39.0), 0);
    }

    fn repository() -> &'static Path {
        Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
    }

    #[test]
    fn names_are_unique_and_every_needed_feature_has_a_file() {
        for (i, widget) in TABLE.iter().enumerate() {
            assert!(edel::features::is_name(widget.name), "{}", widget.name);
            assert!(
                TABLE[..i].iter().all(|w| w.name != widget.name),
                "two widgets called {}",
                widget.name
            );
            if let Some(feature) = widget.needs {
                let file = repository().join(format!("features/{feature}.toml"));
                assert!(
                    file.is_file(),
                    "the widget {} needs the feature {feature}, which has no {}",
                    widget.name,
                    file.display()
                );
            }
        }
    }

    #[test]
    fn every_built_in_preset_names_only_widgets_in_the_table() {
        for (name, _) in edel::presets::BUILT_IN {
            let (preset, _) = edel::presets::named(Some(name));
            for panel in &preset.panels {
                for widget in panel.widgets() {
                    assert!(
                        find(widget).is_some(),
                        "presets/{name}.toml names the widget {widget}, which the table lacks"
                    );
                }
            }
        }
    }

    #[test]
    fn a_widget_is_skipped_without_its_feature_or_its_line() {
        fn none(_: &Live) -> String {
            String::new()
        }
        fn zero(_: &mut Canvas, _: &str) -> f32 {
            0.0
        }
        fn nothing(_: &mut Canvas, _: &str, _: f32) {}
        // A table with a stand-in for a widget that talks to an OS feature.
        static TEST: [Widget; 2] = [
            menu::WIDGET,
            Widget {
                name: "battery",
                needs: Some("power"),
                shows: none,
                width: zero,
                draw: nothing,
                input: no_input,
                parts: no_parts,
                role: Role::Label,
                label: str::to_owned,
            },
        ];
        let dir = std::env::temp_dir().join(format!("edel-shell-ui-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let names = ["menu", "weather", "battery"].map(String::from);
        let names_of = |found: &[&Widget]| found.iter().map(|w| w.name).collect::<Vec<_>>();
        // No power.toml: battery is skipped, and weather, which the table
        // lacks; menu stays.
        let (found, notes) = pick(&TEST, &names, &dir);
        assert_eq!(names_of(&found), ["menu"]);
        assert_eq!(notes.len(), 2, "{notes:?}");
        assert!(notes[0].contains("\"weather\""), "{}", notes[0]);
        assert!(notes[1].contains("needs the feature power"), "{}", notes[1]);
        // With it, battery is shown, after menu as the preset says.
        std::fs::write(dir.join("power.toml"), "format = 1\n").unwrap();
        let (found, notes) = pick(&TEST, &names, &dir);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(names_of(&found), ["menu", "battery"]);
        assert_eq!(notes.len(), 1);
    }
}
