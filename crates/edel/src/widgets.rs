//! The catalogue of widgets (M5.33b, ADR-010): the one owner of each widget's
//! name, title, surfaces and options. shell-ui's panel table and
//! `lock_screen::Part` read it, so a fact is written here once. A new widget
//! is one entry here and one drawing module per surface it fits. Placing a
//! widget is a level 1 setting, never a plugin (ADR-007).

use std::path::{Path, PathBuf};

use crate::i18n::{n_, tr, trf};
use crate::lock_screen::Background;
use crate::tokens::{
    self, Colour, MARK_CONTRAST, TEXT_CONTRAST, Tokens, contrast, nearest_passing,
};
use crate::wallpapers::{self, Wallpaper};

/// Where a widget can stand (M5.33b).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    /// A panel of the desktop.
    Panel,
    /// The lock and login screens.
    LockScreen,
    /// The desktop itself, over the wallpaper and under the windows (M5.33c,
    /// M8.21 part 21j1).
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

/// The option that takes a colour (M5.33d): the clock, the date, a message
/// and the battery, the widgets drawn on the wallpaper. The count, the
/// player, the person and a picture stand on their own glass and take none.
pub const COLOUR: &str = "colour";

/// The colour that follows the accent (M5.33d), with `wallpaper` and the
/// named accents and `#rrggbb` that [`is_colour`] also takes.
pub const ACCENT: &str = "accent";

