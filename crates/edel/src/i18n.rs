//! The words people read, in their language (roadmap M5.24a and M5.24b).
//! Each part that shows words (shell-ui, Settings, `edel`) marks every one
//! with [`tr`] or
//! [`trf`]; a test gathers the marked words into the part's template,
//! `po/PART.pot`, and fails when the committed one differs. A language's
//! words are a gettext `.po` file, `LANG/PART.po` under
//! [`crate::places::LOCALE_DIR`], read at start by the small reader here
//! (no libintl, and no compiled `.mo` step: the text file is the one
//! format), and the language is `region.language`, the person's over the
//! machine's. A word the catalogue lacks, or has no translation for,
//! stays English.
//!
//! Words two parts show are translated once (ADR-010): `edel`'s template,
//! `po/edel.pot`, owns everything marked in this crate's sources, the
//! pages' titles and descriptions and the presets' names among them, and
//! [`init`] gives the Settings app that catalogue under its own.

use std::cell::RefCell;
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

    /// `other`'s words added under this one's: a word both translate keeps
    /// this one's.
    fn under(mut self, other: Catalogue) -> Catalogue {
        for (english, theirs) in other.words {
            self.words.entry(english).or_insert(theirs);
        }
        self
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

thread_local! {
    /// A catalogue one thread uses instead of the machine's, for tests.
    static OVERRIDE: RefCell<Option<Catalogue>> = const { RefCell::new(None) };
}

/// The parts whose words a part shows beside its own: the Settings app
/// shows `edel`'s pages' titles and presets' names, and shell-ui's quick
/// settings (M5.9a) the battery's line and the name of a page, which
/// `po/edel.pot` owns.
fn shared_with(part: &str) -> &'static [&'static str] {
    match part {
        "settings" | "shell-ui" => &["edel"],
        _ => &[],
    }
}

/// `part`'s catalogue in `language`, else in its base language.
fn read_catalogue(language: &str, part: &str) -> Catalogue {
    // `pt_BR` reads `pt_BR`'s words, else `pt`'s.
    let base = language.split(['_', '.', '@']).next().unwrap_or(language);
    [language, base]
        .iter()
        .find_map(|name| std::fs::read_to_string(places::catalogue(name, part)).ok())
        .map(|text| parse_po(&text))
        .unwrap_or_default()
}

/// Reads `part`'s catalogue in the person's language, once, at start;
/// returns the language and how many words it translates.
pub fn init(part: &str) -> Option<(String, usize)> {
    let language = language()?;
    let mut catalogue = read_catalogue(&language, part);
    for shared in shared_with(part) {
        catalogue = catalogue.under(read_catalogue(&language, shared));
    }
    let count = catalogue.len();
    let _ = WORDS.set(catalogue);
    Some((language, count))
}

/// Runs `f` with `catalogue` as this thread's words, whatever the machine
/// has: a test of a message in a made-up language.
pub fn with_catalogue<R>(catalogue: Catalogue, f: impl FnOnce() -> R) -> R {
    struct Restore(Option<Catalogue>);
    impl Drop for Restore {
        fn drop(&mut self) {
            OVERRIDE.with(|o| *o.borrow_mut() = self.0.take());
        }
    }
    let _restore = Restore(OVERRIDE.with(|o| o.borrow_mut().replace(catalogue)));
    f()
}

