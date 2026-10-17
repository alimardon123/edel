//! The catalogue of widgets (M5.33b, ADR-010): the one owner of each widget's
//! name, title, surfaces and options. shell-ui's panel table and
//! `lock_screen::Part` read it, so a fact is written here once. A new widget
//! is one entry here and one drawing module per surface it fits. Placing a
//! widget is a level 1 setting, never a plugin (ADR-007).

use crate::i18n::n_;

/// Where a widget can stand (M5.33b).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    /// A panel of the desktop.
    Panel,
    /// The lock and login screens.
    LockScreen,
    /// The desktop itself (M5.33c); no entry claims it yet.
    Desktop,
}

/// One widget: what it is called, what it is titled, where it stands and
/// the options its placed item takes in the settings file.
#[derive(Debug, PartialEq, Eq)]
pub struct Widget {
    /// The name the settings file writes it by, such as `clock`.
    pub name: &'static str,
    /// What Settings, the editor's drawer and a screen reader call it.
    pub title: &'static str,
    /// The surfaces it can stand on.
    pub surfaces: &'static [Surface],
    /// The options a placed item of it takes, by their key names.
    pub options: &'static [&'static str],
}

impl Widget {
    /// Whether the widget can stand on surface `s`.
    pub fn fits(&self, s: Surface) -> bool {
        self.surfaces.contains(&s)
    }

    /// Whether a placed item of the widget takes `option`.
    pub fn takes(&self, option: &str) -> bool {
        self.options.contains(&option)
    }
}

/// The menu button of a panel.
pub const MENU: Widget = Widget {
    name: "menu",
    title: n_("Menu"),
    surfaces: &[Surface::Panel],
    options: &[],
};

/// The window list.
pub const WINDOWS: Widget = Widget {
    name: "windows",
    title: n_("Windows"),
    surfaces: &[Surface::Panel],
    options: &[],
};

/// The workspace switcher.
pub const WORKSPACES: Widget = Widget {
    name: "workspaces",
    title: n_("Workspaces"),
    surfaces: &[Surface::Panel],
    options: &[],
};

/// The floating or tiling toggle.
pub const LAYOUT: Widget = Widget {
    name: "layout",
    title: n_("Floating or tiling"),
    surfaces: &[Surface::Panel],
    options: &[],
};

/// The keyboard layout indicator.
pub const KEYBOARD: Widget = Widget {
    name: "keyboard",
    title: n_("Keyboard layout"),
    surfaces: &[Surface::Panel],
    options: &[],
};

/// The tray of apps' icons.
pub const TRAY: Widget = Widget {
    name: "tray",
    title: n_("Tray"),
    surfaces: &[Surface::Panel],
    options: &[],
};

/// The clock: on a panel, and on the lock screen as one of its clocks.
pub const CLOCK: Widget = Widget {
    name: "clock",
    title: n_("Clock"),
    surfaces: &[Surface::Panel, Surface::LockScreen],
    options: &["face", "hours", "size", "weight", "zone", "label"],
};

/// The focused window's title.
pub const TITLE: Widget = Widget {
    name: "title",
    title: n_("Window title"),
    surfaces: &[Surface::Panel],
    options: &[],
};

/// The apps, as a taskbar shows them.
pub const APPS: Widget = Widget {
    name: "apps",
    title: n_("Apps"),
    surfaces: &[Surface::Panel],
    options: &[],
};

/// The search pill that opens the launcher.
pub const SEARCH: Widget = Widget {
    name: "search",
    title: n_("Search"),
    surfaces: &[Surface::Panel],
    options: &[],
};

/// A thin line between groups of a panel.
pub const SEPARATOR: Widget = Widget {
    name: "separator",
    title: n_("Separator"),
    surfaces: &[Surface::Panel],
    options: &[],
};

/// The status area: network, volume and battery.
pub const STATUS: Widget = Widget {
    name: "status",
    title: n_("Status"),
    surfaces: &[Surface::Panel],
    options: &[],
};

/// The notifications button.
pub const NOTIFICATIONS: Widget = Widget {
    name: "notifications",
    title: n_("Notifications"),
    surfaces: &[Surface::Panel],
    options: &[],
};

/// The date on the lock screen: its parts and its style.
pub const DATE: Widget = Widget {
    name: "date",
    title: n_("Date"),
    surfaces: &[Surface::LockScreen],
    options: &["show", "style"],
};

/// How many notifications wait, on the lock screen.
pub const COUNT: Widget = Widget {
    name: "count",
    title: n_("Notification count"),
    surfaces: &[Surface::LockScreen],
    options: &[],
};

