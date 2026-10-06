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
];

/// The title of `key`'s row.
pub fn title(key: &str) -> &'static str {
    ROWS.iter().find(|r| r.key == key).map_or("", |r| r.title)
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

/// The command that sets `key` to `value` on any machine, as Copy as
/// command gives it (ADR-008: every setting is a row, a command and a
/// line of the file).
pub fn command(key: &str, value: &str) -> String {
    format!("edel settings set {key}={value}")
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
    fn a_row_says_where_its_value_comes_from() {
        assert_eq!(describe(&Source::Release, "Classic"), "Automatic (Classic)");
        let set = Source::Machine(Value::String("hive".into()));
        assert_eq!(describe(&set, "Hive"), "Set by this machine");
        assert!(!resettable(&set));
        let own = Source::Person(Value::Boolean(true));
        assert_eq!(describe(&own, "on"), "Your choice");
        assert!(resettable(&own));
    }

    #[test]
    fn copy_as_command_is_the_line_edel_settings_takes() {
        let line = command("layout.preset", "mac-like");
        assert_eq!(line, "edel settings set layout.preset=mac-like");
        let (key, value) = line
            .strip_prefix("edel settings set ")
            .and_then(|a| a.split_once('='))
            .unwrap();
        assert!(system::set("format = 1\n", key, value).is_ok());
    }
}