/// Marks `english` for the template where a call to [`tr`] cannot stand,
/// as in a constant or a table, and gives it back as it is; [`tr`] then
/// translates it where it is shown.
pub const fn n_(english: &'static str) -> &'static str {
    english
}

/// `english` in the person's language, or None when no catalogue has it.
/// For words that are not literals, such as clap's help.
pub fn translate(english: &str) -> Option<String> {
    let mine = OVERRIDE.with(|o| {
        o.borrow()
            .as_ref()
            .map(|c| c.get(english).map(str::to_string))
    });
    match mine {
        Some(found) => found,
        None => WORDS.get()?.get(english).map(str::to_string),
    }
}

/// `english` in the person's language, or as it is.
pub fn tr(english: &'static str) -> &'static str {
    let mine = OVERRIDE.with(|o| {
        o.borrow().as_ref().map(|c| {
            c.get(english)
                .map(|s| &*Box::leak(s.to_string().into_boxed_str()))
        })
    });
    if let Some(found) = mine {
        // Only a test's words get here, and they are few.
        return found.unwrap_or(english);
    }
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

/// The words marked in the sources `places` name (files, or folders read
/// for their `.rs` files), relative to `root`, in order, each once. Tests'
/// words (after `#[cfg(test)]`) and comments are not shown to people, so
/// they are left out.
pub fn gather(root: &std::path::Path, places: &[&str]) -> Vec<String> {
    let mut sources = Vec::new();
    let mut dirs = Vec::new();
    for place in places {
        let path = root.join(place);
        if path.is_dir() {
            dirs.push(path);
        } else {
            sources.push(path);
        }
    }
    while let Some(d) = dirs.pop() {
        let mut entries: Vec<_> = std::fs::read_dir(&d)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", d.display()))
            .flatten()
            .map(|e| e.path())
            .collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                dirs.push(path);
            } else if path.extension().is_some_and(|e| e == "rs")
                // The reader names the marks, in strings, but has no words.
                && path.file_name().is_none_or(|n| n != "i18n.rs")
            {
                sources.push(path);
            }
        }
    }
    sources.sort();
    let mut words: Vec<String> = Vec::new();
    for path in sources {
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        let shown = text.split("#[cfg(test)]").next().unwrap_or("");
        let code: String = shown
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .flat_map(|l| [l, "\n"])
            .collect();
        for word in marked(&code) {
            if !words.contains(&word) {
                words.push(word);
            }
        }
    }
    words
}

/// Checks `po/PART.pot` under `root` against `words`, or writes it afresh
/// when `EDEL_WRITE_DOCS` is set.
pub fn check_template(root: &std::path::Path, part: &str, words: &[String], from: &str) {
    let path = root.join(format!("po/{part}.pot"));
    let want = template(part, words);
    if std::env::var_os("EDEL_WRITE_DOCS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &want).unwrap();
    }
    let have = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        have == want,
        "po/{part}.pot is not what {from} mark; run EDEL_WRITE_DOCS=1 cargo test -p edel i18n"
    );
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

    /// In a made-up language ("xx", M5.24b) the pages' titles and
    /// descriptions and the presets' names come out translated, and stay
    /// English where the catalogue lacks them or a person named the preset.
    #[test]
    fn pages_and_presets_are_translated_and_fall_back_to_english() {
        let layout = crate::settings::page("layout").unwrap();
        let xx = parse_po(&format!(
            "msgid {}\nmsgstr \"xx titel\"\n\nmsgid {}\nmsgstr \"xx about\"\n\nmsgid \"Mac-like\"\nmsgstr \"xx Mac\"\n",
            quote(layout.title),
            quote(layout.about)
        ));
        with_catalogue(xx, || {
            assert_eq!(tr(layout.title), "xx titel");
            assert_eq!(tr(layout.about), "xx about");
            assert_eq!(crate::presets::title("mac-like"), "xx Mac");
            // Not in the catalogue: English. A person's own preset is its
            // name with a capital.
            assert_eq!(crate::presets::title("hive"), "Hive");
            assert_eq!(crate::presets::title("my laptop"), "My laptop");
            let displays = crate::settings::page("displays").unwrap();
            assert_eq!(tr(displays.title), "Displays");
        });
        assert_eq!(tr(layout.title), "Layout");
        assert_eq!(crate::presets::title("mac-like"), "Mac-like");
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
    /// EDEL_WRITE_DOCS set it is written afresh. `edel`'s own is checked
    /// in main.rs, as its help comes from the command table.
    #[test]
    fn each_parts_template_is_gathered_from_its_source() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let owned_by_edel = gather(&root, &["crates/edel/src"]);
        for (part, places) in PARTS {
            let mut words = gather(&root, places);
            // The Settings app shows words `edel` marks, such as the pages'
            // titles; they are translated once, in `po/edel.pot`, which
            // `init` gives it as well (ADR-010).
            if shared_with(part).contains(&"edel") {
                words.retain(|w| !owned_by_edel.contains(w));
            }
            check_template(&root, part, &words, &format!("{places:?}"));
        }
    }

    /// The words of a template, as `msgid`s.
    fn words_of(part: &str) -> Vec<String> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let text = std::fs::read_to_string(root.join(format!("po/{part}.pot"))).unwrap();
        text.lines()
            .filter_map(|l| l.strip_prefix("msgid "))
            .map(unquote)
            .filter(|w| !w.is_empty())
            .collect()
    }

    /// A word `edel` shows is translated in `po/edel.pot` alone, however
    /// many parts show it (ADR-010): the Settings app reads that catalogue
    /// under its own, so its template leaves such words out.
    #[test]
    fn a_word_edel_owns_is_in_no_other_template() {
        let owned = words_of("edel");
        assert!(owned.len() > 100, "po/edel.pot should hold edel's words");
        for part in ["shell-ui", "settings"] {
            for word in words_of(part) {
                assert!(
                    !owned.contains(&word),
                    "{word:?} is in po/edel.pot and po/{part}.pot; leave it to edel's"
                );
            }
        }
    }

    /// The parts whose words are gathered here, and where their source is.
    /// Settings shows the pages' titles and the presets' names, which
    /// `edel` owns (`crates/edel/src`).
    const PARTS: &[(&str, &[&str])] = &[
        ("shell-ui", &["crates/shell-ui/src"]),
        ("settings", &["crates/settings/src"]),
    ];
}
