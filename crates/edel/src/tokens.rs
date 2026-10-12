//! Design tokens (DESIGN-PRINCIPLES, ADR-004): the colours and sizes
//! everything is drawn with, from `design/tokens.toml`. The repository's
//! file is built in, so a machine whose copy is missing or broken still
//! draws the release's look. Reading is lenient (ADR-008): an unknown key
//! or a value that does not parse keeps the built-in value and is reported,
//! never fatal; [`check`] is the strict reader for builds and tests.

use anyhow::{Context, Result, bail};
use toml::{Table, Value};
use toml_edit::DocumentMut;

use crate::i18n::{tr, trf};
use crate::settings::{self, Appearance, Colours, SettingsFile};
#[cfg(feature = "pictures")]
use crate::wallpapers::{self, Wallpaper};

/// Where a machine may keep tokens of its own, which take the built-in
/// ones' place; no image ships one yet, so the parts draw with these.
pub const PATH: &str = crate::places::TOKENS_FILE;

/// The repository's tokens, the release's defaults.
pub const BUILT_IN: &str = include_str!("../../../design/tokens.toml");

/// The only tokens format so far.
pub const FORMAT: i64 = 1;

/// The monospace font's family when the tokens name none (M5.12b5), the
/// font apps use for code and terminals. `design/tokens.toml` may name one
/// in `[font] monospace` later; the reader already takes it.
pub const MONOSPACE: &str = "Noto Sans Mono";

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

/// [`load`], then a person's appearance on top ([`Tokens::with_appearance`],
/// M5.12b1); the notes of both.
pub fn load_with(
    scheme: Scheme,
    accent: Option<&str>,
    colours: Option<&Colours>,
    from_wallpaper: Option<Colour>,
) -> (Tokens, Vec<String>) {
    let (tokens, mut notes) = load(scheme);
    let (tokens, more) = tokens.with_appearance(accent, colours, scheme, from_wallpaper);
    notes.extend(more);
    (tokens, notes)
}

/// The tokens of `scheme` with the person's appearance from two settings
/// files' texts (M5.12b1): the accent the person's file names, else the
/// machine's; the colours laid over by [`settings::colours_over`]. A file
/// that does not read counts as absent, and what is wrong in the tokens
/// comes back as notes, as [`load_with`] gives them.
pub fn load_from_files(
    scheme: Scheme,
    machine: Option<&str>,
    person: Option<&str>,
    from_wallpaper: Option<Colour>,
) -> (Tokens, Vec<String>) {
    let ((machine, mut notes), (person, more)) = (read_file(machine), read_file(person));
    notes.extend(more);
    let accent = accent_between(machine.as_ref(), person.as_ref());
    let colours = settings::colours_over(
        machine.as_ref().and_then(|f| f.appearance.colours.as_ref()),
        person.as_ref().and_then(|f| f.appearance.colours.as_ref()),
    );
    let (tokens, more) = load_with(scheme, accent.as_deref(), colours.as_ref(), from_wallpaper);
    notes.extend(more);
    let shape = Shape::between(machine.as_ref(), person.as_ref());
    let (tokens, more) = tokens.with_shape(&shape);
    notes.extend(more);
    let text = Text::between(machine.as_ref(), person.as_ref());
    let (tokens, more) = tokens.with_text(&text);
    notes.extend(more);
    (tokens, notes)
}

/// A settings file's text read leniently; none when there is no text or
/// the text is not a file this release reads (M5.12b1). The notes are the
/// windows' shape values the reader left out, which the file's own check
/// names too (M5.12b2): a whole number out of its range is dropped there.
fn read_file(text: Option<&str>) -> (Option<SettingsFile>, Vec<String>) {
    let Some(text) = text else {
        return (None, Vec::new());
    };
    match settings::read(text) {
        Ok(read) => {
            let notes = read
                .problems
                .iter()
                .filter(|p| {
                    settings::SHAPE_KEYS.contains(&p.key.as_str())
                        || settings::DEPTH_KEYS.contains(&p.key.as_str())
                        || settings::TEXT_KEYS.contains(&p.key.as_str())
                })
                .map(ToString::to_string)
                .collect();
            // The theme in use applies under the file's own values (M5.12b6a).
            (Some(crate::settings::in_use(&read.file)), notes)
        }
        Err(_) => (None, Vec::new()),
    }
}

/// The accent the person's file names, else the machine's (M5.12b1).
fn accent_between(machine: Option<&SettingsFile>, person: Option<&SettingsFile>) -> Option<String> {
    person
        .and_then(|file| file.appearance.accent.clone())
        .or_else(|| machine.and_then(|file| file.appearance.accent.clone()))
}

/// The tokens of `scheme` as this machine's people see them (M5.12b1): the
/// machine's and the person's settings files, found through
/// [`crate::places`], and, when the accent is `wallpaper`, the colour of
/// the wallpaper in that scheme. Every reader on a machine calls this one
/// function, so the compositor, shell-ui and Settings show the same colours.
pub fn on_this_machine(scheme: Scheme) -> (Tokens, Vec<String>) {
    let machine = text_at(&crate::places::machine_settings());
    let person = crate::places::person_settings().and_then(|path| text_at(&path));
    #[cfg(feature = "pictures")]
    let from_wallpaper = wallpaper_accent(scheme, machine.as_deref(), person.as_deref());
    #[cfg(not(feature = "pictures"))]
    let from_wallpaper = None;
    load_from_files(
        scheme,
        machine.as_deref(),
        person.as_deref(),
        from_wallpaper,
    )
}

/// The text of the settings file at `path`, or of the former name it had,
/// when one reads (M5.12b1).
fn text_at(path: &std::path::Path) -> Option<String> {
    std::fs::read_to_string(crate::places::found(path)).ok()
}

/// The accent a wallpaper gives in `scheme` (M5.12b1), when the accent is
/// `wallpaper`: the colour of the picture the wallpaper names there, read
/// by [`crate::pictures::accent_of`] and fitted to read as an accent by
/// [`accent_for`]; a colour wallpaper's colour, fitted the same way; none
/// for no picture, which leaves the release's accent.
#[cfg(feature = "pictures")]
fn wallpaper_accent(scheme: Scheme, machine: Option<&str>, person: Option<&str>) -> Option<Colour> {
    let ((machine, _), (person, _)) = (read_file(machine), read_file(person));
    if accent_between(machine.as_ref(), person.as_ref()).as_deref() != Some(WALLPAPER_ACCENT) {
        return None;
    }
    let over = |key: fn(&SettingsFile) -> Option<&String>| {
        person
            .as_ref()
            .and_then(key)
            .or_else(|| machine.as_ref().and_then(key))
            .map(String::as_str)
    };
    let dark = scheme == Scheme::Dark;
    let value = wallpapers::value_for(
        over(|f| f.appearance.wallpaper.as_ref()),
        over(|f| f.appearance.wallpaper_dark.as_ref()),
        dark,
    );
    let panel = load(scheme).0;
    let colour = match wallpapers::on_this_machine(value, dark) {
        Wallpaper::Background => None,
        Wallpaper::Colour(colour) => Some(colour),
        Wallpaper::Picture(path) => crate::pictures::accent_of(&path),
    };
    colour.map(|colour| accent_for(colour, &panel))
}

/// A colour fitted to read as an accent on the panel: [`nearest_passing`]
/// of it against the panel at [`MARK_CONTRAST`], so a wallpaper's colour
/// always reads as an accent (M5.12b1).
pub fn accent_for(colour: Colour, tokens: &Tokens) -> Colour {
    nearest_passing(colour, tokens.panel, MARK_CONTRAST)
}

/// The accent a file names to take the wallpaper's colour (M5.12b1).
pub const WALLPAPER_ACCENT: &str = "wallpaper";

/// `#rrggbb` as an opaque colour; none for anything else, as an accent is
/// opaque (M5.12b1).
fn opaque_colour(text: &str) -> Option<Colour> {
    (text.len() == SIX_DIGIT_LEN)
        .then(|| Colour::parse(text))
        .flatten()
}

/// The length of `#rrggbb`, with its `#`.
const SIX_DIGIT_LEN: usize = 7;