/// What plays now, with its buttons, on the lock screen.
pub const PLAYER: Widget = Widget {
    name: "player",
    title: n_("Music player"),
    surfaces: &[Surface::LockScreen],
    options: &[],
};

/// The person: their name and the field their password is typed in.
pub const PERSON: Widget = Widget {
    name: "person",
    title: n_("Sign in"),
    surfaces: &[Surface::LockScreen],
    options: &[],
};

/// Words of the person's own, on the lock screen.
pub const MESSAGE: Widget = Widget {
    name: "message",
    title: n_("Message"),
    surfaces: &[Surface::LockScreen],
    options: &["text"],
};

/// A picture, on the lock screen.
pub const PICTURE: Widget = Widget {
    name: "picture",
    title: n_("Picture"),
    surfaces: &[Surface::LockScreen],
    options: &["path"],
};

/// The battery's charge, on the lock screen.
pub const BATTERY: Widget = Widget {
    name: "battery",
    title: n_("Battery"),
    surfaces: &[Surface::LockScreen],
    options: &[],
};

/// Every widget, panel ones first, then the lock screen's.
pub const CATALOGUE: &[Widget] = &[
    MENU,
    WINDOWS,
    WORKSPACES,
    LAYOUT,
    KEYBOARD,
    TRAY,
    CLOCK,
    TITLE,
    APPS,
    SEARCH,
    SEPARATOR,
    STATUS,
    NOTIFICATIONS,
    DATE,
    COUNT,
    PLAYER,
    PERSON,
    MESSAGE,
    PICTURE,
    BATTERY,
];

/// The widget called `name`, if the catalogue has one.
pub fn named(name: &str) -> Option<&'static Widget> {
    CATALOGUE.iter().find(|w| w.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock_screen::{Anchor, DatePart, DateStyle, Face, Part, Placed, Size};
    use std::collections::BTreeSet;

    #[test]
    fn names_are_unique_each_fits_a_surface_and_none_fits_the_desktop_yet() {
        for (i, widget) in CATALOGUE.iter().enumerate() {
            assert!(crate::features::is_name(widget.name), "{}", widget.name);
            assert!(
                CATALOGUE[..i].iter().all(|w| w.name != widget.name),
                "two widgets called {}",
                widget.name
            );
            assert!(
                widget.fits(Surface::Panel) || widget.fits(Surface::LockScreen),
                "{} fits no surface",
                widget.name
            );
            assert!(
                !widget.fits(Surface::Desktop),
                "{} fits the desktop, which has no entry until M5.33c",
                widget.name
            );
        }
    }

    #[test]
    fn every_lock_screen_part_is_an_entry_and_every_entry_for_the_lock_is_a_part() {
        for part in Part::ALL {
            assert!(
                named(part.name()).is_some_and(|w| w.fits(Surface::LockScreen)),
                "the lock screen's {} is not an entry that fits it",
                part.name()
            );
        }
        for widget in CATALOGUE.iter().filter(|w| w.fits(Surface::LockScreen)) {
            assert!(
                Part::named(widget.name).is_some(),
                "{} fits the lock screen but is no lock_screen::Part",
                widget.name
            );
        }
    }

    #[test]
    fn a_placed_part_writes_exactly_the_options_the_catalogue_names() {
        let placed = Placed {
            part: Part::Clock,
            at: Anchor::Top,
            x: 0.1,
            y: 0.2,
            text: Some("Hi".into()),
            path: Some("/a.png".into()),
            face: Some(Face::Digital),
            hours: Some(24),
            size: Some(Size::Large),
            weight: Some(300),
            show: Some(vec![DatePart::Weekday]),
            style: Some(DateStyle::Long),
            zone: Some("Asia/Tokyo".into()),
            label: Some("Home".into()),
        };
        let value = toml::Value::try_from(&placed).unwrap();
        // widget, at, x and y are written for every part, not options.
        let keys: BTreeSet<&str> = value
            .as_table()
            .unwrap()
            .keys()
            .map(String::as_str)
            .filter(|key| !["widget", "at", "x", "y"].contains(key))
            .collect();
        let options: BTreeSet<&str> = CATALOGUE
            .iter()
            .flat_map(|w| w.options.iter().copied())
            .collect();
        assert_eq!(keys, options);
    }

    #[test]
    fn every_widget_a_built_in_preset_names_is_an_entry_that_fits_a_panel() {
        for (name, _) in crate::presets::BUILT_IN {
            let (preset, _) = crate::presets::named(Some(name));
            for panel in &preset.panels {
                for widget in panel.widgets() {
                    assert!(
                        named(widget).is_some_and(|w| w.fits(Surface::Panel)),
                        "presets/{name}.toml names the widget {widget}, which is no panel entry"
                    );
                }
            }
        }
    }
}
