//! Every row of Settings with its key (M5.6b): the one list a test holds
//! to `edel::system`'s key table, and what each row says about where its
//! value comes from (ADR-008).

use edel::system::Source;

/// One row: the settings file key it shows and changes.
pub struct Row {
    pub key: &'static str,
    pub title: &'static str,
}

/// The rows of every page, page by page.
pub const ROWS: &[Row] = &[
    Row {
        key: "layout.preset",
        title: "Preset",
    },
    Row {
        key: "layout.tiling",
        title: "Tile windows",
    },
    Row {
        key: "layout.tiling_style",
        title: "Tiling style",
    },
    Row {
        key: "layout.title_bars",
        title: "Title bars",
    },
    Row {
        key: "layout.window_buttons",
        title: "Window buttons",
    },
    // The title bar's buttons (M5.18a).
    Row {
        key: "layout.minimize_button",
        title: "Minimize button",
    },
    Row {
        key: "layout.maximize_button",
        title: "Maximize button",
    },
    Row {
        key: "layout.close_button",
        title: "Close button",
    },
];

/// The title of `key`'s row.
pub fn title(key: &str) -> &'static str {
    ROWS.iter().find(|r| r.key == key).map_or("", |r| r.title)
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
    match edel::system::KEYS.iter().find(|k| k.path == key) {
        Some(edel::system::Key {
            kind: edel::system::Kind::OneOf(values),
            ..
        }) => values,
        _ => &[],
    }
}

/// A value as a row shows it: `floating-only` is Floating only.
pub fn label(value: &str) -> String {
    crate::files::title(&value.replace('-', " "))
}

/// What a row says under its title about where its value comes from:
/// the person's own choice, the machine's, or `Automatic` and the value
/// the release gives it.
pub fn describe(source: &Source, current: &str) -> String {
    match source {
        Source::Person(_) => "Your choice".to_string(),
        Source::Machine(_) => "Set by this machine".to_string(),
        Source::Release => format!("Automatic ({current})"),
    }
}

/// What a row's line adds about where its value comes from, as the
/// mockup's rows say it: nothing when the release decides, `From the
/// preset` when the preset does, `Your choice` or `Set by this machine`.
pub fn note(source: &Source, from_preset: bool) -> Option<&'static str> {
    match source {
        Source::Person(_) => Some("Your choice"),
        Source::Machine(_) => Some("Set by this machine"),
        Source::Release if from_preset => Some("From the preset"),
        Source::Release => None,
    }
}

/// The command that sets each of `pairs` on any machine, as Copy as
/// command gives it (ADR-008: every setting is a row, a command and a
/// line of the file); `edel settings set` takes several `KEY=VALUE` at
/// once, for a row of several keys.
pub fn command(pairs: &[(&str, String)]) -> String {
    let assignments: Vec<String> = pairs.iter().map(|(k, v)| format!("{k}={v}")).collect();
    format!("edel settings set {}", assignments.join(" "))
}

/// Whether the row can be reset: only the person's own value can, as
/// Settings writes only the person's file.
pub fn resettable(source: &Source) -> bool {
    matches!(source, Source::Person(_))
}

#[cfg(test)]
mod tests {
    use super::*;
    use edel::system::{self, KEYS, Source};
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
        assert_eq!(titles.len(), 8);
        assert_eq!(values("layout.tiling_style"), ["stack", "split"]);
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
    fn copy_as_command_is_the_line_edel_settings_takes() {
        let line = command(&[("layout.preset", "mac-like".into())]);
        assert_eq!(line, "edel settings set layout.preset=mac-like");
        let (key, value) = line
            .strip_prefix("edel settings set ")
            .and_then(|a| a.split_once('='))
            .unwrap();
        assert!(system::set("format = 1\n", key, value).is_ok());
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