/// The accents named in `design/tokens.toml`'s `[accent]` table, in the
/// file's order, each with its light and its dark shade (M5.12b1). They
/// are the release's: the names a person may pick are the built-in file's.
pub fn accents() -> Vec<(String, Colour, Colour)> {
    let doc: DocumentMut = BUILT_IN.parse().expect("checked by the tests");
    let Some(table) = doc.get("accent").and_then(|item| item.as_table()) else {
        return Vec::new();
    };
    table
        .iter()
        .filter_map(|(name, item)| {
            let shade = |key: &str| item.as_table()?.get(key)?.as_str().and_then(Colour::parse);
            Some((name.to_string(), shade("light")?, shade("dark")?))
        })
        .collect()
}

/// The shade of accent `name` in `scheme`, from [`accents`]; none when the
/// release names no such accent (M5.12b1).
pub fn accent_named(name: &str, scheme: Scheme) -> Option<Colour> {
    accents()
        .into_iter()
        .find(|(accent, _, _)| accent == name)
        .map(|(_, light, dark)| match scheme {
            Scheme::Light => light,
            Scheme::Dark => dark,
        })
}

/// Whether `value` is an accent a person may choose: a name from
/// [`accents`], `wallpaper` or `#rrggbb` (M5.12b1).
pub fn is_accent(value: &str) -> bool {
    value == WALLPAPER_ACCENT
        || accents().iter().any(|(name, _, _)| name == value)
        || opaque_colour(value).is_some()
}

/// WCAG 2's relative luminance weights for red, green and blue (M5.12b1).
const RED_WEIGHT: f32 = 0.2126;
const GREEN_WEIGHT: f32 = 0.7152;
const BLUE_WEIGHT: f32 = 0.0722;
/// WCAG 2's curve from an sRGB channel to linear light: a straight line
/// below the cutoff, a power above it.
const LINEAR_CUTOFF: f32 = 0.03928;
const LINEAR_SLOPE: f32 = 12.92;
const LINEAR_OFFSET: f32 = 0.055;
const LINEAR_SCALE: f32 = 1.055;
const LINEAR_GAMMA: f32 = 2.4;
/// The amount WCAG 2 adds to both luminances before their ratio.
const CONTRAST_FLARE: f32 = 0.05;

/// Text needs this much contrast on its surface, WCAG 2's AA (M5.12b1).
pub const TEXT_CONTRAST: f32 = 4.5;
/// An icon, a focus ring or a filled control needs this much (M5.12b1).
pub const MARK_CONTRAST: f32 = 3.0;

/// The pairs the release holds to a contrast: (foreground token, surface
/// token, ratio) (M5.12b1). The dimmed text of windows without the keyboard
/// is left out on purpose, as WCAG leaves out inactive parts; the overview's
/// light top is left out until M5.12b3's glass.
pub const PAIRS: &[(&str, &str, f32)] = &[
    ("panel_text", "panel", TEXT_CONTRAST),
    ("title_text", "title_bar_focused", TEXT_CONTRAST),
    ("title_text", "title_bar", TEXT_CONTRAST),
    ("accent_text", "accent", TEXT_CONTRAST),
    ("backdrop_text", "backdrop_deep", TEXT_CONTRAST),
    ("backdrop_text", "lock_backdrop", TEXT_CONTRAST),
    ("backdrop_text", "lock_backdrop_deep", TEXT_CONTRAST),
    ("accent", "panel", MARK_CONTRAST),
    ("accent", "window", MARK_CONTRAST),
    ("accent", "card", MARK_CONTRAST),
];

/// How far apart two colours read, WCAG 2's contrast ratio, from 1 to 21;
/// alpha is ignored (M5.12b1).
pub fn contrast(a: Colour, b: Colour) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    let (light, dark) = if la >= lb { (la, lb) } else { (lb, la) };
    (light + CONTRAST_FLARE) / (dark + CONTRAST_FLARE)
}

/// Tenths of a ratio, which a problem line shows: a ratio is rounded down,
/// so a pair that fails never reads better than it is (M5.12b1).
const TENTHS: f32 = 10.0;

/// `ratio` rounded down to a tenth.
fn shown_down(ratio: f32) -> f32 {
    (ratio * TENTHS).floor() / TENTHS
}

/// A colour's relative luminance, from 0 (black) to 1 (white), WCAG 2's
/// formula (M5.12b1).
fn luminance(colour: Colour) -> f32 {
    let linear = |channel: f32| {
        if channel <= LINEAR_CUTOFF {
            channel / LINEAR_SLOPE
        } else {
            ((channel + LINEAR_OFFSET) / LINEAR_SCALE).powf(LINEAR_GAMMA)
        }
    };
    RED_WEIGHT * linear(colour.r) + GREEN_WEIGHT * linear(colour.g) + BLUE_WEIGHT * linear(colour.b)
}

/// How much one step changes a colour's lightness, and how many steps
/// [`nearest_passing`] tries each way (M5.12b1): 1 percent, to 100 steps.
const LIGHTNESS_STEP: f32 = 0.01;
const MOST_LIGHTNESS_STEPS: u32 = 100;
/// The six sectors of the colour wheel, and the hue of each sector's edge.
const HUE_SECTORS: f32 = 6.0;

/// `colour` made lighter or darker, whichever reaches `ratio` against `on`
/// with the smaller change, in steps of 1 percent of lightness; `colour`
/// itself when it already does (M5.12b1).
pub fn nearest_passing(colour: Colour, on: Colour, ratio: f32) -> Colour {
    if contrast(colour, on) >= ratio {
        return colour;
    }
    let (hue, saturation, lightness) = to_hsl(colour);
    for step in 1..=MOST_LIGHTNESS_STEPS {
        let change = step as f32 * LIGHTNESS_STEP;
        for target in [lightness - change, lightness + change] {
            let candidate = from_hsl(hue, saturation, target.clamp(0.0, 1.0), colour.a);
            if contrast(candidate, on) >= ratio {
                return candidate;
            }
        }
    }
    // No colour in the steps reaches the ratio: the nearer of black and white.
    let black = from_hsl(0.0, 0.0, 0.0, colour.a);
    let white = from_hsl(0.0, 0.0, 1.0, colour.a);
    if contrast(black, on) >= contrast(white, on) {
        black
    } else {
        white
    }
}

/// A colour as hue (0 to 1), saturation and lightness (each 0 to 1).
pub(crate) fn to_hsl(colour: Colour) -> (f32, f32, f32) {
    let max = colour.r.max(colour.g).max(colour.b);
    let min = colour.r.min(colour.g).min(colour.b);
    let lightness = (max + min) / 2.0;
    let chroma = max - min;
    if chroma == 0.0 {
        return (0.0, 0.0, lightness);
    }
    let saturation = if lightness > 0.5 {
        chroma / (2.0 - max - min)
    } else {
        chroma / (max + min)
    };
    let sector = if max == colour.r {
        (colour.g - colour.b) / chroma
            + if colour.g < colour.b {
                HUE_SECTORS
            } else {
                0.0
            }
    } else if max == colour.g {
        (colour.b - colour.r) / chroma + 2.0
    } else {
        (colour.r - colour.g) / chroma + 4.0
    };
    (sector / HUE_SECTORS, saturation, lightness)
}

/// The colour with a hue, saturation and lightness, and `alpha` (M5.12b1).
fn from_hsl(hue: f32, saturation: f32, lightness: f32, alpha: f32) -> Colour {
    if saturation == 0.0 {
        return Colour {
            r: lightness,
            g: lightness,
            b: lightness,
            a: alpha,
        };
    }
    let q = if lightness < 0.5 {
        lightness * (1.0 + saturation)
    } else {
        lightness + saturation - lightness * saturation
    };
    let p = 2.0 * lightness - q;
    let channel = |t: f32| {
        let t = t.rem_euclid(1.0);
        if t < 1.0 / HUE_SECTORS {
            p + (q - p) * HUE_SECTORS * t
        } else if t < 3.0 / HUE_SECTORS {
            q
        } else if t < 4.0 / HUE_SECTORS {
            p + (q - p) * (4.0 / HUE_SECTORS - t) * HUE_SECTORS
        } else {
            p
        }
    };
    Colour {
        r: channel(hue + 2.0 / HUE_SECTORS),
        g: channel(hue),
        b: channel(hue - 2.0 / HUE_SECTORS),
        a: alpha,
    }
}

