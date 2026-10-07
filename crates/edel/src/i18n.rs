//! The words people read, in their language (roadmap M5.24a). Each part
//! that shows words (shell-ui, Settings) marks every one with [`tr`] or
//! [`trf`]; a test gathers the marked words into the part's template,
//! `po/PART.pot`, and fails when the committed one differs. A language's
//! words are a gettext `.po` file, `LANG/PART.po` under
//! [`crate::places::LOCALE_DIR`], read at start by the small reader here
//! (no libintl, and no compiled `.mo` step: the text file is the one
//! format), and the language is `region.language`, the person's over the
//! machine's. A word the catalogue lacks, or has no translation for,
//! stays English.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::places;

/// One language's words for one part: English to theirs.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Catalogue {
    words: HashMap<String, String>,
}

impl Catalogue {
    pub fn len(&self) -> usize {
        self.words.len()
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    pub fn get(&self, english: &str) -> Option<&str> {
        self.words.get(english).map(String::as_str)
    }
}

/// A `.po` file's text read: each `msgid` with a non-empty `msgstr`, its
/// lines joined and its escapes undone; entries marked fuzzy, plurals and
/// the header (the empty `msgid`) are left out.
pub fn parse_po(text: &str) -> Catalogue {
    let mut words = HashMap::new();
    let (mut id, mut text_of, mut fuzzy) = (None::<String>, None::<String>, false);
    // Which string a continuation line adds to.
    #[derive(PartialEq)]
    enum Field {
        Id,
        Str,
        Other,
    }
    let mut field = Field::Other;
    let mut finish = |id: &mut Option<String>, text_of: &mut Option<String>, fuzzy: &mut bool| {
        if let (Some(i), Some(t)) = (id.take(), text_of.take()) {
            if !i.is_empty() && !t.is_empty() && !*fuzzy {
                words.insert(i, t);
            }
        }
        *fuzzy = false;
    };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            finish(&mut id, &mut text_of, &mut fuzzy);
            field = Field::Other;
        } else if let Some(flags) = line.strip_prefix("#,") {
            finish(&mut id, &mut text_of, &mut fuzzy);
            fuzzy = flags.split(',').any(|f| f.trim() == "fuzzy");
            field = Field::Other;
        } else if line.starts_with('#') {
            continue;
        } else if let Some(rest) = line.strip_prefix("msgid ") {
            if id.is_some() {
                finish(&mut id, &mut text_of, &mut fuzzy);
            }
            id = Some(unquote(rest));
            field = Field::Id;
        } else if let Some(rest) = line.strip_prefix("msgstr ") {
            text_of = Some(unquote(rest));
            field = Field::Str;
        } else if line.starts_with('"') {
            let more = unquote(line);
            match field {
                Field::Id => id.get_or_insert_with(String::new).push_str(&more),
                Field::Str => text_of.get_or_insert_with(String::new).push_str(&more),
                Field::Other => {}
            }
        } else {
            // msgid_plural, msgstr[N], msgctxt: not used by our parts.
            field = Field::Other;
            if line.starts_with("msgid_plural") || line.starts_with("msgstr[") {
                id = None;
                text_of = None;
            }
        }
    }
    finish(&mut id, &mut text_of, &mut fuzzy);
    Catalogue { words }
}

