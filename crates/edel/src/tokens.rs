//! Design tokens (DESIGN-PRINCIPLES, ADR-004): the colours and sizes
//! everything is drawn with, from `design/tokens.toml`. The repository's
//! file is built in, so a machine whose copy is missing or broken still
//! draws the release's look. Reading is lenient (ADR-008): an unknown key
//! or a value that does not parse keeps the built-in value and is reported,
//! never fatal; [`check`] is the strict reader for builds and tests.

use anyhow::{Context, Result, bail};
use toml::{Table, Value};

/// Where a machine may keep tokens of its own, which take the built-in
/// ones' place; no image ships one yet, so the parts draw with these.
pub const PATH: &str = crate::places::TOKENS_FILE;

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

    /// `#rrggbb`, or `#rrggbbaa` when not opaque, as CSS writes it.
    pub fn hex(self) -> String {
        let [r, g, b, a] = self.bytes();
        if a == 255 {
            format!("#{r:02x}{g:02x}{b:02x}")
        } else {
            format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
        }
    }
}

/// Light or dark (M5.5c): which colours the tokens give. `[colour]` holds
/// the dark ones and `[colour.light]` the light ones; sizes and fonts are
/// the same in both. Light is the release's default, as ADR-008 and the
/// mockups have it (M5.12a).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Scheme {
    Dark,
    #[default]
    Light,
}

impl Scheme {
    /// `appearance.mode`'s value: `light`, `dark`, or `auto`,
    /// which is the release's choice, light in this one; none for anything
    /// else.
    pub fn parse(value: &str) -> Option<Scheme> {
        match value {
            "light" | "auto" => Some(Scheme::Light),
            "dark" => Some(Scheme::Dark),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Scheme::Dark => "dark",
            Scheme::Light => "light",
        }
    }
}

/// The image's tokens in `scheme` if it has a file, else the built-in
/// ones; what was skipped comes back as notes, never fatal (ADR-008).
pub fn load(scheme: Scheme) -> (Tokens, Vec<String>) {
    match std::fs::read_to_string(crate::places::found_shared(PATH)) {
        Ok(text) => Tokens::read_scheme(&text, scheme),
        Err(_) => (Tokens::built_in_scheme(scheme), Vec::new()),
    }
}

/// Where images keep GTK's colours from the tokens (M5.5b), which each
/// person's `~/.config/gtk-4.0/gtk.css` imports.
pub const GTK_CSS: &str = crate::places::GTK_CSS;

/// The tokens the compositor and shell-ui use.
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
    /// shell-ui's panel (M5.1b) and the text and icons on it.
    pub panel: Colour,
    pub panel_text: Colour,
    /// The one accent: what is chosen or shown, such as the shown
    /// workspace's button (M5.2c).
    pub accent: Colour,
    /// The panel's height, logical pixels.
    pub panel_height: u32,
    /// The panel's text, logical pixels per em.
    pub panel_text_size: u32,
    /// The panel's smaller text, such as the clock's date, logical
    /// pixels per em.
    pub panel_text_small_size: u32,
    /// A button in the panel: its height, the app icon in it and the
    /// shell's own icon (the menu's, the layout toggle's), logical
    /// pixels (M5.29, from the mockups).
    pub panel_control: u32,
    pub panel_icon: u32,
    pub panel_glyph: u32,
    /// Corner radii, logical pixels (M5.5e): windows' (M5.14), menus' and
    /// the panel's fillets', and controls': the panel's buttons and rows.
    pub radius_window: u32,
    pub radius_menu: u32,
    pub radius_control: u32,
    /// Small things inside a window: a sidebar's rows, a value's box.
    pub radius_small: u32,
    /// The one shadow, cast by one light above (M5.5e): its colour, how
    /// far it blurs and how far down it falls, logical pixels.
    pub shadow: Colour,
    /// Apps' own surfaces (M5.6a): a window's page, the cards raised on
    /// it, and the hairlines round cards and between rows.
    pub window: Colour,
    pub card: Colour,
    pub line: Colour,
    /// Text and icons on the accent.
    pub accent_text: Colour,
    /// The hairline round windows and along the panels' inner edge.
    pub edge: Colour,
    pub shadow_blur: u32,
    pub shadow_offset: u32,
    /// A row in a menu or list, logical pixels.
    pub row: u32,
    /// Apps' text, in logical pixels (M5.6a).
    pub text_size: u32,
    /// The interface font's family.
    pub font: String,
}