/// The text on an accent: whichever of the two schemes' `accent_text`
/// reads better on it (M5.12b1).
/// `base` with `percent` of `accent` mixed in, its alpha kept: the
/// focused title bar's tint.
fn tinted(base: Colour, accent: Colour, percent: u32) -> Colour {
    let k = percent.min(100) as f32 / 100.0;
    let mix = |a: f32, b: f32| a + (b - a) * k;
    Colour {
        r: mix(base.r, accent.r),
        g: mix(base.g, accent.g),
        b: mix(base.b, accent.b),
        a: base.a,
    }
}

fn text_on(accent: Colour) -> Colour {
    [
        Tokens::built_in_scheme(Scheme::Light).accent_text,
        Tokens::built_in().accent_text,
    ]
    .into_iter()
    .reduce(|best, text| {
        if contrast(text, accent) > contrast(best, accent) {
            text
        } else {
            best
        }
    })
    .unwrap_or(BLACK)
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
    /// Between tiled windows and the screen's edge, when it differs from
    /// `gap` (M5.12b2).
    pub gap_outer: u32,
    /// shell-ui's panel (M5.1b) and the text and icons on it.
    pub panel: Colour,
    pub panel_text: Colour,
    /// The one accent: what is chosen or shown, such as the shown
    /// workspace's button (M5.2c).
    pub accent: Colour,
    /// The panel's height, logical pixels: the medium size, the bar's.
    pub panel_height: u32,
    /// The small and large heights a panel's size may be (M5.31c).
    pub panel_small_height: u32,
    pub panel_large_height: u32,
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
    /// The windows' borders (M5.12b2): one without the keyboard, the
    /// focused one, and the colour the focused border ends in when it is a
    /// gradient from `window_border_focused`.
    pub window_border: Colour,
    pub window_border_focused: Colour,
    pub window_border_focused_end: Colour,
    /// The overview's backdrop where there is no wallpaper (M5.2j-b): a
    /// gradient from `backdrop` at the top to `backdrop_deep` at the bottom.
    pub backdrop: Colour,
    pub backdrop_deep: Colour,
    /// Text and marks drawn straight on the backdrop.
    pub backdrop_text: Colour,
    /// The lock and login screens' frosted glass (M5.10c): the field, the
    /// notifications' count and the corner's buttons, and the hairline
    /// round them.
    pub lock_glass: Colour,
    pub lock_glass_edge: Colour,
    /// What lies behind the lock and login screens without a wallpaper:
    /// a gradient from `lock_backdrop` to `lock_backdrop_deep`, lit by two
    /// soft glows (M5.10c).
    pub lock_backdrop: Colour,
    pub lock_backdrop_deep: Colour,
    pub lock_glow: Colour,
    pub lock_glow_second: Colour,
    /// The lock editor's stage: the deep navy it is filled with and the glow
    /// that lights it behind the picture (M5.10c7f).
    pub lock_edit_deep: Colour,
    pub lock_edit_glow: Colour,
    /// The lock and login screens' clock: its size, logical pixels, and
    /// its weight, 100 (thin) to 900 (black).
    pub lock_clock: u32,
    pub lock_clock_weight: u32,
    /// The panels' opacity in percent (M5.12b2, `appearance.panel_opacity`):
    /// only the bars and docks are drawn with it, never the cards and menus
    /// that share the panel's colour.
    pub panel_opacity: u32,
    /// The windows' opacity in percent (M5.12b3): the focused window, its
    /// title bar and border, and every other window; 100 is opaque.
    pub window_opacity: u32,
    pub window_opacity_unfocused: u32,
    /// How strongly the wallpaper blurs behind see-through panels, in
    /// percent (M5.12b3, `appearance.blur`); 0 shows it sharp.
    pub blur: u32,
    /// How much of the accent the focused title bar takes, in percent,
    /// laid on by [`Tokens::with_appearance`]; 0 keeps the bar neutral.
    pub title_tint: u32,
    pub shadow_blur: u32,
    pub shadow_offset: u32,
    /// A row in a menu or list, logical pixels.
    pub row: u32,
    /// Apps' text, in logical pixels (M5.6a).
    pub text_size: u32,
    /// The interface font's family.
    pub font: String,
    /// The monospace font's family (M5.12b5), `[font] monospace`.
    pub monospace: String,
    /// The text size in percent of the release's, as `appearance.text_size`
    /// sets it (M5.12b5); 100 is the release's sizes.
    pub text_percent: u32,
}

/// Every colour token by its name in `design/tokens.toml` (M5.12b1): one
/// line here names each colour, and [`Tokens::colour`], `apply_colours` and
/// `check` all read this one list, so a new colour token is one line.
macro_rules! colour_tokens {
    ($($name:ident),* $(,)?) => {
        /// The names of the colour tokens, in the order the file lists them.
        pub const COLOUR_NAMES: &[&str] = &[$(stringify!($name)),*];

        impl Tokens {
            /// The colour token called `name`; none for a name that is not one.
            pub fn colour(&self, name: &str) -> Option<Colour> {
                match name {
                    $(stringify!($name) => Some(self.$name),)*
                    _ => None,
                }
            }

            /// The colour token called `name`, to change it.
            fn slot(&mut self, name: &str) -> Option<&mut Colour> {
                match name {
                    $(stringify!($name) => Some(&mut self.$name),)*
                    _ => None,
                }
            }
        }
    };
}

colour_tokens! {
    background, title_bar, title_bar_focused, title_text, title_text_unfocused,
    title_button_hover, title_close_hover, panel, panel_text, accent, shadow,
    window, card, line, accent_text, edge, window_border, window_border_focused,
    window_border_focused_end, backdrop, backdrop_deep, backdrop_text,
    lock_glass, lock_glass_edge, lock_backdrop, lock_backdrop_deep, lock_glow,
    lock_glow_second, lock_edit_deep, lock_edit_glow,
}

impl Tokens {
    /// These tokens with a person's appearance on top (M5.12b1): `accent`,
    /// a named accent, a colour `#rrggbb`, or `wallpaper`, which takes
    /// `from_wallpaper` when given and else keeps the release's; then
    /// `colours`, any colour token by name, the shared ones first and the
    /// scheme's own (`light` or `dark`) over them. The text on a changed
    /// accent is whichever scheme's `accent_text` reads better on it.
    /// Lenient (ADR-008): an unknown name or a bad value is noted and
    /// skipped.
    pub fn with_appearance(
        mut self,
        accent: Option<&str>,
        colours: Option<&Colours>,
        scheme: Scheme,
        from_wallpaper: Option<Colour>,
    ) -> (Tokens, Vec<String>) {
        let mut notes = Vec::new();
        if let Some(name) = accent {
            let chosen = match name {
                WALLPAPER_ACCENT => from_wallpaper,
                _ => accent_named(name, scheme).or_else(|| opaque_colour(name)),
            };
            match chosen {
                Some(colour) => {
                    self.accent = colour;
                    self.accent_text = text_on(colour);
                }
                None if name != WALLPAPER_ACCENT => notes.push(trf(
                    "accent {name} is not an accent or a colour; the release's is used",
                    &[("name", &format!("{name:?}"))],
                )),
                None => {}
            }
        }
        // The focused title bar takes a breath of the accent, so the window
        // with the keyboard reads at a glance; a person's own
        // title_bar_focused, laid over below, replaces it whole.
        self.title_bar_focused = tinted(self.title_bar_focused, self.accent, self.title_tint);
        if let Some(colours) = colours {
            for (name, value) in &colours.both {
                self.set_from_text(name, value, &mut notes);
            }
            let own = match scheme {
                Scheme::Light => colours.light.as_ref(),
                Scheme::Dark => colours.dark.as_ref(),
            };
            for (name, value) in own.into_iter().flatten() {
                self.set_from_text(name, value, &mut notes);
            }
        }
        (self, notes)
    }

