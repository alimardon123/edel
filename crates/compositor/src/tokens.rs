//! Design tokens (DESIGN-PRINCIPLES, ADR-004): the colours and sizes
//! everything is drawn with, from `design/tokens.toml`. The repository's
//! file is built in, so a machine whose copy is missing or broken still
//! draws the release's look. Reading is lenient (ADR-008): an unknown key
//! or a value that does not parse keeps the built-in value and is reported,
//! never fatal; [`check`] is the strict reader for builds and tests.

use anyhow::{Context, Result, bail};
use toml::{Table, Value};

/// Where images keep the tokens (M4.2b).
pub const PATH: &str = "/usr/share/edel/design/tokens.toml";

/// The repository's tokens, the release's defaults.
pub const BUILT_IN: &str = include_str!("../../../design/tokens.toml");

/// The only tokens format so far.
pub const FORMAT: i64 = 1;

/// An sRGB colour with alpha, each channel 0 to 1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Colour {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Colour {
    /// `#rrggbb` or `#rrggbbaa`.
    pub fn parse(text: &str) -> Option<Colour> {
        let hex = text.strip_prefix('#')?;
        if !(hex.len() == 6 || hex.len() == 8) || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let channel = |i: usize| {
            hex.get(i..i + 2)
                .map(|c| u8::from_str_radix(c, 16).map_or(0.0, |v| f32::from(v) / 255.0))
        };
        Some(Colour {
            r: channel(0)?,
            g: channel(2)?,
            b: channel(4)?,
            a: channel(6).unwrap_or(1.0),
        })
    }

    pub fn rgba(self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a]
    }

    /// Red, green, blue and alpha, 0 to 255.
    pub fn bytes(self) -> [u8; 4] {
        [self.r, self.g, self.b, self.a].map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8)
    }
}

/// The tokens the compositor uses.
#[derive(Debug, Clone, PartialEq)]
pub struct Tokens {
    pub background: Colour,
    pub title_bar: Colour,
    pub title_bar_focused: Colour,
    pub title_text: Colour,
    pub title_text_unfocused: Colour,
    pub title_button_hover: Colour,
    pub title_close_hover: Colour,
    /// Logical pixels.
    pub title_bar_height: u32,
    pub border: u32,
    /// Logical pixels per em.
    pub title_text_size: u32,
    /// Between tiled windows and the screen's edges.
    pub gap: u32,
}

impl Tokens {
    /// The release's tokens, from [`BUILT_IN`].
    pub fn built_in() -> Tokens {
        check(BUILT_IN).expect("design/tokens.toml is checked by the tests")
    }

    /// The built-in tokens with every value `text` sets that parses; the
    /// notes say what was skipped and why.
    pub fn read(text: &str) -> (Tokens, Vec<String>) {
        let mut tokens = Tokens::built_in();
        let mut notes = Vec::new();
        match text.parse::<Table>() {
            Ok(table) => tokens.apply(&table, &mut notes),
            Err(e) => notes.push(format!("not TOML, so the built-in tokens are used: {e}")),
        }
        (tokens, notes)
    }

    fn apply(&mut self, table: &Table, notes: &mut Vec<String>) {
        for (key, value) in table {
            match (key.as_str(), value) {
                ("format", Value::Integer(n)) if *n <= FORMAT => {}
                ("format", value) => notes.push(format!(
                    "format {value} is newer than this compositor's {FORMAT}; read what it understands"
                )),
                ("colour", Value::Table(colours)) => self.apply_colours(colours, notes),
                ("size", Value::Table(sizes)) => self.apply_sizes(sizes, notes),
                (key, _) => notes.push(format!("unknown key {key} ignored")),
            }
        }
    }

    fn apply_colours(&mut self, colours: &Table, notes: &mut Vec<String>) {
        for (key, value) in colours {
            let slot = match key.as_str() {
                "background" => &mut self.background,
                "title_bar" => &mut self.title_bar,
                "title_bar_focused" => &mut self.title_bar_focused,
                "title_text" => &mut self.title_text,
                "title_text_unfocused" => &mut self.title_text_unfocused,
                "title_button_hover" => &mut self.title_button_hover,
                "title_close_hover" => &mut self.title_close_hover,
                _ => {
                    notes.push(format!("unknown key colour.{key} ignored"));
                    continue;
                }
            };
            match value.as_str().and_then(Colour::parse) {
                Some(colour) => *slot = colour,
                None => notes.push(format!(
                    "colour.{key} = {value} is not #rrggbb or #rrggbbaa; the built-in value is used"
                )),
            }
        }
    }