impl Tokens {
    /// GTK's named colours from the tokens (M5.5b), as `@define-color`
    /// lines GTK4 and libadwaita apps read from `gtk.css`: `dark`'s, then
    /// `light`'s inside `@media (prefers-color-scheme: light)` (M5.5c),
    /// which GTK matches when the settings portal says light.
    pub fn gtk_css(dark: &Tokens, light: &Tokens) -> String {
        let mut css = String::from(
            "/* Edel OS: GTK's named colours from design/tokens.toml (M5.5b),\n   \
             written by edel::tokens::Tokens::gtk_css; a cargo test keeps\n   \
             this file equal to it. GTK picks the light ones when the\n   \
             settings portal says light (M5.5c). */\n",
        );
        for (name, value) in dark.gtk_colours() {
            css.push_str(&format!("@define-color {name} {value};\n"));
        }
        css.push_str("\n@media (prefers-color-scheme: light) {\n");
        for (name, value) in light.gtk_colours() {
            css.push_str(&format!("  @define-color {name} {value};\n"));
        }
        css.push_str("}\n");
        css
    }

    /// The docs site's colours, radii and font from the tokens (the docs
    /// site, M8.10a), as CSS custom properties, `dark`'s and then
    /// `light`'s, for mdBook's dark and light themes: the page is drawn
    /// as a GTK app's view is, the sidebar as its window, so the site
    /// looks like the desktop's own apps. `docs/theme/tokens.css` must
    /// equal it, and `docs/theme/edel.css` lays the site out with it.
    pub fn site_css(dark: &Tokens, light: &Tokens) -> String {
        let mut css = String::from(
            "/* Edel OS: the docs site's colours and sizes from design/tokens.toml,\n   \
             written by edel::tokens::Tokens::site_css; a cargo test keeps this\n   \
             file equal to it (EDEL_WRITE_DOCS=1 cargo test -p edel site_css).\n   \
             Never edit it by hand: change the tokens. */\n",
        );
        for (selector, tokens) in [
            (":root, .coal, .navy, .ayu", dark),
            (".light, .rust", light),
        ] {
            css.push_str(&format!("\n{selector} {{\n"));
            for (name, value) in tokens.site_colours() {
                css.push_str(&format!("  --edel-{name}: {value};\n"));
            }
            css.push_str("}\n");
        }
        css.push_str(&format!(
            "\n:root {{\n  --edel-radius-window: {}px;\n  --edel-radius-menu: {}px;\n  --edel-radius-control: {}px;\n  --edel-font: \"{}\";\n}}\n",
            dark.radius_window, dark.radius_menu, dark.radius_control, dark.font
        ));
        css
    }