    /// Sets the colour token `name` to the colour `value` reads as, or notes
    /// why it cannot (M5.12b1).
    fn set_from_text(&mut self, name: &str, value: &str, notes: &mut Vec<String>) {
        let Some(slot) = self.slot(name) else {
            notes.push(trf(
                "colour {name} is not a colour token; it is ignored",
                &[("name", &format!("{name:?}"))],
            ));
            return;
        };
        match Colour::parse(value) {
            Some(colour) => *slot = colour,
            None => notes.push(trf(
                "colour {name} = {value} is not #rrggbb or #rrggbbaa; the release's is used",
                &[
                    ("name", &format!("{name:?}")),
                    ("value", &format!("{value:?}")),
                ],
            )),
        }
    }

    /// What the release's pairs fail to hold (M5.12b1), one line each, under
    /// the ratio a pair needs: `panel_text #777777 on panel #888888 reads at
    /// 1.2:1; text needs 4.5:1, such as #1a1a1a`, the suggestion being
    /// [`nearest_passing`] of the foreground. Empty when every pair holds.
    pub fn contrast_problems(&self) -> Vec<String> {
        PAIRS
            .iter()
            .filter_map(|&(text, surface, ratio)| {
                let foreground = self.colour(text)?;
                let background = self.colour(surface)?;
                let reads = contrast(foreground, background);
                (reads < ratio).then(|| {
                    let fix = nearest_passing(foreground, background, ratio);
                    let what = if ratio >= TEXT_CONTRAST {
                        tr("text")
                    } else {
                        tr("an icon or mark")
                    };
                    trf(
                        "{name} {fg} on {surface} {bg} reads at {reads}:1; {what} needs {need}:1, such as {fix}",
                        &[
                            ("name", text),
                            ("fg", &foreground.hex()),
                            ("surface", surface),
                            ("bg", &background.hex()),
                            ("reads", &format!("{:.1}", shown_down(reads))),
                            ("what", what),
                            ("need", &format!("{ratio:.1}")),
                            ("fix", &fix.hex()),
                        ],
                    )
                })
            })
            .collect()
    }
}

/// Each `[accent.NAME]` of the file has a `light` and a `dark` colour
/// (M5.12b1); what is wrong is noted, and [`check`] refuses it.
fn check_accents(accents: &Table, notes: &mut Vec<String>) {
    for (name, shades) in accents {
        let Some(shades) = shades.as_table() else {
            notes.push(trf(
                "accent.{name} is not a table of light and dark colours",
                &[("name", name)],
            ));
            continue;
        };
        for shade in ["light", "dark"] {
            let colour = shades
                .get(shade)
                .and_then(Value::as_str)
                .and_then(Colour::parse);
            if colour.is_none() {
                notes.push(trf(
                    "accent.{name}.{shade} needs a colour such as \"#2d6ae3\"",
                    &[("name", name), ("shade", shade)],
                ));
            }
        }
    }
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
                ("accent", Value::Table(accents)) => check_accents(accents, notes),
                (key, _) => notes.push(format!("unknown key {key} ignored")),
            }
        }
    }

    fn apply_colours(&mut self, colours: &Table, notes: &mut Vec<String>) {
        for (key, value) in colours {
            // The light scheme's colours, read by `apply_light`.
            if key == "light" && value.is_table() {
                continue;
            }
            let Some(slot) = self.slot(key) else {
                notes.push(format!("unknown key colour.{key} ignored"));
                continue;
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
                "gap_outer" => (&mut self.gap_outer, 100),
                "panel" => (&mut self.panel_height, 200),
                "panel_small" => (&mut self.panel_small_height, 200),
                "panel_large" => (&mut self.panel_large_height, 200),
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
                "lock_clock" => (&mut self.lock_clock, 400),
                "panel_opacity" => (&mut self.panel_opacity, 100),
                "window_opacity" => (&mut self.window_opacity, 100),
                "window_opacity_unfocused" => (&mut self.window_opacity_unfocused, 100),
                "blur" => (&mut self.blur, 100),
                "title_tint" => (&mut self.title_tint, 30),
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
            if key == "lock_clock_weight" {
                match value.as_integer().filter(|n| (100..=900).contains(n)) {
                    Some(n) => self.lock_clock_weight = n as u32,
                    None => notes.push(format!(
                        "font.{key} = {value} is not a weight from 100 to 900; the built-in one is used"
                    )),
                }
                continue;
            }
            let slot = match key.as_str() {
                "interface" => &mut self.font,
                "monospace" => &mut self.monospace,
                _ => {
                    notes.push(format!("unknown key font.{key} ignored"));
                    continue;
                }
            };
            match value.as_str().filter(|name| !name.trim().is_empty()) {
                Some(name) => *slot = name.trim().to_string(),
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
        ("colour", COLOUR_NAMES),
        (
            "size",
            &[
                "title_bar",
                "border",
                "title_text",
                "gap",
                "panel",
                "panel_small",
                "panel_large",
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
                "lock_clock",
                "gap_outer",
                "panel_opacity",
                "window_opacity",
                "window_opacity_unfocused",
                "blur",
                "title_tint",
            ][..],
        ),
        ("font", &["interface", "lock_clock_weight"][..]),
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
        gap_outer: 0,
        panel: BLACK,
        panel_text: BLACK,
        accent: BLACK,
        panel_height: 0,
        panel_small_height: 0,
        panel_large_height: 0,
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
        window_border: BLACK,
        window_border_focused: BLACK,
        window_border_focused_end: BLACK,
        backdrop: BLACK,
        backdrop_deep: BLACK,
        backdrop_text: BLACK,
        lock_glass: BLACK,
        lock_glass_edge: BLACK,
        lock_backdrop: BLACK,
        lock_backdrop_deep: BLACK,
        lock_glow: BLACK,
        lock_glow_second: BLACK,
        lock_edit_deep: BLACK,
        lock_edit_glow: BLACK,
        lock_clock: 0,
        panel_opacity: 0,
        window_opacity: 0,
        window_opacity_unfocused: 0,
        blur: 0,
        title_tint: 0,
        lock_clock_weight: 0,
        shadow_blur: 0,
        shadow_offset: 0,
        row: 0,
        text_size: 0,
        font: String::new(),
        monospace: MONOSPACE.to_string(),
        text_percent: 100,
    };
    tokens.apply(&table, &mut notes);
    let mut light = tokens.clone();
    light.apply_light(&table, &mut notes);
    if !notes.is_empty() {
        bail!("{}", notes.join("; "));
    }
    Ok(tokens)
}

/// A person's shape (M5.12b2): each value the person's file's, else the
/// machine's; none is the release's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Shape {
    pub radius: Option<u32>,
    pub gaps: Option<u32>,
    pub gaps_outer: Option<u32>,
    pub border_width: Option<u32>,
    pub title_bar_height: Option<u32>,
    pub panel_opacity: Option<u32>,
    /// The windows' depth (M5.12b3): the two opacities, the shadow's blur
    /// and its darkness, each as the person's file or the machine's sets it.
    pub window_opacity: Option<u32>,
    pub window_opacity_unfocused: Option<u32>,
    pub shadow_size: Option<u32>,
    pub shadow_darkness: Option<u32>,
    /// The glass behind see-through panels (M5.12b3): how strongly the
    /// wallpaper blurs there.
    pub blur: Option<u32>,
}

impl Shape {
    /// Each value the person's settings file sets, else the machine's
    /// (M5.12b2).
    pub fn between(machine: Option<&SettingsFile>, person: Option<&SettingsFile>) -> Shape {
        let over = |value: fn(&Appearance) -> Option<u32>| {
            person
                .and_then(|file| value(&file.appearance))
                .or_else(|| machine.and_then(|file| value(&file.appearance)))
        };
        Shape {
            radius: over(|a| a.radius),
            gaps: over(|a| a.gaps),
            gaps_outer: over(|a| a.gaps_outer),
            border_width: over(|a| a.border_width),
            title_bar_height: over(|a| a.title_bar_height),
            panel_opacity: over(|a| a.panel_opacity),
            window_opacity: over(|a| a.window_opacity),
            window_opacity_unfocused: over(|a| a.window_opacity_unfocused),
            shadow_size: over(|a| a.shadow_size),
            shadow_darkness: over(|a| a.shadow_darkness),
            blur: over(|a| a.blur),
        }
    }
}

/// `value` if it is in the range of the shape key `key` (M5.12b2), else a
/// note that the release's value is used, in the words of
/// `docs/MESSAGES.md`. A key with no range in the table takes any value.
fn in_shape_range(key: &str, value: Option<u32>, notes: &mut Vec<String>) -> Option<u32> {
    let value = value?;
    let (least, most) = settings::whole_range(key).unwrap_or((i64::MIN, i64::MAX));
    let shown = i64::from(value);
    if shown > most {
        notes.push(trf(
            "{key} {value} is more than {most}; the release's is used",
            &[
                ("key", key),
                ("value", &value.to_string()),
                ("most", &most.to_string()),
            ],
        ));
        None
    } else if shown < least {
        notes.push(trf(
            "{key} {value} is less than {least}; the release's is used",
            &[
                ("key", key),
                ("value", &value.to_string()),
                ("least", &least.to_string()),
            ],
        ));
        None
    } else {
        Some(value)
    }
}

impl Tokens {
    /// These tokens with a person's shape on top (M5.12b2): the radius
    /// sets `radius_window` and scales `radius_menu`, `radius_control` and
    /// `radius_small` by it (from the release's own `radius_window`; to
    /// the radius itself when that is 0); the gaps set `gap` and
    /// `gap_outer`, and `gaps_outer` alone sets `gap_outer`; the border,
    /// the title bar's height and the panels' opacity follow. The windows'
    /// depth (M5.12b3) follows too: the two opacities set their tokens; the
    /// shadow's size sets its blur and scales the release's drop by it (so
    /// 0 is no shadow), and its darkness sets the shadow colour's alpha in
    /// both schemes, its red, green and blue kept. The glass behind
    /// see-through panels (M5.12b3) sets `blur`.
    /// Lenient (ADR-008): a value outside its key's range is noted and
    /// skipped.
    pub fn with_shape(mut self, shape: &Shape) -> (Tokens, Vec<String>) {
        let mut notes = Vec::new();
        if let Some(radius) = in_shape_range("appearance.radius", shape.radius, &mut notes) {
            let base = self.radius_window;
            // Rounded half up, so 12 at 20 over 10 is 24.
            let scaled = |value: u32| {
                if base == 0 {
                    radius
                } else {
                    ((u64::from(value) * u64::from(radius) + u64::from(base) / 2) / u64::from(base))
                        as u32
                }
            };
            self.radius_menu = scaled(self.radius_menu);
            self.radius_control = scaled(self.radius_control);
            self.radius_small = scaled(self.radius_small);
            self.radius_window = radius;
        }
        if let Some(gap) = in_shape_range("appearance.gaps", shape.gaps, &mut notes) {
            self.gap = gap;
            self.gap_outer = gap;
        }
        if let Some(gap_outer) =
            in_shape_range("appearance.gaps_outer", shape.gaps_outer, &mut notes)
        {
            self.gap_outer = gap_outer;
        }
        if let Some(border) =
            in_shape_range("appearance.border_width", shape.border_width, &mut notes)
        {
            self.border = border;
        }
        if let Some(height) = in_shape_range(
            "appearance.title_bar_height",
            shape.title_bar_height,
            &mut notes,
        ) {
            self.title_bar_height = height;
        }
        if let Some(percent) =
            in_shape_range("appearance.panel_opacity", shape.panel_opacity, &mut notes)
        {
            self.panel_opacity = percent;
        }
        if let Some(percent) = in_shape_range(
            "appearance.window_opacity",
            shape.window_opacity,
            &mut notes,
        ) {
            self.window_opacity = percent;
        }
        if let Some(percent) = in_shape_range(
            "appearance.window_opacity_unfocused",
            shape.window_opacity_unfocused,
            &mut notes,
        ) {
            self.window_opacity_unfocused = percent;
        }
        if let Some(blur) = in_shape_range("appearance.shadow_size", shape.shadow_size, &mut notes)
        {
            // The release's drop grows with the blur, rounded half up, so
            // 48 doubles 24 and 6 to 12; with no release blur there is no
            // drop to scale, and 0 is no shadow at all.
            let (release_blur, release_drop) =
                (u64::from(self.shadow_blur), u64::from(self.shadow_offset));
            self.shadow_offset = if release_blur == 0 {
                0
            } else {
                ((2 * release_drop * u64::from(blur) + release_blur) / (2 * release_blur)) as u32
            };
            self.shadow_blur = blur;
        }
        if let Some(percent) = in_shape_range(
            "appearance.shadow_darkness",
            shape.shadow_darkness,
            &mut notes,
        ) {
            // The alpha in 255ths, rounded half up: 50 percent is 128.
            let alpha = (percent * 255 + 50) / 100;
            self.shadow.a = alpha as f32 / 255.0;
        }
        if let Some(percent) = in_shape_range("appearance.blur", shape.blur, &mut notes) {
            self.blur = percent;
        }
        (self, notes)
    }
}

/// A person's text (M5.12b5): the interface and monospace fonts and the
/// text size in percent; each value the person's file's, else the machine's.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Text {
    pub font: Option<String>,
    pub monospace_font: Option<String>,
    pub text_size: Option<u32>,
}

