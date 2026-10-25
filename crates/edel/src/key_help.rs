//! What every settings file key means, its default and who decides it
//! (M6.11, ADR-008): the kinds of a key's [`Help`], which each entry of
//! [`crate::settings::KEYS`] carries, and what reads them there: the help
//! `edel settings get` prints, the suggestion `set` and `check` give, and
//! the two generated docs, `docs/settings.md`'s key list and
//! `docs/defaults.md`.

use crate::settings::{KEYS, Key, PAGES, page_of};

/// Who decides a key's value while the settings file does not set it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecidedBy {
    /// The release's own value, the same on every machine.
    Release,
    /// The layout preset in use (`layout.preset`), so it differs by preset.
    Preset,
    /// A design token in `design/tokens.toml`.
    Token,
    /// The image the machine was made from (a desktop, a server).
    Image,
    /// The hardware: the screen, the keyboard, the disk.
    Hardware,
}

impl DecidedBy {
    /// The word `edel settings get` and the docs show for it.
    pub fn word(self) -> &'static str {
        match self {
            DecidedBy::Release => "release",
            DecidedBy::Preset => "preset",
            DecidedBy::Token => "token",
            DecidedBy::Image => "image",
            DecidedBy::Hardware => "hardware",
        }
    }
}

/// Whose a key is (Seam, ADR-011): a person's follows them to another of
/// their devices (M7.10f), a device's stays with the machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Belongs {
    Person,
    Device,
}

/// One key's line, the `about` of its entry in [`crate::settings::KEYS`].
#[derive(Clone, Copy, Debug)]
pub struct Help {
    /// One sentence for people, marked for translation.
    pub help: &'static str,
    /// The value that applies while the file does not set the key, as the
    /// file would write it, or a short phrase when it depends ("the
    /// preset's", "every screen's own").
    pub default: &'static str,
    /// Who decides that default.
    pub from: DecidedBy,
    pub belongs: Belongs,
    /// The release that stopped using the key, if one did: it is still
    /// read, reported as no longer used and refused by `set`.
    pub retired: Option<&'static str>,
}

/// The line for `key`, a concrete key (`users.ali.admin`) or a table path
/// (`users.*.admin`).
pub fn find(key: &str) -> Option<&'static Help> {
    KEYS.iter()
        .find(|entry| fits(entry.path, key))
        .map(|entry| &entry.about)
}

/// Whether `key` is `path`, a `*` standing for any one part.
fn fits(path: &str, key: &str) -> bool {
    let (mut a, mut b) = (path.split('.'), key.split('.'));
    loop {
        match (a.next(), b.next()) {
            (None, None) => return true,
            (Some(p), Some(k)) if p == "*" || p == k => {}
            _ => return false,
        }
    }
}

/// The Markdown of `docs/settings.md`'s list of every key (M6.11): a heading
/// for each page of the Settings app with its keys in a table, then a last
/// heading for the keys no page holds.
pub fn settings_markdown() -> String {
    let mut out = String::new();
    for page in PAGES {
        out.push_str(&format!("### `{}`\n\n", page.section));
        let lines: Vec<&Key> = KEYS
            .iter()
            .filter(|key| page_of(key.path).map(|p| p.section) == Some(page.section))
            .collect();
        if lines.is_empty() {
            out.push_str("This page has no key in the file.\n\n");
        } else {
            out.push_str(&table(&lines));
        }
    }
    let other: Vec<&Key> = KEYS
        .iter()
        .filter(|key| page_of(key.path).is_none())
        .collect();
    out.push_str("### Other keys\n\n");
    out.push_str(&table(&other));
    out
}

/// A default as the docs show it: a value the file would write (`"classic"`,
/// `false`, `4`, `[]`) in code style, a phrase ("the preset's") as words.
fn shown(default: &str) -> String {
    let value = default.starts_with('"')
        || default.starts_with('[')
        || default.starts_with('{')
        || matches!(default, "true" | "false")
        || default.parse::<f64>().is_ok();
    if value {
        format!("`{}`", cell(default))
    } else {
        cell(default)
    }
}

/// The whole of `docs/defaults.md` (M6.11): every key's default and who
/// decides it, in the order of [`KEYS`].
pub fn defaults_markdown() -> String {
    let mut out = String::from(
        "# Defaults\n\nThis file is written by `EDEL_WRITE_DOCS=1 cargo test -p edel key_help` from the key table, `KEYS` in `crates/edel/src/settings.rs`, and must not be edited by hand. It gives the value every settings key has while the settings file does not set it, and who decides that value. [The settings file](settings.md) says what each key does.\n\n| Key | Default | Decided by |\n|---|---|---|\n",
    );
    for key in KEYS {
        out.push_str(&format!(
            "| `{}` | {} | {} |\n",
            key.path,
            shown(key.about.default),
            key.about.from.word()
        ));
    }
    out
}

