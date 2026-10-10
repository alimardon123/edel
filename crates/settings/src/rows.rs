//! Every row of Settings with its key (M5.6b): the one list a test holds
//! to `edel::settings`'s key table, and what each row says about where its
//! value comes from (ADR-008).

use edel::i18n::{n_, tr, trf};
use edel::settings::Source;

/// One row: the settings file key it shows and changes.
pub struct Row {
    pub key: &'static str,
    pub title: &'static str,
}

/// The rows of every page, page by page.
pub const ROWS: &[Row] = &[
    Row {
        key: "layout.preset",
        title: n_("Preset"),
    },
    Row {
        key: "layout.tiling",
        title: n_("Tile windows"),
    },
    Row {
        key: "layout.tiling_style",
        title: n_("Tiling style"),
    },
    // The panels that apply, and the editor that changes them (M5.31d).
    Row {
        key: "layout.panels",
        title: n_("Panels"),
    },
    Row {
        key: "layout.title_bars",
        title: n_("Title bars"),
    },
    Row {
        key: "layout.window_buttons",
        title: n_("Window buttons"),
    },
    // The title bar's buttons (M5.18a).
    Row {
        key: "layout.minimize_button",
        title: n_("Minimize button"),
    },
    Row {
        key: "layout.maximize_button",
        title: n_("Maximize button"),
    },
    Row {
        key: "layout.close_button",
        title: n_("Close button"),
    },
    // The tray's apps in the panel (M5.9h): the card lists each app.
    Row {
        key: "layout.tray_in_panel",
        title: n_("Tray"),
    },
    // The Displays page (M5.7a); the `*` is a screen's name.
    Row {
        key: "displays.*.resolution",
        title: n_("Resolution"),
    },
    Row {
        key: "displays.*.scale",
        title: n_("Scale"),
    },
    Row {
        key: "displays.*.position",
        title: n_("Position"),
    },
    Row {
        key: "displays.*.enabled",
        title: n_("On"),
    },
    // The Notifications page (M5.9b).
    Row {
        key: edel::settings::DO_NOT_DISTURB,
        title: n_("Do not disturb"),
    },
    // The Updates page (M5.8c).
    Row {
        key: "updates.channel",
        title: n_("Channel"),
    },
];

/// What the Panels group says under its heading (M5.31d).
pub const PANELS_WHAT: &str = n_("Which panels and docks show, where, and what they hold");

/// The key of the row `name` means, by its key or its title, whatever the
/// case (`layout.panels` or `Panels`), if one does.
pub fn row_named(name: &str) -> Option<&'static str> {
    let name = name.trim().to_lowercase();
    ROWS.iter()
        .find(|r| r.key.to_lowercase() == name || title(r.key).to_lowercase() == name)
        .map(|r| r.key)
}

/// The title of `key`'s row, in the person's language.
pub fn title(key: &str) -> &'static str {
    ROWS.iter()
        .find(|r| r.key == key)
        .map_or("", |r| tr(r.title))
}

/// The rows on the page for `section`, such as `layout`, which search
/// looks through as well as the pages' titles.
pub fn on_page(section: &str) -> impl Iterator<Item = &'static Row> {
    ROWS.iter().filter(move |r| {
        r.key
            .split_once('.')
            .is_some_and(|(page, _)| page == section)
    })
}

/// The values `key` may take, as the key table lists them, so a row's
/// choices are always the ones `edel settings set` takes.
pub fn values(key: &str) -> &'static [&'static str] {
    edel::settings::choices(key)
}

/// A value as a row shows it: `floating-only` is Floating only, in the
/// person's language for the values the key table lists.
pub fn label(value: &str) -> String {
    let word = match value {
        "always" => n_("Always"),
        "floating-only" => n_("Floating only"),
        "left" => n_("Left"),
        "right" => n_("Right"),
        "stack" => n_("Stack"),
        "split" => n_("Split"),
        "scroll" => n_("Scroll"),
        _ => return crate::files::title(&value.replace('-', " ")),
    };
    tr(word).to_string()
}