    fn apply_sizes(&mut self, sizes: &Table, notes: &mut Vec<String>) {
        for (key, value) in sizes {
            let (slot, max) = match key.as_str() {
                "title_bar" => (&mut self.title_bar_height, 200),
                "border" => (&mut self.border, 50),
                "title_text" => (&mut self.title_text_size, 100),
                "gap" => (&mut self.gap, 100),
                _ => {
                    notes.push(format!("unknown key size.{key} ignored"));
                    continue;
                }
            };
            match value.as_integer().filter(|n| (0..=max).contains(n)) {
                Some(n) => *slot = n as u32,
                None => notes.push(format!(
                    "size.{key} = {value} is not a whole number from 0 to {max}; the built-in value is used"
                )),
            }
        }
    }
}

/// Reads tokens strictly: every key known, every value valid, all present.
pub fn check(text: &str) -> Result<Tokens> {
    let table: Table = text.parse().context("parsing the tokens")?;
    for (section, keys) in [
        (
            "colour",
            &[
                "background",
                "title_bar",
                "title_bar_focused",
                "title_text",
                "title_text_unfocused",
                "title_button_hover",
                "title_close_hover",
            ][..],
        ),
        ("size", &["title_bar", "border", "title_text", "gap"][..]),
    ] {
        let found = table.get(section).and_then(Value::as_table);
        if let Some(key) = keys
            .iter()
            .find(|k| found.is_none_or(|t| !t.contains_key(**k)))
        {
            bail!("{section}.{key} is missing");
        }
    }
    if table.get("format").and_then(Value::as_integer) != Some(FORMAT) {
        bail!("format must be {FORMAT}");
    }
    let mut notes = Vec::new();
    let mut tokens = Tokens {
        background: BLACK,
        title_bar: BLACK,
        title_bar_focused: BLACK,
        title_text: BLACK,
        title_text_unfocused: BLACK,
        title_button_hover: BLACK,
        title_close_hover: BLACK,
        title_bar_height: 0,
        border: 0,
        title_text_size: 0,
        gap: 0,
    };
    tokens.apply(&table, &mut notes);
    if !notes.is_empty() {
        bail!("{}", notes.join("; "));
    }
    Ok(tokens)
}

const BLACK: Colour = Colour {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_built_in_tokens_pass_the_strict_check() {
        let tokens = check(BUILT_IN).unwrap();
        assert_eq!(tokens, Tokens::built_in());
        assert_eq!(tokens.title_bar_height, 28);
    }

    #[test]
    fn colours_parse_with_and_without_alpha() {
        let c = Colour::parse("#ff8000").unwrap();
        assert_eq!(c.rgba(), [1.0, 128.0 / 255.0, 0.0, 1.0]);
        assert_eq!(Colour::parse("#00000080").unwrap().a, 128.0 / 255.0);
        assert_eq!(
            Colour::parse("#3a404b").unwrap().bytes(),
            [0x3a, 0x40, 0x4b, 0xff]
        );
        for bad in ["ff8000", "#ff800", "#gg8000", "#ff8000801", "#ÿÿÿ"] {
            assert_eq!(Colour::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn reading_keeps_built_in_values_for_what_it_cannot_use() {
        let text = r##"
            format = 1
            shiny = true
            [colour]
            background = "#102030"
            title_bar = "red"
            glow = "#ffffff"
            [size]
            border = 900
        "##;
        let (tokens, notes) = Tokens::read(text);
        let built_in = Tokens::built_in();
        assert_eq!(tokens.background, Colour::parse("#102030").unwrap());
        assert_eq!(tokens.title_bar, built_in.title_bar);
        assert_eq!(tokens.border, built_in.border);
        assert_eq!(tokens.title_bar_height, built_in.title_bar_height);
        assert_eq!(notes.len(), 4, "{notes:?}");
        assert!(notes.iter().any(|n| n.contains("colour.glow")));
        assert!(check(text).is_err());
    }

    #[test]
    fn a_broken_or_newer_file_still_gives_tokens() {
        let (tokens, notes) = Tokens::read("not = [toml");
        assert_eq!(tokens, Tokens::built_in());
        assert_eq!(notes.len(), 1);
        let (_, notes) = Tokens::read("format = 2\n");
        assert!(notes[0].starts_with("format 2 is newer"), "{notes:?}");
    }

    #[test]
    fn the_strict_check_wants_every_token() {
        let without = BUILT_IN.replace("title_text = ", "# title_text = ");
        let err = check(&without).unwrap_err();
        assert!(
            err.to_string().contains("colour.title_text is missing"),
            "{err}"
        );
    }
}