impl Text {
    /// Each value the person's settings file sets, else the machine's
    /// (M5.12b5).
    pub fn between(machine: Option<&SettingsFile>, person: Option<&SettingsFile>) -> Text {
        Text {
            font: over(machine, person, |a| a.font.clone()),
            monospace_font: over(machine, person, |a| a.monospace_font.clone()),
            text_size: over(machine, person, |a| a.text_size),
        }
    }
}

/// The value of one appearance key: the person's file's, else the machine's.
fn over<T>(
    machine: Option<&SettingsFile>,
    person: Option<&SettingsFile>,
    value: fn(&Appearance) -> Option<T>,
) -> Option<T> {
    person
        .and_then(|file| value(&file.appearance))
        .or_else(|| machine.and_then(|file| value(&file.appearance)))
}

/// `family` trimmed if it names a font, else a note that the release's is
/// used, in the words of `docs/MESSAGES.md` (M5.12b5).
fn family_in(key: &str, family: Option<&str>, notes: &mut Vec<String>) -> Option<String> {
    let family = family?.trim();
    if family.is_empty() {
        notes.push(trf(
            "{key} is empty; the release's is used",
            &[("key", key)],
        ));
        None
    } else {
        Some(family.to_string())
    }
}

/// A text size of `value` scaled to `percent` of the release's, rounded
/// half up and never below 8 px (M5.12b5).
fn scaled(value: u32, percent: u32) -> u32 {
    ((u64::from(value) * u64::from(percent) + 50) / 100).max(8) as u32
}

impl Tokens {
    /// Every size of letters (M5.12b5), in one list: a later text size token
    /// is one line here, and [`Tokens::with_text`] scales them all.
    fn text_sizes_mut(&mut self) -> [&mut u32; 5] {
        [
            &mut self.title_text_size,
            &mut self.panel_text_size,
            &mut self.panel_text_small_size,
            &mut self.text_size,
            &mut self.lock_clock,
        ]
    }