    /// The site's colour roles, the same as GTK's in [`Tokens::gtk_css`]:
    /// the page as a view (the panel's colour), the sidebar as a window
    /// (the background), code and tables as header bars (the title bar).
    fn site_colours(&self) -> [(&'static str, String); 9] {
        [
            ("page", self.panel.hex()),
            ("sidebar", self.background.hex()),
            ("raised", self.title_bar.hex()),
            ("hover", self.title_bar_focused.hex()),
            ("border", self.title_button_hover.hex()),
            ("text", self.title_text.hex()),
            ("text-muted", self.title_text_unfocused.hex()),
            ("accent", self.accent.hex()),
            ("shadow", self.shadow.hex()),
        ]
    }

    /// GTK's named colours from these tokens: the windows' page and
    /// text, lists, cards, popovers and dialogs as raised cards, the
    /// header bars as our title bars, sidebars as the panel, and the
    /// accent.
    fn gtk_colours(&self) -> [(&'static str, String); 18] {
        let text = self.title_text.hex();
        [
            ("accent_color", self.accent.hex()),
            ("accent_bg_color", self.accent.hex()),
            ("accent_fg_color", self.accent_text.hex()),
            ("window_bg_color", self.window.hex()),
            ("window_fg_color", text.clone()),
            ("view_bg_color", self.card.hex()),
            ("view_fg_color", text.clone()),
            ("headerbar_bg_color", self.title_bar.hex()),
            ("headerbar_fg_color", text.clone()),
            ("headerbar_backdrop_color", self.background.hex()),
            ("sidebar_bg_color", self.panel.hex()),
            ("sidebar_fg_color", text.clone()),
            ("popover_bg_color", self.card.hex()),
            ("popover_fg_color", text.clone()),
            ("dialog_bg_color", self.window.hex()),
            ("dialog_fg_color", text.clone()),
            ("card_bg_color", self.card.hex()),
            ("card_fg_color", text),
        ]
    }

    /// The release's tokens, from [`BUILT_IN`].
    pub fn built_in() -> Tokens {
        check(BUILT_IN).expect("design/tokens.toml is checked by the tests")
    }

    /// The release's tokens in `scheme`.
    pub fn built_in_scheme(scheme: Scheme) -> Tokens {
        let mut tokens = Tokens::built_in();
        if scheme == Scheme::Light {
            let table: Table = BUILT_IN.parse().expect("checked by the tests");
            tokens.apply_light(&table, &mut Vec::new());
        }
        tokens
    }

    /// [`Tokens::read`], then, for the light scheme, `[colour.light]` over
    /// the colours.
    pub fn read_scheme(text: &str, scheme: Scheme) -> (Tokens, Vec<String>) {
        let mut tokens = Tokens::built_in_scheme(scheme);
        let mut notes = Vec::new();
        match text.parse::<Table>() {
            Ok(table) => {
                tokens.apply(&table, &mut notes);
                if scheme == Scheme::Light {
                    tokens.apply_light(&table, &mut notes);
                }
            }
            Err(e) => notes.push(format!("not TOML, so the built-in tokens are used: {e}")),
        }
        (tokens, notes)
    }

    /// `[colour.light]`'s colours over these.
    fn apply_light(&mut self, table: &Table, notes: &mut Vec<String>) {
        let light = table
            .get("colour")
            .and_then(Value::as_table)
            .and_then(|colours| colours.get("light"))
            .and_then(Value::as_table);
        if let Some(light) = light {
            self.apply_colours(light, notes);
        }
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
                    "format {value} is newer than this release's {FORMAT}; read what it understands"
                )),
                ("colour", Value::Table(colours)) => self.apply_colours(colours, notes),
                ("size", Value::Table(sizes)) => self.apply_sizes(sizes, notes),
                ("font", Value::Table(fonts)) => self.apply_fonts(fonts, notes),
                (key, _) => notes.push(format!("unknown key {key} ignored")),
            }
        }
    }

    fn apply_colours(&mut self, colours: &Table, notes: &mut Vec<String>) {
        for (key, value) in colours {
            let slot = match key.as_str() {
                // The light scheme's colours, read by `apply_light`.
                "light" if value.is_table() => continue,
                "background" => &mut self.background,
                "title_bar" => &mut self.title_bar,
                "title_bar_focused" => &mut self.title_bar_focused,
                "title_text" => &mut self.title_text,
                "title_text_unfocused" => &mut self.title_text_unfocused,
                "title_button_hover" => &mut self.title_button_hover,
                "title_close_hover" => &mut self.title_close_hover,
                "panel" => &mut self.panel,
                "panel_text" => &mut self.panel_text,
                "accent" => &mut self.accent,
                "shadow" => &mut self.shadow,
                "window" => &mut self.window,
                "card" => &mut self.card,
                "line" => &mut self.line,
                "accent_text" => &mut self.accent_text,
                "edge" => &mut self.edge,
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
                "panel" => (&mut self.panel_height, 200),
                "panel_text" => (&mut self.panel_text_size, 100),
                "panel_text_small" => (&mut self.panel_text_small_size, 100),
                "panel_control" => (&mut self.panel_control, 200),
                "panel_icon" => (&mut self.panel_icon, 100),
                "panel_glyph" => (&mut self.panel_glyph, 100),
                "radius_window" => (&mut self.radius_window, 100),
                "radius_menu" => (&mut self.radius_menu, 100),
                "radius_control" => (&mut self.radius_control, 100),
                "radius_small" => (&mut self.radius_small, 100),
                "shadow_blur" => (&mut self.shadow_blur, 100),
                "shadow_offset" => (&mut self.shadow_offset, 100),
                "row" => (&mut self.row, 200),
                "text" => (&mut self.text_size, 100),
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

impl Tokens {
    fn apply_fonts(&mut self, fonts: &Table, notes: &mut Vec<String>) {
        for (key, value) in fonts {
            if key != "interface" {
                notes.push(format!("unknown key font.{key} ignored"));
                continue;
            }
            match value.as_str().filter(|name| !name.trim().is_empty()) {
                Some(name) => self.font = name.trim().to_string(),
                None => notes.push(format!(
                    "font.{key} = {value} is not a font's name; the built-in one is used"
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
                "panel",
                "panel_text",
                "accent",
                "shadow",
                "window",
                "card",
                "line",
                "accent_text",
                "edge",
            ][..],
        ),
        (
            "size",
            &[
                "title_bar",
                "border",
                "title_text",
                "gap",
                "panel",
                "panel_text",
                "panel_text_small",
                "panel_control",
                "panel_icon",
                "panel_glyph",
                "radius_window",
                "radius_menu",
                "radius_control",
                "radius_small",
                "row",
                "text",
                "shadow_blur",
                "shadow_offset",
            ][..],
        ),
        ("font", &["interface"][..]),
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
    // The light scheme names every colour too (M5.5c).
    let colours = table.get("colour").and_then(Value::as_table);
    let light = colours
        .and_then(|c| c.get("light"))
        .and_then(Value::as_table);
    if let Some(colours) = colours {
        for key in colours.keys().filter(|k| *k != "light") {
            if light.is_none_or(|l| !l.contains_key(key)) {
                bail!("colour.light.{key} is missing");
            }
        }
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
        panel: BLACK,
        panel_text: BLACK,
        accent: BLACK,
        panel_height: 0,
        panel_text_size: 0,
        panel_text_small_size: 0,
        panel_control: 0,
        panel_icon: 0,
        panel_glyph: 0,
        radius_window: 0,
        radius_menu: 0,
        radius_control: 0,
        radius_small: 0,
        shadow: BLACK,
        window: BLACK,
        card: BLACK,
        line: BLACK,
        accent_text: BLACK,
        edge: BLACK,
        shadow_blur: 0,
        shadow_offset: 0,
        row: 0,
        text_size: 0,
        font: String::new(),
    };
    tokens.apply(&table, &mut notes);
    let mut light = tokens.clone();
    light.apply_light(&table, &mut notes);
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

    /// The shell feature's `gtk.css`, which images carry as [`GTK_CSS`].
    const GTK_CSS_FILE: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../features/shell/usr/share/edel/gtk.css"
    );

    /// Every new person's `~/.config/gtk-4.0/gtk.css` imports [`GTK_CSS`]
    /// by its place in `edel::places` (M5.27); with EDEL_WRITE_GTK_CSS set,
    /// the test writes it.
    #[test]
    fn a_person_imports_the_shipped_gtk_css() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../features/shell/etc/skel/.config/gtk-4.0/gtk.css"
        );
        let want = format!(
            "/* GTK4 and libadwaita apps read this file. It takes Edel OS's colours\n   \
             from the design tokens (M5.5b); add your own rules below it. */\n\
             @import url(\"file://{GTK_CSS}\");\n"
        );
        if std::env::var_os("EDEL_WRITE_GTK_CSS").is_some() {
            std::fs::write(path, &want).unwrap();
        }
        assert_eq!(std::fs::read_to_string(path).unwrap_or_default(), want);
    }

    #[test]
    fn the_shipped_gtk_css_is_the_tokens() {
        // With EDEL_WRITE_GTK_CSS set, the test writes the file instead,
        // after a token changed.
        let light = Tokens::built_in_scheme(Scheme::Light);
        let css = Tokens::gtk_css(&Tokens::built_in(), &light);
        if std::env::var_os("EDEL_WRITE_GTK_CSS").is_some() {
            std::fs::write(GTK_CSS_FILE, &css).unwrap();
        }
        let shipped = std::fs::read_to_string(GTK_CSS_FILE).unwrap_or_default();
        assert!(
            shipped == css,
            "features/shell/usr/share/edel/gtk.css differs from the tokens; run EDEL_WRITE_GTK_CSS=1 cargo test -p edel gtk_css"
        );
        assert!(css.contains("@define-color accent_bg_color #5b8ef5;"));
        // Apps' windows are the window token's colour in each scheme.
        let window = |t: &Tokens| format!("@define-color window_bg_color {};", t.window.hex());
        assert!(css.contains(&format!("\n{}", window(&Tokens::built_in()))));
        assert!(css.contains(&format!("  {}", window(&light))));
        // A changed token changes the file.
        let mut tokens = Tokens::built_in();
        tokens.accent = Colour::parse("#e5484d").unwrap();
        assert!(
            Tokens::gtk_css(&tokens, &light).contains("@define-color accent_bg_color #e5484d;")
        );
    }

    /// The docs site's `docs/theme/tokens.css` (M8.10a).
    const SITE_CSS_FILE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/theme/tokens.css");

    #[test]
    fn the_site_css_is_the_tokens() {
        // With EDEL_WRITE_DOCS set, the test writes the file instead,
        // after a token changed.
        let light = Tokens::built_in_scheme(Scheme::Light);
        let css = Tokens::site_css(&Tokens::built_in(), &light);
        if std::env::var_os("EDEL_WRITE_DOCS").is_some() {
            std::fs::write(SITE_CSS_FILE, &css).unwrap();
        }
        let shipped = std::fs::read_to_string(SITE_CSS_FILE).unwrap_or_default();
        assert!(
            shipped == css,
            "docs/theme/tokens.css differs from the tokens; run EDEL_WRITE_DOCS=1 cargo test -p edel site_css"
        );
        assert!(css.contains("  --edel-accent: #5b8ef5;"));
        assert!(css.contains("  --edel-page: #f6f7f9;"));
        assert!(css.contains("  --edel-radius-menu: 12px;"));
    }

    #[test]
    fn the_light_scheme_changes_the_colours_and_nothing_else() {
        let dark = Tokens::built_in_scheme(Scheme::Dark);
        let light = Tokens::built_in_scheme(Scheme::Light);
        assert_eq!(dark, Tokens::built_in());
        assert_eq!(light.background.hex(), "#dfe3ea");
        assert_eq!(light.panel.hex(), "#f6f7f9");
        assert_eq!(light.accent.hex(), "#2d6ae3");
        assert_eq!(light.panel_height, dark.panel_height);
        assert_eq!(light.font, dark.font);
        // A machine's file changes either scheme, and a light colour it
        // gives wins over its dark one there.
        let file =
            "format = 1\n[colour]\npanel = \"#000000\"\n[colour.light]\npanel = \"#ffffff\"\n";
        let (light, notes) = Tokens::read_scheme(file, Scheme::Light);
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(light.panel.hex(), "#ffffff");
        assert_eq!(light.background.hex(), "#dfe3ea");
        let (dark, _) = Tokens::read_scheme(file, Scheme::Dark);
        assert_eq!(dark.panel.hex(), "#000000");
        // Light is the release's default (M5.12a), and auto is it.
        assert_eq!(Scheme::parse("auto"), Some(Scheme::Light));
        assert_eq!(Scheme::default(), Scheme::Light);
        assert_eq!(Scheme::parse("sepia"), None);
    }

    #[test]
    fn check_wants_every_colour_in_the_light_scheme() {
        let missing = BUILT_IN.replace("panel = \"#f6f7f9\"\n", "");
        let e = format!("{:#}", check(&missing).unwrap_err());
        assert!(e.contains("colour.light.panel is missing"), "{e}");
    }

    #[test]
    fn hex_writes_alpha_only_when_not_opaque() {
        assert_eq!(Colour::parse("#24272e").unwrap().hex(), "#24272e");
        assert_eq!(Colour::parse("#24272e80").unwrap().hex(), "#24272e80");
    }

    #[test]
    fn the_built_in_tokens_pass_the_strict_check() {
        let tokens = check(BUILT_IN).unwrap();
        assert_eq!(tokens, Tokens::built_in());
        assert_eq!(tokens.title_bar_height, 28);
        assert_eq!((tokens.radius_control, tokens.row), (7, 36));
        // The mockups' second round (M5.5e): windows 10, menus 12,
        // controls 7, and one shadow from above.
        assert_eq!(
            (
                tokens.radius_window,
                tokens.radius_menu,
                tokens.radius_control
            ),
            (10, 12, 7)
        );
        assert_eq!((tokens.shadow_blur, tokens.shadow_offset), (24, 6));
        assert!(tokens.shadow.a > 0.0 && tokens.shadow.a < 1.0);
        assert_eq!(tokens.font, "Inter");
        let (other, notes) = Tokens::read("[font]\ninterface = \"Noto Sans\"\n[size]\nrow = 40");
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!((other.font.as_str(), other.row), ("Noto Sans", 40));
        let (_, notes) = Tokens::read("[font]\ninterface = \" \"");
        assert_eq!(notes.len(), 1, "an empty name is skipped");
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