/// The clock: on a panel, on the lock screen as one of its clocks, and on the
/// desktop (M8.21 part 21j1).
pub const CLOCK: Widget = Widget {
    name: "clock",
    title: n_("Clock"),
    surfaces: &[Surface::Panel, Surface::LockScreen, Surface::Desktop],
    options: &["face", "hours", "size", "weight", "zone", "label", COLOUR],
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

/// The date on the lock screen and on the desktop: its parts and its style.
pub const DATE: Widget = Widget {
    name: "date",
    title: n_("Date"),
    surfaces: &[Surface::LockScreen, Surface::Desktop],
    options: &["show", "style", COLOUR],
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
    options: &["text", COLOUR],
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
    options: &[COLOUR],
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

/// Where a widget's ink is held to (M5.33d): the surface behind it, and the
/// colour a `wallpaper` colour takes, when the wallpaper gives one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Behind {
    pub surface: Colour,
    pub source: Option<Colour>,
}

/// Whether `v` is a colour a widget's `colour` takes (M5.33d): `accent`, an
/// accent's name, `wallpaper` or `#rrggbb`.
pub fn is_colour(v: &str) -> bool {
    v == ACCENT || tokens::is_accent(v)
}

/// Whether a widget's ink is large text (M5.33d): the clock's figures are,
/// and the rest is text.
fn is_large(w: &Widget) -> bool {
    w.name == CLOCK.name
}

/// The contrast a widget's ink must reach over its surface (M5.33d):
/// [`MARK_CONTRAST`] for the clock, [`TEXT_CONTRAST`] for the rest.
pub fn ratio(w: &Widget) -> f32 {
    if is_large(w) {
        MARK_CONTRAST
    } else {
        TEXT_CONTRAST
    }
}

/// The colour `v` names before it is fitted to read (M5.33d): the accent,
/// a named accent in the scheme of `tokens`, or `#rrggbb`. None for
/// `wallpaper`, which takes the wallpaper's own, and for anything else.
fn chosen(v: &str, tokens: &Tokens) -> Option<Colour> {
    if v == ACCENT {
        Some(tokens.accent)
    } else if let Some(colour) = tokens::accent_named(v, tokens.scheme) {
        Some(colour)
    } else if v.len() == tokens::SIX_DIGIT_LEN && v.starts_with('#') {
        Colour::parse(v)
    } else {
        None
    }
}

/// The ink a widget draws in (M5.33d), from its `colour`: None is automatic,
/// and the caller draws the release's text colour; `wallpaper` takes the
/// wallpaper's colour, and is automatic when there is none; anything else
/// is [`chosen`]. Every colour is fitted to read over `behind`
/// ([`nearest_passing`]), so one that does not pass is drawn as the nearest
/// that does.
pub fn ink(value: Option<&str>, w: &Widget, tokens: &Tokens, behind: Behind) -> Option<Colour> {
    let colour = match value? {
        tokens::WALLPAPER_ACCENT => behind.source?,
        v => chosen(v, tokens)?,
    };
    Some(nearest_passing(colour, behind.surface, ratio(w)))
}

/// What lies behind a widget on `on` (M5.33d). On the desktop it is the
/// wallpaper `desktop` gives; on the lock screen it is `wallpaper`, the
/// look's, and blurred or sharp the wallpaper that applies (`desktop`).
/// A colour is its own surface and source, dimmed by `dim` percent towards
/// `lock_backdrop_deep` as the lock screen dims it. A picture's surface is
/// `backdrop_deep`, the surface the release holds its own text to, and its
/// source is `picture`'s colour for it. The checker reads no pictures and
/// passes a `picture` that gives None, so a picture has no source there.
pub fn behind(
    on: Surface,
    wallpaper: &Background,
    dim: u32,
    desktop: &Wallpaper,
    tokens: &Tokens,
    picture: impl Fn(&Path) -> Option<Colour>,
) -> Behind {
    if on == Surface::Desktop {
        return match desktop {
            Wallpaper::Colour(colour) => Behind {
                surface: *colour,
                source: Some(*colour),
            },
            Wallpaper::Picture(path) => Behind {
                surface: tokens.backdrop_deep,
                source: picture(path.as_path()),
            },
            Wallpaper::Background => Behind {
                surface: tokens.background,
                source: None,
            },
        };
    }
    let dimmed = |colour: Colour| colour.towards(tokens.lock_backdrop_deep, dim as f32 / 100.0);
    let applies = match wallpaper {
        Background::Backdrop => None,
        Background::Colour(colour) => Some(Wallpaper::Colour(*colour)),
        Background::Picture(path) => Some(Wallpaper::Picture(path.clone())),
        Background::Blurred | Background::Sharp => Some(desktop.clone()),
    };
    match applies {
        Some(Wallpaper::Colour(colour)) => Behind {
            surface: dimmed(colour),
            source: Some(colour),
        },
        Some(Wallpaper::Picture(path)) => Behind {
            surface: dimmed(tokens.backdrop_deep),
            source: picture(path.as_path()),
        },
        Some(Wallpaper::Background) | None => Behind {
            surface: tokens.lock_backdrop,
            source: None,
        },
    }
}

/// The desktop's wallpaper as the checker reads it, with no disk (M5.33d):
/// `none` or an empty value is the background, `#rrggbb` a colour, and a
/// name, a path or nothing (the release's default) a picture.
pub fn desktop_of(value: Option<&str>) -> Wallpaper {
    match value {
        Some("") | Some(wallpapers::NONE) => Wallpaper::Background,
        Some(v) if v.starts_with('#') => {
            Colour::parse(v).map_or(Wallpaper::Background, Wallpaper::Colour)
        }
        other => Wallpaper::Picture(PathBuf::from(other.unwrap_or_default())),
    }
}

/// Why `v` does not read over `surface` (M5.33d), as a line for the one who
/// writes it: None for `wallpaper`, which is fitted when drawn, and for a
/// colour that passes [`ratio`]. The line names the pair, what it reads at
/// and a colour that does pass.
pub fn problem(w: &Widget, v: &str, tokens: &Tokens, surface: Colour) -> Option<String> {
    if v == tokens::WALLPAPER_ACCENT {
        return None;
    }
    let colour = chosen(v, tokens)?;
    let need = ratio(w);
    let reads = contrast(colour, surface);
    if reads >= need {
        return None;
    }
    let what = if is_large(w) {
        tr("large text")
    } else {
        tr("text")
    };
    let fix = nearest_passing(colour, surface, need);
    Some(trf(
        "the {part}'s colour {value} is {fg} on its wallpaper {bg} and reads at {reads}:1; {what} needs {need}:1, such as {fix}",
        &[
            ("part", w.name),
            ("value", &format!("{v:?}")),
            ("fg", &colour.hex()),
            ("bg", &surface.hex()),
            ("reads", &format!("{:.1}", tokens::shown_down(reads))),
            ("what", what),
            ("need", &format!("{need:.1}")),
            ("fix", &fix.hex()),
        ],
    ))
}

/// The colours the editor offers a widget (M5.33d), 12 in all: automatic,
/// the accent, each named accent in the file's order, then `wallpaper`.
pub fn swatches() -> Vec<Option<String>> {
    let mut all = vec![None, Some(ACCENT.to_string())];
    all.extend(tokens::accents().into_iter().map(|(name, _, _)| Some(name)));
    all.push(Some(tokens::WALLPAPER_ACCENT.to_string()));
    all
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock_screen::{Anchor, DatePart, DateStyle, Face, Part, Placed, Size};
    use std::collections::BTreeSet;

    #[test]
    fn names_are_unique_each_fits_a_surface_and_every_desktop_entry_is_a_part() {
        for (i, widget) in CATALOGUE.iter().enumerate() {
            assert!(crate::features::is_name(widget.name), "{}", widget.name);
            assert!(
                CATALOGUE[..i].iter().all(|w| w.name != widget.name),
                "two widgets called {}",
                widget.name
            );
            assert!(
                widget.fits(Surface::Panel)
                    || widget.fits(Surface::LockScreen)
                    || widget.fits(Surface::Desktop),
                "{} fits no surface",
                widget.name
            );
            if widget.fits(Surface::Desktop) {
                assert!(
                    Part::named(widget.name).is_some(),
                    "{} fits the desktop but is no lock_screen::Part",
                    widget.name
                );
            }
        }
        let desktop: Vec<&str> = CATALOGUE
            .iter()
            .filter(|w| w.fits(Surface::Desktop))
            .map(|w| w.name)
            .collect();
        assert_eq!(desktop, ["clock", "date"]);
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
            colour: Some("accent".into()),
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
    #[test]
    fn the_clock_the_date_a_message_and_the_battery_take_a_colour() {
        for widget in CATALOGUE {
            let takes = matches!(widget.name, "clock" | "date" | "message" | "battery");
            assert_eq!(widget.takes(COLOUR), takes, "{}", widget.name);
        }
    }

    #[test]
    fn a_colour_is_the_accent_wallpaper_an_accents_name_or_a_hex() {
        for value in ["accent", "blue", "wallpaper", "#e8b04a"] {
            assert!(is_colour(value), "{value}");
        }
        for value in ["#e8b04a80", "navy", ""] {
            assert!(!is_colour(value), "{value}");
        }
    }

    #[test]
    fn a_colour_follows_the_accent_and_each_schemes_shade() {
        let light = Tokens::built_in_scheme(tokens::Scheme::Light);
        let dark = Tokens::built_in_scheme(tokens::Scheme::Dark);
        assert_eq!(chosen("accent", &light), Some(light.accent));
        assert_eq!(chosen("accent", &dark), Some(dark.accent));
        assert_eq!(
            chosen("blue", &light),
            tokens::accent_named("blue", tokens::Scheme::Light)
        );
        assert_eq!(
            chosen("blue", &dark),
            tokens::accent_named("blue", tokens::Scheme::Dark)
        );
        assert_ne!(chosen("blue", &light), chosen("blue", &dark));
        assert_eq!(chosen("wallpaper", &light), None);
        assert_eq!(chosen("#000000", &light), Colour::parse("#000000"));
    }

    #[test]
    fn ink_is_automatic_or_fitted_to_read_over_its_surface() {
        let tokens = Tokens::built_in();
        let black = Behind {
            surface: Colour::parse("#000000").unwrap(),
            source: None,
        };
        assert_eq!(ink(None, &CLOCK, &tokens, black), None);
        // A colour that reads already is kept as it is.
        assert_eq!(
            ink(Some("#e8b04a"), &CLOCK, &tokens, black),
            Colour::parse("#e8b04a")
        );
        // A grey that does not read on black is made light enough.
        let grey = ink(Some("#333333"), &DATE, &tokens, black).unwrap();
        assert!(contrast(grey, black.surface) >= TEXT_CONTRAST);
        // `wallpaper` takes the wallpaper's colour, and is automatic without one.
        assert_eq!(ink(Some("wallpaper"), &CLOCK, &tokens, black), None);
        let picture = Behind {
            source: Colour::parse("#e8b04a"),
            ..black
        };
        assert_eq!(
            ink(Some("wallpaper"), &CLOCK, &tokens, picture),
            Colour::parse("#e8b04a")
        );
    }

    #[test]
    fn behind_gives_each_surface_its_rule() {
        let tokens = Tokens::built_in();
        let blue = Colour::parse("#1f2b48").unwrap();
        let red = Colour::parse("#ff0000").unwrap();
        let picture = |path: &Path| (path == Path::new("/a.png")).then_some(red);
        let no_picture = |_: &Path| None;
        let photo = Wallpaper::Picture(PathBuf::from("/a.png"));
        let dimmed = |c: Colour| c.towards(tokens.lock_backdrop_deep, 0.3);

        let on_desktop = |desktop: &Wallpaper| {
            behind(
                Surface::Desktop,
                &Background::Sharp,
                0,
                desktop,
                &tokens,
                picture,
            )
        };
        assert_eq!(
            on_desktop(&Wallpaper::Colour(blue)),
            Behind {
                surface: blue,
                source: Some(blue)
            }
        );
        assert_eq!(
            on_desktop(&photo),
            Behind {
                surface: tokens.backdrop_deep,
                source: Some(red)
            }
        );
        assert_eq!(
            on_desktop(&Wallpaper::Background),
            Behind {
                surface: tokens.background,
                source: None
            }
        );

        let on_lock = |background: &Background, desktop: &Wallpaper| {
            behind(
                Surface::LockScreen,
                background,
                30,
                desktop,
                &tokens,
                no_picture,
            )
        };
        assert_eq!(
            on_lock(&Background::Backdrop, &photo),
            Behind {
                surface: tokens.lock_backdrop,
                source: None
            }
        );
        assert_eq!(
            on_lock(&Background::Colour(blue), &photo),
            Behind {
                surface: dimmed(blue),
                source: Some(blue)
            }
        );
        assert_eq!(
            on_lock(&Background::Blurred, &Wallpaper::Colour(blue)),
            Behind {
                surface: dimmed(blue),
                source: Some(blue)
            }
        );
        assert_eq!(
            on_lock(&Background::Blurred, &Wallpaper::Background),
            Behind {
                surface: tokens.lock_backdrop,
                source: None
            }
        );
        let on_photo = behind(
            Surface::LockScreen,
            &Background::Picture(PathBuf::from("/a.png")),
            30,
            &Wallpaper::Background,
            &tokens,
            picture,
        );
        assert_eq!(
            on_photo,
            Behind {
                surface: dimmed(tokens.backdrop_deep),
                source: Some(red)
            }
        );
    }

    #[test]
    fn desktop_of_reads_a_value_with_no_disk() {
        assert_eq!(desktop_of(Some("")), Wallpaper::Background);
        assert_eq!(desktop_of(Some("none")), Wallpaper::Background);
        assert_eq!(
            desktop_of(Some("#000000")),
            Wallpaper::Colour(Colour::parse("#000000").unwrap())
        );
        assert_eq!(
            desktop_of(Some("hills")),
            Wallpaper::Picture(PathBuf::from("hills"))
        );
        assert!(matches!(desktop_of(None), Wallpaper::Picture(_)));
    }

    #[test]
    fn problem_names_the_pair_and_is_none_where_it_reads() {
        let tokens = Tokens::built_in_scheme(tokens::Scheme::Light);
        let line = problem(&CLOCK, "accent", &tokens, tokens.lock_backdrop)
            .expect("the light accent does not read as large text over the classic look");
        assert!(
            line.starts_with("the clock's colour \"accent\" is #"),
            "{line}"
        );
        assert!(line.contains(" reads at "), "{line}");
        assert!(
            line.contains("; large text needs 3.0:1, such as #"),
            "{line}"
        );
        let black = Colour::parse("#000000").unwrap();
        assert!(problem(&CLOCK, "accent", &tokens, black).is_none());
        assert!(problem(&DATE, "wallpaper", &tokens, black).is_none());
    }

    #[test]
    fn the_editor_offers_twelve_swatches_from_automatic_to_wallpaper() {
        let all = swatches();
        assert_eq!(all.len(), 12);
        assert_eq!(all[0], None);
        assert_eq!(all[1].as_deref(), Some("accent"));
        assert_eq!(all[11].as_deref(), Some("wallpaper"));
        assert_eq!(all[2].as_deref(), Some(tokens::accents()[0].0.as_str()));
    }
}