    /// These tokens with a person's text on top (M5.12b5): the interface
    /// and monospace fonts replace their families, and `text_size` scales
    /// every size of letters by its percent, rounded half up, never below 8
    /// px; the boxes stay as they are, so the bars keep their room. Lenient
    /// (ADR-008): an empty family or a size out of range is noted and
    /// skipped.
    pub fn with_text(mut self, text: &Text) -> (Tokens, Vec<String>) {
        let mut notes = Vec::new();
        if let Some(family) = family_in("appearance.font", text.font.as_deref(), &mut notes) {
            self.font = family;
        }
        if let Some(family) = family_in(
            "appearance.monospace_font",
            text.monospace_font.as_deref(),
            &mut notes,
        ) {
            self.monospace = family;
        }
        if let Some(percent) = in_shape_range("appearance.text_size", text.text_size, &mut notes) {
            for size in self.text_sizes_mut() {
                *size = scaled(*size, percent);
            }
            self.text_percent = percent;
        }
        (self, notes)
    }
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
    fn the_panel_sizes_come_from_the_tokens_small_medium_and_large() {
        let tokens = Tokens::built_in();
        assert_eq!(
            (
                tokens.panel_small_height,
                tokens.panel_height,
                tokens.panel_large_height
            ),
            (34, 40, 48)
        );
        let (read, notes) = Tokens::read(&BUILT_IN.replace("panel_large = 48", "panel_large = 52"));
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(read.panel_large_height, 52);
        let (_, notes) = Tokens::read(&BUILT_IN.replace("panel_small = 34", "panel_small = -1"));
        assert!(notes[0].contains("size.panel_small"), "{notes:?}");
        let without = BUILT_IN.replace("panel_small = 34", "# panel_small = 34");
        let err = check(&without).unwrap_err();
        assert!(
            err.to_string().contains("size.panel_small is missing"),
            "{err}"
        );
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
        assert_eq!(tokens.font, "Inter Variable");
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

    #[test]
    fn the_built_in_tokens_hold_every_pair_in_both_schemes() {
        assert_eq!(Tokens::built_in().contrast_problems(), Vec::<String>::new());
        let light = Tokens::built_in_scheme(Scheme::Light);
        assert_eq!(light.contrast_problems(), Vec::<String>::new());
    }

    #[test]
    fn every_named_accent_with_its_text_holds_every_pair_in_both_schemes() {
        for (name, _, _) in accents() {
            for scheme in [Scheme::Light, Scheme::Dark] {
                let built = Tokens::built_in_scheme(scheme);
                let (tokens, notes) = built.with_appearance(Some(&name), None, scheme, None);
                assert!(notes.is_empty(), "{name}: {notes:?}");
                assert_eq!(
                    tokens.contrast_problems(),
                    Vec::<String>::new(),
                    "{name} in {scheme:?}"
                );
            }
        }
    }

    #[test]
    fn a_pale_accent_fails_its_text_and_the_suggestion_passes() {
        let mut tokens = Tokens::built_in_scheme(Scheme::Light);
        tokens.accent = Colour::parse("#c0d0ff").unwrap();
        let problems = tokens.contrast_problems();
        let line = problems
            .iter()
            .find(|p| p.starts_with("accent_text "))
            .expect("the pale accent fails its text");
        let fix = nearest_passing(tokens.accent_text, tokens.accent, TEXT_CONTRAST);
        assert!(
            line.ends_with(&format!("text needs 4.5:1, such as {}", fix.hex())),
            "{line}"
        );
        assert!(contrast(fix, tokens.accent) >= TEXT_CONTRAST);
    }

    #[test]
    fn a_problem_names_both_colours_the_ratio_and_a_colour_that_passes() {
        let mut tokens = Tokens::built_in_scheme(Scheme::Light);
        tokens.panel_text = Colour::parse("#777777").unwrap();
        tokens.panel = Colour::parse("#888888").unwrap();
        let problems = tokens.contrast_problems();
        let line = problems
            .iter()
            .find(|p| p.starts_with("panel_text "))
            .expect("the grey text fails");
        assert!(
            line.starts_with(
                "panel_text #777777 on panel #888888 reads at 1.2:1; text needs 4.5:1, such as #"
            ),
            "{line}"
        );
    }

    #[test]
    fn contrast_is_21_for_black_and_white_and_1_for_a_colour_with_itself() {
        let black = Colour::parse("#000000").unwrap();
        let white = Colour::parse("#ffffff").unwrap();
        let panel = Colour::parse("#1a1c22").unwrap();
        assert!((contrast(black, white) - 21.0).abs() < 1e-3);
        assert!((contrast(white, black) - 21.0).abs() < 1e-3);
        assert!((contrast(panel, panel) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn nearest_passing_keeps_a_colour_that_passes_and_darkens_one_that_does_not() {
        let black = Colour::parse("#000000").unwrap();
        let grey = Colour::parse("#888888").unwrap();
        let text = Colour::parse("#777777").unwrap();
        assert_eq!(nearest_passing(text, black, TEXT_CONTRAST), text);
        let fixed = nearest_passing(text, grey, TEXT_CONTRAST);
        assert!(contrast(fixed, grey) >= TEXT_CONTRAST, "{}", fixed.hex());
        assert!(luminance(fixed) < luminance(text), "{}", fixed.hex());
    }

    #[test]
    fn a_colour_token_is_read_and_named_by_one_list() {
        let tokens = Tokens::built_in();
        assert_eq!(COLOUR_NAMES.len(), 30);
        for name in COLOUR_NAMES {
            assert!(tokens.colour(name).is_some(), "{name}");
        }
        assert_eq!(tokens.colour("panel"), Some(tokens.panel));
        assert_eq!(tokens.colour("panl"), None);
    }

    #[test]
    fn the_accents_are_in_the_files_order_and_blue_is_the_release_accent() {
        let names: Vec<String> = accents().into_iter().map(|(name, _, _)| name).collect();
        assert_eq!(
            names,
            [
                "blue", "purple", "pink", "red", "orange", "yellow", "green", "teal", "graphite"
            ]
        );
        let (_, light, dark) = accents().remove(0);
        assert_eq!(light, Tokens::built_in_scheme(Scheme::Light).accent);
        assert_eq!(dark, Tokens::built_in().accent);
    }

    #[test]
    fn a_named_accent_is_its_shade_in_each_scheme() {
        assert_eq!(accent_named("red", Scheme::Light).unwrap().hex(), "#d0363f");
        assert_eq!(accent_named("red", Scheme::Dark).unwrap().hex(), "#f2707a");
        assert_eq!(accent_named("crimson", Scheme::Light), None);
        assert!(is_accent("red") && is_accent("wallpaper") && is_accent("#ff0000"));
        assert!(!is_accent("#ff000080") && !is_accent("crimson") && !is_accent("#ff00"));
    }

    #[test]
    fn a_named_accent_takes_its_text_and_a_colour_takes_its_own() {
        let light = Tokens::built_in_scheme(Scheme::Light);
        let (red, notes) = light
            .clone()
            .with_appearance(Some("red"), None, Scheme::Light, None);
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(red.accent.hex(), "#d0363f");
        assert_eq!(red.accent_text.hex(), "#ffffff");
        let (hex, _) = light.with_appearance(Some("#ff0000"), None, Scheme::Light, None);
        assert_eq!(hex.accent.hex(), "#ff0000");
        let (dark, _) = Tokens::built_in().with_appearance(Some("red"), None, Scheme::Dark, None);
        assert_eq!(dark.accent.hex(), "#f2707a");
        assert_eq!(dark.accent_text.hex(), "#0b1220");
    }

    #[test]
    fn the_wallpaper_accent_takes_the_pictures_colour_or_keeps_the_release_s() {
        let built = Tokens::built_in_scheme(Scheme::Light);
        let picture = Colour::parse("#336699").unwrap();
        let (taken, _) =
            built
                .clone()
                .with_appearance(Some("wallpaper"), None, Scheme::Light, Some(picture));
        assert_eq!(taken.accent, picture);
        let (kept, notes) =
            built
                .clone()
                .with_appearance(Some("wallpaper"), None, Scheme::Light, None);
        assert_eq!(kept.accent, built.accent);
        assert!(notes.is_empty(), "{notes:?}");
        let (_, notes) = built.with_appearance(Some("crimson"), None, Scheme::Light, None);
        assert_eq!(notes.len(), 1, "{notes:?}");
    }

    #[test]
    fn colours_change_a_token_in_both_schemes_or_only_its_own() {
        let shared = Colours {
            both: std::collections::BTreeMap::from([("panel".into(), "#102030".into())]),
            light: None,
            dark: None,
        };
        let (light, notes) = Tokens::built_in_scheme(Scheme::Light).with_appearance(
            None,
            Some(&shared),
            Scheme::Light,
            None,
        );
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(light.panel.hex(), "#102030");
        let (dark, _) = Tokens::built_in().with_appearance(None, Some(&shared), Scheme::Dark, None);
        assert_eq!(dark.panel.hex(), "#102030");
        let own = Colours {
            both: std::collections::BTreeMap::from([("panel".into(), "#102030".into())]),
            light: None,
            dark: Some(std::collections::BTreeMap::from([(
                "panel".into(),
                "#000000".into(),
            )])),
        };
        let (dark, _) = Tokens::built_in().with_appearance(None, Some(&own), Scheme::Dark, None);
        assert_eq!(dark.panel.hex(), "#000000");
        let (light, _) = Tokens::built_in_scheme(Scheme::Light).with_appearance(
            None,
            Some(&own),
            Scheme::Light,
            None,
        );
        assert_eq!(light.panel.hex(), "#102030");
    }

    #[test]
    fn an_unknown_colour_name_or_value_is_noted_and_skipped() {
        let bad = Colours {
            both: std::collections::BTreeMap::from([
                ("panl".into(), "#ffffff".into()),
                ("panel".into(), "red".into()),
            ]),
            light: None,
            dark: None,
        };
        let (tokens, notes) =
            Tokens::built_in().with_appearance(None, Some(&bad), Scheme::Dark, None);
        assert_eq!(tokens.panel, Tokens::built_in().panel);
        assert_eq!(notes.len(), 2, "{notes:?}");
        assert!(notes.iter().any(|n| n.contains("\"panl\"")), "{notes:?}");
    }

    #[test]
    fn the_accent_table_wants_both_shades_as_colours() {
        let without = BUILT_IN.replace(
            "light = \"#2d6ae3\"\ndark = \"#5b8ef5\"",
            "dark = \"#5b8ef5\"",
        );
        let err = format!("{:#}", check(&without).unwrap_err());
        assert!(err.contains("accent.blue.light needs a colour"), "{err}");
        let (_, notes) = Tokens::read(&BUILT_IN.replace("dark = \"#f2c94c\"", "dark = \"yellow\""));
        assert!(
            notes.iter().any(|n| n.contains("accent.yellow.dark")),
            "{notes:?}"
        );
    }

    #[test]
    fn load_with_gives_the_appearance_over_the_release() {
        let (tokens, notes) = load_with(Scheme::Light, Some("red"), None, None);
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(tokens.accent.hex(), "#d0363f");
    }

    #[test]
    fn the_persons_accent_wins_over_the_machines() {
        let machine = Some("format = 1\n[appearance]\naccent = \"red\"\n");
        let person = Some("format = 1\n[appearance]\naccent = \"green\"\n");
        let (both, _) = load_from_files(Scheme::Light, machine, person, None);
        assert_eq!(both.accent, accent_named("green", Scheme::Light).unwrap());
        let (only, _) = load_from_files(Scheme::Light, machine, None, None);
        assert_eq!(only.accent, accent_named("red", Scheme::Light).unwrap());
    }

    #[test]
    fn colours_are_merged_name_by_name_across_the_two_files() {
        let machine = Some(
            "format = 1\n[appearance.colours]\npanel = \"#112233\"\n[appearance.colours.dark]\npanel = \"#000001\"\n",
        );
        let person = Some("format = 1\n[appearance.colours]\nwindow = \"#445566\"\n");
        let (tokens, notes) = load_from_files(Scheme::Light, machine, person, None);
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(tokens.panel.hex(), "#112233");
        assert_eq!(tokens.window.hex(), "#445566");
    }

    #[test]
    fn a_broken_persons_file_reads_as_absent() {
        let machine = Some("format = 1\n[appearance]\naccent = \"red\"\n");
        let (tokens, notes) = load_from_files(Scheme::Light, machine, Some("not = [toml"), None);
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(tokens.accent, accent_named("red", Scheme::Light).unwrap());
    }

    #[test]
    fn a_radius_of_0_makes_every_corner_square_and_20_scales_them() {
        let zero = Shape {
            radius: Some(0),
            ..Shape::default()
        };
        let (square, notes) = Tokens::built_in().with_shape(&zero);
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(
            (
                square.radius_window,
                square.radius_menu,
                square.radius_control,
                square.radius_small
            ),
            (0, 0, 0, 0)
        );
        let twenty = Shape {
            radius: Some(20),
            ..Shape::default()
        };
        let (round, _) = Tokens::built_in().with_shape(&twenty);
        assert_eq!(
            (
                round.radius_window,
                round.radius_menu,
                round.radius_control,
                round.radius_small
            ),
            (20, 24, 14, 10)
        );
    }

    #[test]
    fn gaps_set_the_gap_and_its_outer_edge_unless_gaps_outer_is_set() {
        let alone = Shape {
            gaps: Some(12),
            ..Shape::default()
        };
        let (both, _) = Tokens::built_in().with_shape(&alone);
        assert_eq!((both.gap, both.gap_outer), (12, 12));
        let apart = Shape {
            gaps: Some(12),
            gaps_outer: Some(4),
            ..Shape::default()
        };
        let (apart, _) = Tokens::built_in().with_shape(&apart);
        assert_eq!((apart.gap, apart.gap_outer), (12, 4));
    }

    #[test]
    fn panel_opacity_is_its_own_token_and_leaves_the_panel_colour() {
        let half = Shape {
            panel_opacity: Some(50),
            ..Shape::default()
        };
        let release = Tokens::built_in();
        assert_eq!(release.panel_opacity, 100);
        let (tokens, notes) = Tokens::built_in().with_shape(&half);
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(tokens.panel_opacity, 50);
        assert_eq!(tokens.panel, release.panel);
    }

    #[test]
    fn window_opacity_sets_its_two_tokens_and_leaves_the_rest() {
        let release = Tokens::built_in();
        assert_eq!(
            (release.window_opacity, release.window_opacity_unfocused),
            (100, 100)
        );
        let shape = Shape {
            window_opacity: Some(90),
            window_opacity_unfocused: Some(70),
            ..Shape::default()
        };
        let (tokens, notes) = Tokens::built_in().with_shape(&shape);
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(
            (tokens.window_opacity, tokens.window_opacity_unfocused),
            (90, 70)
        );
        assert_eq!((tokens.shadow_blur, tokens.shadow_offset), (24, 6));
        assert_eq!(tokens.window_border, release.window_border);
    }

    #[test]
    fn shadow_size_scales_the_blur_and_the_drop_and_zero_is_none() {
        let release = Tokens::built_in();
        assert_eq!((release.shadow_blur, release.shadow_offset), (24, 6));
        let big = Shape {
            shadow_size: Some(48),
            ..Shape::default()
        };
        let (tokens, notes) = Tokens::built_in().with_shape(&big);
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!((tokens.shadow_blur, tokens.shadow_offset), (48, 12));
        let none = Shape {
            shadow_size: Some(0),
            ..Shape::default()
        };
        let (tokens, _) = Tokens::built_in().with_shape(&none);
        assert_eq!((tokens.shadow_blur, tokens.shadow_offset), (0, 0));
    }

    #[test]
    fn shadow_darkness_sets_the_alpha_and_keeps_the_colour() {
        let release = Tokens::built_in();
        let half = Shape {
            shadow_darkness: Some(50),
            ..Shape::default()
        };
        let (tokens, notes) = Tokens::built_in().with_shape(&half);
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!((tokens.shadow.a * 255.0).round() as u32, 128);
        assert_eq!(
            (tokens.shadow.r, tokens.shadow.g, tokens.shadow.b),
            (release.shadow.r, release.shadow.g, release.shadow.b)
        );
    }

    #[test]
    fn the_glass_blur_sets_its_token_and_101_is_refused() {
        let release = Tokens::built_in();
        assert_eq!(release.blur, 50);
        let sharp = Shape {
            blur: Some(0),
            ..Shape::default()
        };
        let (tokens, notes) = Tokens::built_in().with_shape(&sharp);
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(tokens.blur, 0);
        let (tokens, notes) = Tokens::built_in().with_shape(&Shape {
            blur: Some(100),
            ..Shape::default()
        });
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(tokens.blur, 100);
        let (tokens, notes) = Tokens::built_in().with_shape(&Shape {
            blur: Some(101),
            ..Shape::default()
        });
        assert_eq!(tokens.blur, 50);
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(notes[0].contains("appearance.blur"), "{notes:?}");
    }

    #[test]
    fn an_out_of_range_depth_value_is_noted_and_the_release_s_stays() {
        let faint = Shape {
            window_opacity: Some(10),
            ..Shape::default()
        };
        let (tokens, notes) = Tokens::built_in().with_shape(&faint);
        assert_eq!(tokens.window_opacity, 100);
        assert_eq!(
            notes,
            ["appearance.window_opacity 10 is less than 20; the release's is used"]
        );
    }

    #[test]
    fn an_out_of_range_shape_value_is_noted_and_the_release_s_stays() {
        let wide = Shape {
            radius: Some(40),
            ..Shape::default()
        };
        let (tokens, notes) = Tokens::built_in().with_shape(&wide);
        assert_eq!(tokens.radius_window, 10);
        assert_eq!(
            notes,
            ["appearance.radius 40 is more than 24; the release's is used"]
        );
        let thin = Shape {
            title_bar_height: Some(10),
            ..Shape::default()
        };
        let (tokens, notes) = Tokens::built_in().with_shape(&thin);
        assert_eq!(tokens.title_bar_height, 28);
        assert_eq!(
            notes,
            ["appearance.title_bar_height 10 is less than 24; the release's is used"]
        );
    }

    #[test]
    fn the_persons_shape_wins_over_the_machines_and_the_machines_is_used_alone() {
        let read = |text: &str| settings::read(text).unwrap().file;
        let machine = read("format = 1\n[appearance]\nradius = 6\ngaps = 4\n");
        let person = read("format = 1\n[appearance]\ngaps = 12\n");
        assert_eq!(
            Shape::between(Some(&machine), Some(&person)),
            Shape {
                radius: Some(6),
                gaps: Some(12),
                ..Shape::default()
            }
        );
        assert_eq!(Shape::between(Some(&machine), None).gaps, Some(4));
        assert_eq!(Shape::between(None, None), Shape::default());
    }

    #[test]
    fn load_from_files_gives_the_gap_the_border_and_the_red_focused_border() {
        let person = "format = 1\n[appearance]\ngaps = 12\nborder_width = 3\n[appearance.colours]\nwindow_border_focused = \"#ff0000\"\n";
        let (tokens, notes) = load_from_files(Scheme::Light, None, Some(person), None);
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!((tokens.gap, tokens.border), (12, 3));
        assert_eq!(tokens.window_border_focused.hex(), "#ff0000");
    }

    #[test]
    fn an_out_of_range_value_in_a_file_is_a_note_and_the_release_s_stays() {
        let file = "format = 1\n[appearance]\nradius = 40\n";
        let (tokens, notes) = load_from_files(Scheme::Light, None, Some(file), None);
        assert_eq!(tokens.radius_window, 10);
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(notes[0].starts_with("appearance.radius: "), "{notes:?}");
    }

    #[test]
    fn the_window_borders_have_edge_s_value_in_each_scheme_in_the_file() {
        let table: Table = BUILT_IN.parse().expect("checked by the tests");
        let dark = table["colour"].as_table().expect("[colour]");
        let light = dark["light"].as_table().expect("[colour.light]");
        for name in [
            "window_border",
            "window_border_focused",
            "window_border_focused_end",
        ] {
            assert_eq!(dark[name], dark["edge"], "{name} in [colour]");
            assert_eq!(light[name], light["edge"], "{name} in [colour.light]");
        }
        assert_eq!(
            Tokens::built_in().window_border,
            Tokens::built_in().edge,
            "the built-in dark border"
        );
    }

    #[test]
    fn the_focused_title_bar_takes_a_breath_of_the_accent_unless_set() {
        let release = Tokens::built_in_scheme(Scheme::Light);
        let (tokens, _) = release
            .clone()
            .with_appearance(Some("red"), None, Scheme::Light, None);
        let red = accent_named("red", Scheme::Light).unwrap();
        assert_eq!(
            tokens.title_bar_focused,
            tinted(release.title_bar_focused, red, 6)
        );
        assert_ne!(tokens.title_bar_focused, release.title_bar_focused);
        assert_eq!(tokens.title_bar, release.title_bar);
        let own = Colours {
            both: std::collections::BTreeMap::from([(
                "title_bar_focused".into(),
                "#123456".into(),
            )]),
            light: None,
            dark: None,
        };
        let (mine, _) = release.with_appearance(Some("red"), Some(&own), Scheme::Light, None);
        assert_eq!(mine.title_bar_focused.hex(), "#123456");
    }

    #[test]
    fn absent_text_changes_nothing_and_the_monospace_font_is_the_default() {
        let built = Tokens::built_in();
        assert_eq!(built.monospace, MONOSPACE);
        assert_eq!(built.text_percent, 100);
        let (same, notes) = built.clone().with_text(&Text::default());
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(same, built);
    }

    #[test]
    fn a_text_size_of_150_and_80_scales_each_size_of_letters_only() {
        let text = |percent: u32| Text {
            text_size: Some(percent),
            ..Text::default()
        };
        let built = Tokens::built_in();
        let (big, notes) = built.clone().with_text(&text(150));
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(
            (
                big.title_text_size,
                big.panel_text_size,
                big.panel_text_small_size,
                big.text_size,
                big.lock_clock
            ),
            (20, 20, 17, 20, 225)
        );
        assert_eq!(big.text_percent, 150);
        // The boxes keep their sizes, so the bars keep their room.
        assert_eq!(
            (big.title_bar_height, big.panel_height, big.row),
            (built.title_bar_height, built.panel_height, built.row)
        );
        let (small, _) = built.clone().with_text(&text(80));
        assert_eq!(
            (
                small.title_text_size,
                small.panel_text_size,
                small.panel_text_small_size,
                small.text_size,
                small.lock_clock
            ),
            (10, 10, 9, 10, 120)
        );
    }

    #[test]
    fn a_title_at_150_percent_still_fits_its_bar_less_six_pixels() {
        let text = Text {
            text_size: Some(150),
            ..Text::default()
        };
        let (big, _) = Tokens::built_in().with_text(&text);
        assert!(big.title_text_size + 6 <= big.title_bar_height);
    }

    #[test]
    fn a_family_replaces_the_font_and_the_monospace_one() {
        let text = Text {
            font: Some(" Noto Sans ".into()),
            monospace_font: Some("Fira Code".into()),
            text_size: None,
        };
        let (tokens, notes) = Tokens::built_in().with_text(&text);
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(tokens.font, "Noto Sans");
        assert_eq!(tokens.monospace, "Fira Code");
    }

    #[test]
    fn the_persons_text_wins_over_the_machines_each_key_on_its_own() {
        let read = |text: &str| settings::read(text).unwrap().file;
        let machine = read("format = 1\n[appearance]\ntext_size = 80\nfont = \"Noto Sans\"\n");
        let person = read("format = 1\n[appearance]\ntext_size = 150\n");
        let text = Text::between(Some(&machine), Some(&person));
        assert_eq!(
            text,
            Text {
                font: Some("Noto Sans".into()),
                monospace_font: None,
                text_size: Some(150),
            }
        );
        let (tokens, notes) = load_from_files(Scheme::Light, None, None, None);
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(tokens.text_size, Tokens::built_in().text_size);
        let (tokens, _) = load_from_files(
            Scheme::Light,
            Some("format = 1\n[appearance]\ntext_size = 80\n"),
            Some("format = 1\n[appearance]\ntext_size = 150\n"),
            None,
        );
        assert_eq!((tokens.text_size, tokens.text_percent), (20, 150));
    }

    #[test]
    fn an_out_of_range_size_or_an_empty_family_is_a_note_and_changes_nothing() {
        let built = Tokens::built_in();
        let wide = Text {
            text_size: Some(200),
            ..Text::default()
        };
        let (tokens, notes) = built.clone().with_text(&wide);
        assert_eq!(tokens, built);
        assert_eq!(
            notes,
            ["appearance.text_size 200 is more than 150; the release's is used"]
        );
        let empty = Text {
            font: Some("  ".into()),
            ..Text::default()
        };
        let (tokens, notes) = built.clone().with_text(&empty);
        assert_eq!(tokens.font, built.font);
        assert_eq!(notes, ["appearance.font is empty; the release's is used"]);
    }
}