/// A quoted `.po` string's text, its escapes undone.
fn unquote(quoted: &str) -> String {
    let inner = quoted
        .trim()
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or("");
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// `text` as a `.po` string, its quotes, backslashes and line ends escaped.
pub fn quote(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The language `region.language` names, the person's over the machine's;
/// none when neither sets one, and English is shown.
pub fn language() -> Option<String> {
    let read = |path: std::path::PathBuf| std::fs::read_to_string(places::found(&path)).ok();
    let machine = read(places::machine_settings());
    let person = places::person_settings().and_then(read);
    crate::settings::chosen("region.language", machine.as_deref(), person.as_deref())
}

static WORDS: OnceLock<Catalogue> = OnceLock::new();

/// Reads `part`'s catalogue in the person's language, once, at start;
/// returns the language and how many words it translates.
pub fn init(part: &str) -> Option<(String, usize)> {
    let language = language()?;
    // `pt_BR` reads `pt_BR`'s words, else `pt`'s.
    let base = language.split(['_', '.', '@']).next().unwrap_or(&language);
    let catalogue = [language.as_str(), base]
        .iter()
        .find_map(|name| std::fs::read_to_string(places::catalogue(name, part)).ok())
        .map(|text| parse_po(&text))
        .unwrap_or_default();
    let count = catalogue.len();
    let _ = WORDS.set(catalogue);
    Some((language, count))
}

/// Marks `english` for the template where a call to [`tr`] cannot stand,
/// as in a constant or a table, and gives it back as it is; [`tr`] then
/// translates it where it is shown.
pub const fn n_(english: &'static str) -> &'static str {
    english
}

/// `english` in the person's language, or as it is.
pub fn tr(english: &'static str) -> &'static str {
    WORDS
        .get()
        .and_then(|c| c.words.get(english))
        .map_or(english, String::as_str)
}

/// `english` in the person's language with each `{name}` replaced by its
/// value, as translators keep the names: `trf("Workspace {n}", &[("n",
/// "2")])`.
pub fn trf(english: &'static str, values: &[(&str, &str)]) -> String {
    let mut out = tr(english).to_string();
    for (name, value) in values {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    out
}

/// The words marked with `tr("...")`, `trf("...", ...)` or `n_("...")`
/// in `source`, in order, each once: what the part's template lists.
pub fn marked(source: &str) -> Vec<String> {
    let mut found = Vec::new();
    for call in ["tr(", "trf(", "n_("] {
        let mut rest = source;
        while let Some(at) = rest.find(call) {
            // Not part of a longer name, such as `attr(`.
            let before = rest[..at].chars().next_back();
            rest = &rest[at + call.len()..];
            if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                continue;
            }
            // The string may start on the next line, as rustfmt puts it.
            let Some(after) = rest.trim_start().strip_prefix('"') else {
                continue;
            };
            rest = after;
            let mut text = String::new();
            let mut chars = rest.chars();
            let mut closed = false;
            while let Some(c) = chars.next() {
                match c {
                    '"' => {
                        closed = true;
                        break;
                    }
                    '\\' => match chars.next() {
                        Some('n') => text.push('\n'),
                        Some('t') => text.push('\t'),
                        // A line's end escaped: the next line's leading
                        // spaces are left out, as Rust does.
                        Some('\n') => {
                            let skipped = chars.as_str().trim_start();
                            chars = skipped.chars();
                        }
                        Some(other) => text.push(other),
                        None => break,
                    },
                    c => text.push(c),
                }
            }
            if closed && !text.is_empty() {
                found.push((source.len() - rest.len(), text));
            }
        }
    }
    found.sort_by_key(|(at, _)| *at);
    let mut words: Vec<String> = Vec::new();
    for (_, text) in found {
        if !words.contains(&text) {
            words.push(text);
        }
    }
    words
}

/// The template a part's words make: a header, then each word with an
/// empty translation.
pub fn template(part: &str, words: &[String]) -> String {
    let mut out = format!(
        "# The words {part} shows people, gathered from its source by a test in\n\
         # crates/edel (EDEL_WRITE_DOCS=1 cargo test -p edel i18n writes it).\n\
         # A language's translation is LANG/{part}.po beside it (M5.24).\n\
         msgid \"\"\n\
         msgstr \"\"\n\
         \"Content-Type: text/plain; charset=UTF-8\\n\"\n"
    );
    for word in words {
        out.push_str(&format!("\nmsgid {}\nmsgstr \"\"\n", quote(word)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_po_file_gives_its_translated_words_and_skips_the_rest() {
        let po = r#"# A comment
msgid ""
msgstr ""
"Content-Type: text/plain; charset=UTF-8\n"

msgid "Type to search"
msgstr "Suchen"

msgid "Two "
"lines"
msgstr "Zwei "
"Zeilen"

#, fuzzy
msgid "Unsure"
msgstr "Unsicher"

msgid "Untranslated"
msgstr ""

msgid "Say \"hi\"\n"
msgstr "Sag \"hallo\"\n"
"#;
        let c = parse_po(po);
        assert_eq!(c.get("Type to search"), Some("Suchen"));
        assert_eq!(c.get("Two lines"), Some("Zwei Zeilen"));
        assert_eq!(c.get("Unsure"), None, "fuzzy is not trusted");
        assert_eq!(c.get("Untranslated"), None);
        assert_eq!(c.get("Say \"hi\"\n"), Some("Sag \"hallo\"\n"));
        assert_eq!(c.get(""), None, "the header is no word");
        assert_eq!(c.len(), 3);
    }

    #[test]
    fn marked_words_are_found_in_order_once_and_round_trip_through_a_template() {
        let source = r#"
            let a = tr("Type to search");
            let b = trf(
                "Workspace {n}",
                &[("n", "2")],
            );
            let c = attr("not a word");
            let d = tr("Type to search");
            let e = tr("A \"quote\"");
            const F: &str = n_("Two \
                               lines");
        "#;
        let words = marked(source);
        assert_eq!(
            words,
            [
                "Type to search",
                "Workspace {n}",
                "A \"quote\"",
                "Two lines"
            ]
        );
        let pot = template("shell-ui", &words);
        assert!(pot.contains("msgid \"A \\\"quote\\\"\""));
        // A template filled in is read back word for word.
        let filled = pot.replace(
            "msgid \"Type to search\"\nmsgstr \"\"",
            "msgid \"Type to search\"\nmsgstr \"Suchen\"",
        );
        assert_eq!(parse_po(&filled).get("Type to search"), Some("Suchen"));
    }

    #[test]
    fn an_untranslated_word_stays_english_and_values_fill_their_names() {
        assert_eq!(tr("Nothing translates this"), "Nothing translates this");
        assert_eq!(
            trf("Workspace {n} of {m}", &[("n", "2"), ("m", "4")]),
            "Workspace 2 of 4"
        );
    }

    /// Each part's template lists exactly the words its source marks; with
    /// EDEL_WRITE_DOCS set it is written afresh.
    #[test]
    fn each_parts_template_is_gathered_from_its_source() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for (part, places) in PARTS {
            let mut sources = Vec::new();
            let mut dirs = Vec::new();
            for place in *places {
                let path = root.join(place);
                if path.is_dir() {
                    dirs.push(path);
                } else {
                    sources.push(path);
                }
            }
            while let Some(d) = dirs.pop() {
                let mut entries: Vec<_> = std::fs::read_dir(&d)
                    .unwrap()
                    .flatten()
                    .map(|e| e.path())
                    .collect();
                entries.sort();
                for path in entries {
                    if path.is_dir() {
                        dirs.push(path);
                    } else if path.extension().is_some_and(|e| e == "rs") {
                        sources.push(path);
                    }
                }
            }
            sources.sort();
            let mut words: Vec<String> = Vec::new();
            for path in sources {
                // Tests' words are not shown to people.
                let text = std::fs::read_to_string(&path).unwrap();
                let shown = text.split("#[cfg(test)]").next().unwrap_or("");
                for word in marked(shown) {
                    if !words.contains(&word) {
                        words.push(word);
                    }
                }
            }
            let path = root.join(format!("po/{part}.pot"));
            let want = template(part, &words);
            if std::env::var_os("EDEL_WRITE_DOCS").is_some() {
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(&path, &want).unwrap();
            }
            let have = std::fs::read_to_string(&path).unwrap_or_default();
            assert!(
                have == want,
                "po/{part}.pot is not what {places:?} mark; run EDEL_WRITE_DOCS=1 cargo test -p edel i18n"
            );
        }
    }

    /// The parts whose words are translated, and where their source is:
    /// Settings shows the pages' titles `edel::settings` keeps.
    const PARTS: &[(&str, &[&str])] = &[
        ("shell-ui", &["crates/shell-ui/src"]),
        (
            "settings",
            &["crates/settings/src", "crates/edel/src/settings.rs"],
        ),
    ];
}