/// What a row says under its title about where its value comes from:
/// the person's own choice, the machine's, or `Automatic` and the value
/// the release gives it.
pub fn describe(source: &Source, current: &str) -> String {
    match source {
        Source::Person(_) => tr("Your choice").to_string(),
        Source::Machine(_) => tr("Set by this machine").to_string(),
        Source::Release => trf("Automatic ({current})", &[("current", current)]),
    }
}

/// What a row's line adds about where its value comes from, as the
/// mockup's rows say it: nothing when the release decides, `From the
/// preset` when the preset does, `Your choice` or `Set by this machine`.
pub fn note(source: &Source, from_preset: bool) -> Option<&'static str> {
    match source {
        Source::Person(_) => Some(tr("Your choice")),
        Source::Machine(_) => Some(tr("Set by this machine")),
        Source::Release if from_preset => Some(tr("From the preset")),
        Source::Release => None,
    }
}

/// The command that sets each of `pairs` on any machine, as Copy as
/// command gives it (ADR-008: every setting is a row, a command and a
/// line of the file); `edel settings set` takes several `KEY=VALUE` at
/// once, for a row of several keys.
pub fn command(pairs: &[(&str, String)]) -> String {
    let assignments: Vec<String> = pairs
        .iter()
        .map(|(k, v)| format!("{k}={}", quoted(v)))
        .collect();
    format!("edel settings set {}", assignments.join(" "))
}