/// Text for a table cell: a `|` would end the cell, so it is escaped.
fn cell(text: &str) -> String {
    text.replace('|', "\\|")
}

/// The table of `keys` under its header, with a blank line after it.
fn table(keys: &[&Key]) -> String {
    let mut out =
        String::from("| Key | What it does | Default | Decided by |\n|---|---|---|---|\n");
    for key in keys {
        out.push_str(&format!(
            "| `{}` | {} | {} | {} |\n",
            key.path,
            cell(key.about.help),
            shown(key.about.default),
            key.about.from.word()
        ));
    }
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two marker lines of `docs/settings.md`'s generated list of keys.
    const BEGIN: &str =
        "<!-- written by key_help.rs: EDEL_WRITE_DOCS=1 cargo test -p edel key_help -->";
    const END: &str = "<!-- end of the keys -->";

    /// The byte range of the generated keys in a settings page: from the
    /// line after `BEGIN` to the start of `END`.
    fn generated(page: &str) -> Option<std::ops::Range<usize>> {
        let start = page.find(BEGIN)? + BEGIN.len();
        let start = start + page[start..].find('\n')? + 1;
        let end = start + page[start..].find(END)?;
        Some(start..end)
    }

    #[test]
    fn each_help_is_one_short_sentence() {
        for Key {
            path: key,
            about: line,
            ..
        } in KEYS
        {
            assert!(line.help.ends_with('.'), "{}: no full stop", key);
            assert!(!line.help.contains('\n'), "{}: more than one line", key);
            assert!(
                line.help.chars().count() <= 150,
                "{}: longer than 150 characters",
                key
            );
            assert!(
                !line.help.contains('\u{2013}') && !line.help.contains('\u{2014}'),
                "{}: an en dash or an em dash",
                key
            );
        }
    }

    #[test]
    fn find_takes_a_concrete_key_for_a_star() {
        let about = |path: &str| {
            KEYS.iter()
                .find(|key| key.path == path)
                .map(|key| &key.about)
        };
        assert!(std::ptr::eq(
            find("users.ali.admin").unwrap(),
            about("users.*.admin").unwrap()
        ));
        assert!(std::ptr::eq(
            find("layout.preset").unwrap(),
            about("layout.preset").unwrap()
        ));
        assert!(find("no.such").is_none());
    }

    #[test]
    fn the_power_defaults_are_the_settings_ones() {
        use crate::settings::{LOCK_AFTER_MINUTES_DEFAULT, LOCK_AFTER_MINUTES_DEFAULT_TEXT};
        assert_eq!(
            LOCK_AFTER_MINUTES_DEFAULT_TEXT.parse::<u32>().ok(),
            Some(LOCK_AFTER_MINUTES_DEFAULT)
        );
        assert_eq!(find("power.lid_close").map(|h| h.default), Some("suspend"));
        assert_eq!(
            find("power.power_button").map(|h| h.default),
            Some("suspend")
        );
        assert_eq!(
            find("power.on_battery").map(|h| h.default),
            Some("balanced")
        );
    }

    #[test]
    fn the_saver_s_help_names_its_percent() {
        // The words say the number the constant holds, so they cannot drift.
        let help = find("power.saver").map(|h| h.help).unwrap_or("");
        assert!(
            help.contains(&format!("{} percent", crate::settings::SAVER_AT_PERCENT)),
            "{help}"
        );
    }

    #[test]
    fn the_docs_are_what_the_table_writes() {
        let settings_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/settings.md");
        let defaults_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/defaults.md");
        if std::env::var_os("EDEL_WRITE_DOCS").is_some() {
            let page = std::fs::read_to_string(settings_path).unwrap();
            let span = generated(&page).expect("docs/settings.md has no generated keys");
            let written = format!(
                "{}{}{}",
                &page[..span.start],
                settings_markdown(),
                &page[span.end..]
            );
            std::fs::write(settings_path, written).unwrap();
            std::fs::write(defaults_path, defaults_markdown()).unwrap();
        }
        let page = std::fs::read_to_string(settings_path).unwrap();
        let span = generated(&page).expect("docs/settings.md has no generated keys");
        assert!(
            page[span] == settings_markdown(),
            "docs/settings.md is not what key_help.rs writes; run EDEL_WRITE_DOCS=1 cargo test -p edel key_help"
        );
        let defaults = std::fs::read_to_string(defaults_path).unwrap_or_default();
        assert!(
            defaults == defaults_markdown(),
            "docs/defaults.md is not what key_help.rs writes; run EDEL_WRITE_DOCS=1 cargo test -p edel key_help"
        );
    }
}