/// `value` as a shell takes it: as it is when it holds only letters,
/// digits and `. , : / @ % + -`, else in single quotes, so a pair such as
/// `[0, 0]` is one word and nothing in it is a glob.
fn quoted(value: &str) -> String {
    let plain = |c: char| c.is_ascii_alphanumeric() || ".,:/@%+-_".contains(c);
    if !value.is_empty() && value.chars().all(plain) {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

/// `Version 2026.10.5 · Stable`, or what is known of it: the version alone,
/// the channel alone, or nothing; one line for the Updates page's card and
/// About's.
pub fn version_line(version: Option<&str>, channel: Option<&str>) -> String {
    match (version, channel) {
        (Some(v), Some(c)) => trf(
            "Version {version} · {channel}",
            &[("version", v), ("channel", c)],
        ),
        (Some(v), None) => trf("Version {version}", &[("version", v)]),
        (None, Some(c)) => c.to_string(),
        (None, None) => String::new(),
    }
}

/// A size as people read it: `412 MB`, `1.4 GB`. Decimal units, as the
/// stores and the disk makers use, so a size here matches the one on the box.
pub fn bytes_text(bytes: u64) -> String {
    const MB: f64 = 1_000_000.0;
    let mb = bytes as f64 / MB;
    if mb < 1000.0 {
        trf("{n} MB", &[("n", &format!("{:.0}", mb.max(1.0)))])
    } else if mb < 10_000.0 {
        trf("{n} GB", &[("n", &format!("{:.1}", mb / 1000.0))])
    } else {
        trf("{n} GB", &[("n", &format!("{:.0}", mb / 1000.0))])
    }
}

/// Whether the row can be reset: only the person's own value can, as
/// Settings writes only the person's file.
pub fn resettable(source: &Source) -> bool {
    matches!(source, Source::Person(_))
}

#[cfg(test)]
mod tests {
    use super::*;
    use edel::settings::{self, KEYS, Source};
    use toml::Value;

    #[test]
    fn every_row_is_a_key_this_release_supports() {
        for row in ROWS {
            let key = KEYS
                .iter()
                .find(|k| k.path == row.key)
                .unwrap_or_else(|| panic!("{} is not a settings file key", row.key));
            assert!(key.supported, "{} is not supported yet", row.key);
        }
    }

    #[test]
    fn a_row_offers_the_values_the_key_table_lists() {
        assert_eq!(values("layout.title_bars"), ["always", "floating-only"]);
        assert_eq!(values("layout.window_buttons"), ["left", "right"]);
        assert_eq!(label("floating-only"), "Floating only");
        let titles: Vec<&str> = on_page("layout").map(|r| r.title).collect();
        assert_eq!(titles.len(), 10);
        assert_eq!(values("layout.tiling_style"), ["stack", "split", "scroll"]);
        assert_eq!(on_page("about").count(), 0);
    }

    #[test]
    fn a_row_says_where_its_value_comes_from() {
        assert_eq!(describe(&Source::Release, "Classic"), "Automatic (Classic)");
        let set = Source::Machine(Value::String("hive".into()));
        assert_eq!(describe(&set, "Hive"), "Set by this machine");
        assert!(!resettable(&set));
        let own = Source::Person(Value::Boolean(true));
        assert_eq!(describe(&own, "on"), "Your choice");
        assert!(resettable(&own));
        assert_eq!(note(&Source::Release, true), Some("From the preset"));
        assert_eq!(note(&Source::Release, false), None);
        assert_eq!(note(&own, true), Some("Your choice"));
        assert_eq!(note(&set, false), Some("Set by this machine"));
    }

    #[test]
    fn the_version_line_says_what_is_known() {
        assert_eq!(
            version_line(Some("2026.10.5"), Some("Stable")),
            "Version 2026.10.5 · Stable"
        );
        assert_eq!(version_line(Some("2026.10.5"), None), "Version 2026.10.5");
        assert_eq!(version_line(None, Some("Stable")), "Stable");
        assert_eq!(version_line(None, None), "");
    }

    #[test]
    fn sizes_read_in_megabytes_then_gigabytes() {
        assert_eq!(bytes_text(412_000_000), "412 MB");
        assert_eq!(bytes_text(1_400_000_000), "1.4 GB");
        assert_eq!(bytes_text(128_000_000_000), "128 GB");
        assert_eq!(bytes_text(200), "1 MB");
    }

    #[test]
    fn a_value_with_spaces_is_one_word_for_the_shell() {
        let line = command(&[("displays.eDP-1.position", "[1280, 0]".into())]);
        assert_eq!(
            line,
            "edel settings set displays.eDP-1.position='[1280, 0]'"
        );
        assert_eq!(quoted("2.5"), "2.5");
        assert_eq!(quoted("1920x1080"), "1920x1080");
        assert_eq!(quoted("it's"), "'it'\\''s'");
        assert_eq!(quoted(""), "''");
    }

    #[test]
    fn the_displays_rows_are_found_by_search_and_belong_to_the_page() {
        let titles: Vec<&str> = on_page("displays").map(|r| r.title).collect();
        assert_eq!(titles, ["Resolution", "Scale", "Position", "On"]);
        assert_eq!(title("displays.*.scale"), "Scale");
    }

    #[test]
    fn the_panels_row_is_found_by_its_key_or_its_title() {
        assert_eq!(row_named("layout.panels"), Some("layout.panels"));
        assert_eq!(row_named("Panels"), Some("layout.panels"));
        assert_eq!(row_named("PANELS"), Some("layout.panels"));
        assert_eq!(row_named("panel"), None, "a start is not a row");
        assert_eq!(row_named("scale"), Some("displays.*.scale"));
        assert_eq!(title("layout.panels"), "Panels");
    }

    #[test]
    fn copy_as_command_is_the_line_edel_settings_takes() {
        let line = command(&[("layout.preset", "mac-like".into())]);
        assert_eq!(line, "edel settings set layout.preset=mac-like");
        let (key, value) = line
            .strip_prefix("edel settings set ")
            .and_then(|a| a.split_once('='))
            .unwrap();
        assert!(settings::set("format = 1\n", key, value).is_ok());
        let both = command(&[
            ("layout.close_button", "true".into()),
            ("layout.minimize_button", "false".into()),
        ]);
        assert_eq!(
            both,
            "edel settings set layout.close_button=true layout.minimize_button=false"
        );
    }
}
