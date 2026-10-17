//! The Appearance page's accent and colours (M5.12b1c): the accent as
//! swatches (the nine named accents, the wallpaper's colour and a colour of
//! the person's own), then the colour tokens a person or this machine sets,
//! each a row with Reset and Copy as command, and a row that adds one.
//! Every choice is one line of the person's settings file: the accent with
//! `Files::choose_over`, so blue, the release's, is taken out and never
//! written, and each colour with `Files::set`, so a pair the contrast check
//! refuses reads as `edel settings set` says it, under the row that tried it
//! and in the page's problem line (ADR-008). The group follows the files as
//! `wallpaper.rs`'s card does. The swatches' colours come from
//! `tokens::accents()`, the colours shown from the tokens, and every colour
//! choice goes through `widgets::choose_colour`.

use std::cell::{Cell, RefCell};
use std::f64::consts::TAU;
use std::path::Path;
use std::rc::{Rc, Weak};

use gtk::glib;
use gtk::prelude::*;

use edel::i18n::{n_, tr};
use edel::settings::{self, Colours};
use edel::tokens::{self, Colour, Scheme, Tokens};

use crate::files::Files;
use crate::style::Theme;
use crate::{rows, widgets};

/// The key the accent swatches choose.
const KEY: &str = "appearance.accent";
/// The accent a person starts from, the release's: the first of the tokens'
/// accents (`design/tokens.toml` owns it), taken out, never written.
fn release() -> String {
    tokens::accents()
        .first()
        .map(|(name, _, _)| name.clone())
        .unwrap_or_default()
}
/// The table of colours a person sets, `appearance.colours`, and its schemes.
const COLOURS: &str = "appearance.colours";
/// The high-contrast switch's key (M6.9): `high`, or taken out for normal.
const CONTRAST: &str = "appearance.contrast";
/// The line under the High contrast switch, before where its value comes from.
const CONTRAST_LINE: &str =
    n_("Strong edges and the plainest colours, for text that is easy to read");
/// The swatch names that are not accents: the wallpaper's colour, a pill
/// now (M5.12b6c), and a colour chosen in the system's dialog.
const WALLPAPER_SWATCH: &str = tokens::WALLPAPER_ACCENT;
const CUSTOM_SWATCH: &str = "custom";
/// A swatch's button in logical pixels, 28 px, and the circle drawn in it,
/// 20 px across: the chosen one's ring lies at the button's edge.
const SWATCH: i32 = 28;
const CIRCLE: f64 = 20.0;
/// The width of the chosen swatch's ring.
const RING_WIDTH: f64 = 2.0;
/// The arms of the plus in a swatch with no colour, as a part of its radius.
const PLUS: f64 = 0.5;
/// The intro line of the colours, and the line when there are none yet.
const COLOURS_INTRO: &str =
    "Your own colour for any part; a colour that would make text hard to read is refused";
const NO_COLOURS: &str = "No colour of your own yet";

/// Which settings file table a colour is written in: both schemes, or one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Scope {
    /// `appearance.colours.NAME`, for both schemes.
    Both,
    /// `appearance.colours.light.NAME`.
    Light,
    /// `appearance.colours.dark.NAME`.
    Dark,
}

/// A colour a person or this machine sets, as the list shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Listed {
    /// The colour token's name, as the settings file writes it.
    pub name: String,
    /// The table it is in.
    pub scope: Scope,
    /// The colour as the file writes it, `#rrggbb`.
    pub value: String,
}

impl Listed {
    /// The settings file key that holds this colour.
    fn key(&self) -> String {
        key_for(&self.name, self.scope)
    }
}

/// The settings file key of colour token `name` in `scope`:
/// `appearance.colours.panel`, `appearance.colours.light.panel` or
/// `appearance.colours.dark.panel`.
pub fn key_for(name: &str, scope: Scope) -> String {
    match scope {
        Scope::Both => format!("{COLOURS}.{name}"),
        Scope::Light => format!("{COLOURS}.light.{name}"),
        Scope::Dark => format!("{COLOURS}.dark.{name}"),
    }
}

/// The scope the drop-down of the add row chose, by its place: both, light
/// or dark.
fn scope_at(at: u32) -> Scope {
    match at {
        1 => Scope::Light,
        2 => Scope::Dark,
        _ => Scope::Both,
    }
}

/// The colours a colour value lists, both schemes' own after the shared
/// ones, each table in name order (`BTreeMap`), so the list is stable.
pub fn listed(colours: Option<&Colours>) -> Vec<Listed> {
    let Some(colours) = colours else {
        return Vec::new();
    };
    let tables = [
        (Scope::Both, Some(&colours.both)),
        (Scope::Light, colours.light.as_ref()),
        (Scope::Dark, colours.dark.as_ref()),
    ];
    tables
        .into_iter()
        .flat_map(|(scope, table)| {
            table
                .into_iter()
                .flatten()
                .map(move |(name, value)| Listed {
                    name: name.clone(),
                    scope,
                    value: value.clone(),
                })
        })
        .collect()
}

/// A colour token's name as people read it: `panel_text` is Panel text.
pub fn readable(token: &str) -> String {
    let spaced = token.replace('_', " ");
    let mut chars = spaced.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// An accent's name as people read it, translated; the wallpaper's and a
/// custom colour's too.
pub fn accent_title(name: &str) -> String {
    match name {
        WALLPAPER_SWATCH => tr("From wallpaper").to_string(),
        CUSTOM_SWATCH => tr("Custom").to_string(),
        other => tokens::accent_title(other).map_or_else(|| other.to_string(), str::to_string),
    }
}

/// The swatches in the order people see them: the accents in the tokens'
/// file order, then the wallpaper's and the custom one.
pub fn swatch_names() -> Vec<String> {
    let mut names: Vec<String> = tokens::accents()
        .into_iter()
        .map(|(name, _, _)| name)
        .collect();
    names.push(WALLPAPER_SWATCH.to_string());
    names.push(CUSTOM_SWATCH.to_string());
    names
}

/// The swatch that is chosen for the accent `value` a file gives (none is
/// the release's blue): an accent's name, `wallpaper`, or `custom` for a
/// colour.
pub fn swatch_of(value: Option<&str>) -> String {
    match value {
        None => release(),
        Some(value) if value == WALLPAPER_SWATCH => WALLPAPER_SWATCH.to_string(),
        Some(value) if tokens::accents().iter().any(|(name, _, _)| name == value) => {
            value.to_string()
        }
        Some(value) if tokens::is_accent(value) => CUSTOM_SWATCH.to_string(),
        Some(_) => release(),
    }
}

/// What a swatch's circle is filled with.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Fill {
    /// One colour.
    One(Colour),
    /// No colour: a ring with a plus in the text's colour.
    Empty,
}

/// What the accent swatch `name` is filled with in `scheme`: its shade, or
/// the empty ring when the tokens have no such accent.
fn fill_of(name: &str, scheme: Scheme) -> Fill {
    tokens::accent_named(name, scheme).map_or(Fill::Empty, Fill::One)
}

/// The scheme the desktop shows now, as the portal says.
fn scheme_now() -> Scheme {
    if is_dark() {
        Scheme::Dark
    } else {
        Scheme::Light
    }
}

/// Whether the portal says dark.
fn is_dark() -> bool {
    adw::StyleManager::default().is_dark()
}

/// The colours a settings file names, read leniently; none when the file
/// does not read or names none.
fn read_colours(path: &Path) -> Option<Colours> {
    let text = std::fs::read_to_string(path).ok()?;
    settings::read(&text).ok()?.file.appearance.colours
}

/// The High contrast row (M6.9), under the swatches: its title and switch,
/// its line and Reset and Copy as command come from `widgets::row`.
fn contrast_row(content: &gtk::Box) -> Contrast {
    let group = widgets::group(content);
    let switch = gtk::Switch::new();
    switch.set_valign(gtk::Align::Center);
    let row = widgets::row(&group, rows::title(CONTRAST), switch.upcast_ref());
    Contrast { switch, row }
}

/// What the High contrast row's line says: what it does, then where its
/// value comes from.
fn contrast_line(note: Option<&str>) -> String {
    [Some(tr(CONTRAST_LINE)), note]
        .into_iter()
        .flatten()
        .collect::<Vec<&str>>()
        .join(" · ")
}

/// What a colour row's line says: its scheme, when it is one scheme's own,
/// then where its value comes from.
fn line(scope: Scope, note: Option<&str>) -> String {
    let scheme = match scope {
        Scope::Both => None,
        Scope::Light => Some(tr("Light only")),
        Scope::Dark => Some(tr("Dark only")),
    };
    [scheme, note]
        .into_iter()
        .flatten()
        .collect::<Vec<&str>>()
        .join(" · ")
}

/// The swatch, a drawing of `SWATCH` (or `CELL`) pixels that the theme's
/// tokens redraw, with `draw` drawing it from the tokens.
fn drawn_area(
    theme: &Rc<Theme>,
    size: i32,
    draw: impl Fn(&Tokens, &gtk::cairo::Context, f64, f64) + 'static,
) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::builder()
        .content_width(size)
        .content_height(size)
        .build();
    let drawn = theme.clone();
    area.set_draw_func(move |_, cr, w, h| {
        let tokens = drawn.tokens();
        draw(&tokens, cr, f64::from(w), f64::from(h));
    });
    let weak = area.downgrade();
    theme.watch(move || {
        if let Some(area) = weak.upgrade() {
            area.queue_draw();
        }
    });
    area
}

/// Sets `colour` as the source of the next drawing.
fn set_source(cr: &gtk::cairo::Context, colour: Colour) {
    cr.set_source_rgba(
        f64::from(colour.r),
        f64::from(colour.g),
        f64::from(colour.b),
        f64::from(colour.a),
    );
}

/// Draws a swatch of `w` by `h` pixels: its circle of `CIRCLE` pixels filled
/// as `fill` says, or a ring with a plus, and, when `chosen`, the ring of the
/// chosen one (2 px in the text's colour, at the button's edge).
fn paint(cr: &gtk::cairo::Context, w: f64, h: f64, fill: Fill, chosen: bool, text: Colour) {
    let (cx, cy) = (w / 2.0, h / 2.0);
    let radius = CIRCLE / 2.0;
    match fill {
        Fill::One(colour) => {
            set_source(cr, colour);
            cr.arc(cx, cy, radius, 0.0, TAU);
            let _ = cr.fill();
        }
        Fill::Empty => {
            set_source(cr, text);
            cr.set_line_width(RING_WIDTH);
            cr.arc(cx, cy, radius - RING_WIDTH / 2.0, 0.0, TAU);
            let _ = cr.stroke();
            let arm = radius * PLUS;
            cr.move_to(cx - arm, cy);
            cr.line_to(cx + arm, cy);
            cr.move_to(cx, cy - arm);
            cr.line_to(cx, cy + arm);
            let _ = cr.stroke();
        }
    }
    if chosen {
        set_source(cr, text);
        cr.set_line_width(RING_WIDTH);
        cr.arc(cx, cy, f64::from(SWATCH) / 2.0 - RING_WIDTH / 2.0, 0.0, TAU);
        let _ = cr.stroke();
    }
}

/// One swatch: its name, its toggle button and, for an accent, its drawing.
/// The wallpaper's and the custom choice are pills with words (M5.12b6c).
struct Swatch {
    /// The accent's name, `wallpaper` or `custom`.
    name: String,
    button: gtk::ToggleButton,
    area: Option<gtk::DrawingArea>,
}

/// One colour row: its key, the colour it shows (read again at each show),
/// its drawing and the parts of its row that change.
struct ColourRow {
    key: String,
    value: Rc<Cell<Option<Colour>>>,
    area: gtk::DrawingArea,
    subtitle: gtk::Label,
    reset: gtk::Button,
}

/// The row that adds a colour: the drop-downs of the name and the scheme,
/// the colour button and the line that says why a pair was refused.
struct AddRow {
    name: gtk::DropDown,
    scope: gtk::DropDown,
    pick: gtk::Button,
    error: gtk::Label,
}

/// The High contrast row under the swatches (M6.9): a switch for
/// `appearance.contrast`, with Reset and Copy as command on the row.
struct Contrast {
    switch: gtk::Switch,
    row: widgets::Row,
}

/// The accent and the colours on the Appearance page (M5.12b1c).
pub struct Group {
    /// This group as the closures it hands out refer to it.
    me: Weak<Group>,
    theme: Rc<Theme>,
    /// The page's line for a problem, shown with a refusal too.
    problem: gtk::Label,
    /// The accent's source line, Reset and Copy as command.
    accent: widgets::Source,
    swatches: Vec<Swatch>,
    /// The custom colour chosen, which the custom pill's dialog starts from.
    custom: Rc<Cell<Option<Colour>>>,
    /// The High contrast switch and its row (M6.9).
    contrast: Contrast,
    /// The box the colour rows are built in, and the keys they were built for.
    list: gtk::Box,
    keys_shown: RefCell<Option<Vec<String>>>,
    rows: RefCell<Vec<ColourRow>>,
    add: AddRow,
    /// Set while `show` moves a toggle, so that is not a choice.
    quiet: Cell<bool>,
}

impl Group {
    /// Shows what the files say now: the accent's swatch chosen and the
    /// source line, the colours the files set, each row's line and Reset.
    /// The rows are built again only when the set of colours changes, so a
    /// colour changed in place keeps the keyboard where it was.
    pub fn show(&self, files: &Files) {
        let high = files.chosen(CONTRAST).as_deref() == Some("high");
        self.quiet.set(true);
        self.contrast.switch.set_active(high);
        self.quiet.set(false);
        let source = files.source(CONTRAST);
        self.contrast
            .row
            .subtitle
            .set_label(&contrast_line(rows::note(&source, false)));
        self.contrast
            .row
            .reset
            .set_visible(rows::resettable(&source));

        let chosen = files.chosen(KEY);
        let name = swatch_of(chosen.as_deref());
        let source = files.source(KEY);
        self.accent
            .label
            .set_label(&rows::describe(&source, &accent_title(&name)));
        self.accent.reset.set_visible(rows::resettable(&source));
        self.quiet.set(true);
        for swatch in &self.swatches {
            swatch.button.set_active(swatch.name == name);
        }
        self.quiet.set(false);
        self.custom.set(chosen.as_deref().and_then(Colour::parse));
        for swatch in &self.swatches {
            if let Some(area) = &swatch.area {
                area.queue_draw();
            }
        }

        let machine = read_colours(&files.machine);
        let person = files.person.as_deref().and_then(read_colours);
        let merged = settings::colours_over(machine.as_ref(), person.as_ref());
        let listed = listed(merged.as_ref());
        let keys: Vec<String> = listed.iter().map(Listed::key).collect();
        if self.keys_shown.borrow().as_ref() != Some(&keys) {
            self.rebuild(&listed);
            *self.keys_shown.borrow_mut() = Some(keys);
        }
        for (row, item) in self.rows.borrow().iter().zip(&listed) {
            row.value.set(Colour::parse(&item.value));
            row.area.queue_draw();
            let source = files.source(&row.key);
            row.subtitle
                .set_label(&line(item.scope, rows::note(&source, false)));
            row.reset.set_visible(rows::resettable(&source));
        }
    }

    /// Builds the colour rows again, one per colour, or the line that there
    /// are none.
    fn rebuild(&self, listed: &[Listed]) {
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        let rows: Vec<ColourRow> = listed.iter().map(|item| self.colour_row(item)).collect();
        if listed.is_empty() {
            widgets::note_row(&self.list, tr(NO_COLOURS));
        }
        *self.rows.borrow_mut() = rows;
    }

    /// One colour's row with its colour button, Reset and Copy as command,
    /// and the line under it that says why a change was refused.
    fn colour_row(&self, item: &Listed) -> ColourRow {
        let key = item.key();
        let value = Rc::new(Cell::new(Colour::parse(&item.value)));
        let shown = value.clone();
        let area = drawn_area(&self.theme, SWATCH, move |t, cr, w, h| {
            let fill = shown.get().map_or(Fill::Empty, Fill::One);
            paint(cr, w, h, fill, false, t.title_text);
        });
        let title = readable(&item.name);
        let button = gtk::Button::builder()
            .child(&area)
            .tooltip_text(tr("Change this colour"))
            .css_classes(["flat"])
            .build();
        let label = format!("{title}: {}", item.value);
        button.update_property(&[gtk::accessible::Property::Label(label.as_str())]);
        let row = widgets::row(&self.list, &title, button.upcast_ref());
        let error = error_label();
        self.list.append(&error);

        let (weak, now, theme) = (self.me.clone(), value.clone(), self.theme.clone());
        let (key_for_pick, error_for_pick) = (key.clone(), error.clone());
        button.connect_clicked(move |button| {
            let initial = now.get().unwrap_or_else(|| theme.tokens().accent);
            let (weak, key, error) = (weak.clone(), key_for_pick.clone(), error_for_pick.clone());
            widgets::choose_colour(button.upcast_ref(), initial, move |picked| {
                let Some(hex) = picked else {
                    return;
                };
                let Some(group) = weak.upgrade() else {
                    return;
                };
                let result = Files::here().set(&key, Some(&hex));
                group.tell(&error, result);
                group.show(&Files::here());
            });
        });

        let (weak, key_for_reset, error) = (self.me.clone(), key.clone(), error.clone());
        row.reset.connect_clicked(move |_| {
            let Some(group) = weak.upgrade() else {
                return;
            };
            let result = Files::here().set(&key_for_reset, None);
            group.tell(&error, result);
            group.show(&Files::here());
        });

        let key_for_copy = key.clone();
        row.copy.connect_clicked(move |button| {
            let Some(value) = Files::here().chosen(&key_for_copy) else {
                return;
            };
            button
                .clipboard()
                .set_text(&rows::command(&[(key_for_copy.as_str(), value)]));
            widgets::copied(button);
        });

        ColourRow {
            key,
            value,
            area,
            subtitle: row.subtitle,
            reset: row.reset,
        }
    }

    /// Writes high contrast on or off (M6.9): `high`, or `normal` taken out
    /// when the release's own is what applies, and logs what the file holds
    /// for CI. A machine's `high` is overruled by a written `normal`.
    fn write_contrast(&self, on: bool) {
        let value = if on { "high" } else { "normal" };
        let result = Files::here().choose_over(CONTRAST, value, Some("normal"));
        match Files::here().chosen(CONTRAST) {
            Some(now) => eprintln!("edel-settings: contrast {CONTRAST} {now} written"),
            None => eprintln!("edel-settings: contrast {CONTRAST} {value} taken out"),
        }
        self.report(result);
        self.show(&Files::here());
    }

    /// Writes `value` as the accent (`blue` is taken out), then shows what
    /// applies now.
    fn write_accent(&self, value: &str) {
        let release = release();
        let result = Files::here().choose_over(KEY, value, Some(&release));
        self.report(result);
        self.show(&Files::here());
    }

    /// Shows why a change was not written in the page's problem line, or
    /// takes an old message away.
    fn report(&self, result: Result<(), String>) {
        match result {
            Ok(()) => self.problem.set_visible(false),
            Err(e) => {
                self.problem.set_label(&e);
                self.problem.set_visible(true);
            }
        }
    }

    /// Shows a refusal under the row that tried it and in the problem line,
    /// or takes both away.
    fn tell(&self, error: &gtk::Label, result: Result<(), String>) {
        match &result {
            Ok(()) => error.set_visible(false),
            Err(e) => {
                error.set_label(e);
                error.set_visible(true);
            }
        }
        self.report(result);
    }
}

/// A line under a row for a refusal, red, hidden until there is one.
fn error_label() -> gtk::Label {
    gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .css_classes(["edel-error"])
        .visible(false)
        .build()
}

/// The add row (M5.12b1c): a drop-down of the colour tokens' names, Both,
/// Light or Dark, and a colour button that shows the token's colour and
/// opens the system's dialog.
fn add_row(theme: &Rc<Theme>, group: &gtk::Box) -> AddRow {
    let names: Vec<String> = tokens::COLOUR_NAMES
        .iter()
        .map(|name| readable(name))
        .collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let name = gtk::DropDown::from_strings(&refs);
    name.update_property(&[gtk::accessible::Property::Label(tr("Colour"))]);
    let scope = gtk::DropDown::from_strings(&[tr("Both"), tr("Light"), tr("Dark")]);
    scope.update_property(&[gtk::accessible::Property::Label(tr("Which scheme"))]);

    let chosen_name = name.clone();
    let area = drawn_area(theme, SWATCH, move |t, cr, w, h| {
        let fill = tokens::COLOUR_NAMES
            .get(chosen_name.selected() as usize)
            .and_then(|token| t.colour(token))
            .map_or(Fill::Empty, Fill::One);
        paint(cr, w, h, fill, false, t.title_text);
    });
    let weak = area.downgrade();
    name.connect_selected_notify(move |_| {
        if let Some(area) = weak.upgrade() {
            area.queue_draw();
        }
    });
    let pick = gtk::Button::builder()
        .child(&area)
        .tooltip_text(tr("Pick this colour"))
        .css_classes(["flat"])
        .build();

    let controls = gtk::Box::builder()
        .spacing(8)
        .valign(gtk::Align::Center)
        .build();
    controls.append(&name);
    controls.append(&scope);
    controls.append(&pick);
    let row = widgets::row(group, tr("Add a colour"), controls.upcast_ref());
    row.reset.set_visible(false);
    row.copy.set_visible(false);
    let error = error_label();
    group.append(&error);
    AddRow {
        name,
        scope,
        pick,
        error,
    }
}

/// The accent swatch of `name`: a toggle button of the swatch's size with a
/// tooltip and an accessible label, its circle drawn by [`swatch_area`].
fn swatch_button(theme: &Rc<Theme>, name: &str) -> (gtk::ToggleButton, gtk::DrawingArea) {
    let title = accent_title(name);
    let button = gtk::ToggleButton::builder()
        .tooltip_text(title.as_str())
        .css_classes(["edel-swatch"])
        .build();
    button.update_property(&[gtk::accessible::Property::Label(title.as_str())]);
    let area = swatch_area(theme, name, button.downgrade());
    button.set_child(Some(&area));
    (button, area)
}

/// The drawing of a swatch: its fill from the tokens' scheme, and the
/// chosen ring while its button is on.
fn swatch_area(
    theme: &Rc<Theme>,
    name: &str,
    button: glib::WeakRef<gtk::ToggleButton>,
) -> gtk::DrawingArea {
    let name = name.to_string();
    drawn_area(theme, SWATCH, move |t, cr, w, h| {
        let chosen = button.upgrade().is_some_and(|button| button.is_active());
        let fill = fill_of(&name, scheme_now());
        paint(cr, w, h, fill, chosen, t.title_text);
    })
}

/// The pill of the wallpaper's or the custom accent (M5.12b6c): an outlined
/// toggle with its words, chosen as a swatch is and, for custom, asking for
/// a colour on every click.
fn pill_button(name: &str) -> gtk::ToggleButton {
    let words = if name == WALLPAPER_SWATCH {
        tr("From wallpaper")
    } else {
        tr("Custom…")
    };
    let title = accent_title(name);
    let button = gtk::ToggleButton::builder()
        .label(words)
        .tooltip_text(title.as_str())
        .css_classes(["edel-accent-pill"])
        .build();
    button.update_property(&[gtk::accessible::Property::Label(title.as_str())]);
    button
}

/// The Accent and Colours groups of the Appearance page, after its Style
/// row (M5.12b1c): the accent's swatches in a flow, then the colours.
/// `show` follows the files; asked for by `appearance.accent`, the page
/// scrolls the swatches to the top and gives the keyboard to the chosen one,
/// and logs their places for CI.
pub fn group(content: &gtk::Box, theme: &Rc<Theme>, problem: &gtk::Label) -> Rc<Group> {
    let accent = widgets::section(content, rows::title(KEY));
    let custom = Rc::new(Cell::new(None));
    // One card: the nine accents, a divider, then the wallpaper's and the
    // custom pills. The accent's name is in the heading's line above, so
    // the card has no words of its own and never makes the page wider
    // than its window.
    let card = gtk::Box::builder()
        .spacing(12)
        .valign(gtk::Align::Start)
        .css_classes(["edel-group", "edel-accent-card"])
        .build();
    let mut first: Option<gtk::ToggleButton> = None;
    let mut swatches = Vec::new();
    for name in swatch_names() {
        let (button, area) = if name == WALLPAPER_SWATCH || name == CUSTOM_SWATCH {
            if name == WALLPAPER_SWATCH {
                let divider = gtk::Box::builder()
                    .valign(gtk::Align::Center)
                    .css_classes(["edel-accent-divider"])
                    .build();
                card.append(&divider);
            }
            (pill_button(&name), None)
        } else {
            let (button, area) = swatch_button(theme, &name);
            (button, Some(area))
        };
        match &first {
            Some(first) => button.set_group(Some(first)),
            None => first = Some(button.clone()),
        }
        card.append(&button);
        swatches.push(Swatch { name, button, area });
    }
    content.append(&card);
    let contrast = contrast_row(content);

    widgets::heading(content, tr("Colours"));
    widgets::note_row(content, tr(COLOURS_INTRO));
    let list = widgets::group(content);
    let add_group = widgets::group(content);
    let add = add_row(theme, &add_group);

    let group = Rc::new_cyclic(|me| Group {
        me: me.clone(),
        theme: theme.clone(),
        problem: problem.clone(),
        accent,
        swatches,
        custom,
        contrast,
        list,
        keys_shown: RefCell::new(None),
        rows: RefCell::new(Vec::new()),
        add,
        quiet: Cell::new(false),
    });
    wire(&group);
    let places: Vec<(String, gtk::ToggleButton)> = group
        .swatches
        .iter()
        .map(|swatch| (swatch.name.clone(), swatch.button.clone()))
        .collect();
    log_places(&card, places, group.accent.reset.clone());
    group.show(&Files::here());
    group
}

/// Connects the swatches, the accent's Reset and Copy as command, and the
/// add row. A swatch is chosen when it turns on, and Custom opens the
/// system's dialog on every click, so a click on the one chosen asks again.
fn wire(group: &Rc<Group>) {
    let weak = Rc::downgrade(group);
    for swatch in &group.swatches {
        let name = swatch.name.clone();
        let weak = weak.clone();
        if name == CUSTOM_SWATCH {
            swatch
                .button
                .connect_clicked(move |button| pick_custom(weak.clone(), button.clone()));
        } else {
            swatch.button.connect_toggled(move |button| {
                if !button.is_active() {
                    return;
                }
                if let Some(group) = weak.upgrade() {
                    if !group.quiet.get() {
                        group.write_accent(&name);
                    }
                }
            });
        }
    }

    let reset_weak = weak.clone();
    let switch_weak = weak.clone();
    group.contrast.switch.connect_active_notify(move |switch| {
        let Some(group) = switch_weak.upgrade() else {
            return;
        };
        if !group.quiet.get() {
            group.write_contrast(switch.is_active());
        }
    });
    let contrast_reset = weak.clone();
    group.contrast.row.reset.connect_clicked(move |_| {
        let Some(group) = contrast_reset.upgrade() else {
            return;
        };
        let result = Files::here().set(CONTRAST, None);
        group.report(result);
        group.show(&Files::here());
    });
    group.contrast.row.copy.connect_clicked(move |button| {
        let value = Files::here()
            .chosen(CONTRAST)
            .unwrap_or_else(|| "normal".to_string());
        button
            .clipboard()
            .set_text(&rows::command(&[(CONTRAST, value)]));
        widgets::copied(button);
    });
    group.accent.reset.connect_clicked(move |_| {
        let Some(group) = reset_weak.upgrade() else {
            return;
        };
        let result = Files::here().set(KEY, None);
        group.report(result);
        group.show(&Files::here());
    });
    group.accent.copy.connect_clicked(move |button| {
        let value = Files::here().chosen(KEY).unwrap_or_else(release);
        button.clipboard().set_text(&rows::command(&[(KEY, value)]));
        widgets::copied(button);
    });

    let add_weak = weak;
    group.add.pick.connect_clicked(move |button| {
        let Some(group) = add_weak.upgrade() else {
            return;
        };
        let Some(token) = tokens::COLOUR_NAMES
            .get(group.add.name.selected() as usize)
            .copied()
        else {
            return;
        };
        let scope = scope_at(group.add.scope.selected());
        let initial = group
            .theme
            .tokens()
            .colour(token)
            .unwrap_or_else(|| group.theme.tokens().accent);
        drop(group);
        let weak = add_weak.clone();
        widgets::choose_colour(button.upcast_ref(), initial, move |picked| {
            let Some(hex) = picked else {
                return;
            };
            let Some(group) = weak.upgrade() else {
                return;
            };
            let result = Files::here().set(&key_for(token, scope), Some(&hex));
            group.tell(&group.add.error, result);
            group.show(&Files::here());
        });
    });
}

/// Asks for the custom colour, starting from the one chosen (else the
/// accent), and writes it as `#rrggbb`; a cancelled choice shows the group
/// as it was.
fn pick_custom(group: Weak<Group>, button: gtk::ToggleButton) {
    let Some(now) = group.upgrade() else {
        return;
    };
    let current = now
        .custom
        .get()
        .unwrap_or_else(|| now.theme.tokens().accent);
    drop(now);
    widgets::choose_colour(button.upcast_ref(), current, move |picked| {
        let Some(group) = group.upgrade() else {
            return;
        };
        match picked {
            Some(hex) => group.write_accent(&hex),
            None => group.show(&Files::here()),
        }
    });
}

/// Logs where the swatches and the accent's Reset lie in the window, once
/// the flow is laid out, for CI to click (logical pixels), as `edel-settings:
/// accent group places NAME X+Y+WxH ..., reset X+Y+WxH`. Asked for by
/// `appearance.accent`, the page first scrolls the group to the top and
/// gives the keyboard to the chosen swatch; it logs once two frames agree.
fn log_places(flow: &gtk::Box, swatches: Vec<(String, gtk::ToggleButton)>, reset: gtk::Button) {
    let scrolled = Cell::new(false);
    let last = RefCell::new(String::new());
    flow.add_tick_callback(move |flow, _| {
        let Some(window) = flow.root().and_downcast::<gtk::Window>() else {
            return glib::ControlFlow::Continue;
        };
        if !flow.is_mapped() || flow.width() == 0 {
            return glib::ControlFlow::Continue;
        }
        if !scrolled.replace(true) {
            if widgets::asked_row() != Some(KEY) {
                return glib::ControlFlow::Break;
            }
            widgets::scroll_to_top(flow.upcast_ref());
            let page = std::iter::successors(
                Some(flow.clone().upcast::<gtk::Widget>()),
                gtk::Widget::parent,
            )
            .find(|w| w.has_css_class(widgets::ASKED));
            let chosen = swatches
                .iter()
                .find(|(_, button)| button.is_active())
                .map(|(_, button)| button.clone());
            if let (Some(page), Some(chosen)) = (page, chosen) {
                widgets::take_asked(&page, &chosen);
            }
            return glib::ControlFlow::Continue;
        }
        let places: Vec<String> = swatches
            .iter()
            .map(|(name, button)| {
                format!("{name} {}", widgets::place(button.compute_bounds(&window)))
            })
            .collect();
        let line = format!(
            "edel-settings: accent group places {}, reset {}",
            places.join(" "),
            widgets::place(reset.compute_bounds(&window)),
        );
        // The view settles over a few frames, so the places are logged only
        // once two frames agree.
        if last.replace(line.clone()) != line {
            return glib::ControlFlow::Continue;
        }
        eprintln!("{line}");
        glib::ControlFlow::Break
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn every_colour_token_reads_as_words() {
        assert_eq!(readable("panel_text"), "Panel text");
        assert_eq!(readable("lock_backdrop_deep"), "Lock backdrop deep");
        assert_eq!(readable("background"), "Background");
        for name in tokens::COLOUR_NAMES {
            let text = readable(name);
            assert!(!text.contains('_'), "{text}");
            assert!(text.starts_with(|c: char| c.is_uppercase()), "{text}");
        }
    }

    #[test]
    fn the_colours_listed_are_shared_then_light_then_dark_each_by_name() {
        let mut colours = Colours::default();
        colours.both.insert("panel".into(), "#111111".into());
        colours.both.insert("background".into(), "#222222".into());
        let mut light = BTreeMap::new();
        light.insert("window".to_string(), "#333333".to_string());
        colours.light = Some(light);
        let mut dark = BTreeMap::new();
        dark.insert("card".to_string(), "#444444".to_string());
        colours.dark = Some(dark);
        let keys: Vec<String> = listed(Some(&colours)).iter().map(Listed::key).collect();
        assert_eq!(
            keys,
            [
                "appearance.colours.background",
                "appearance.colours.panel",
                "appearance.colours.light.window",
                "appearance.colours.dark.card",
            ]
        );
        assert!(listed(None).is_empty());
    }

    #[test]
    fn a_colour_is_written_under_the_key_of_its_scheme() {
        assert_eq!(key_for("panel", Scope::Both), "appearance.colours.panel");
        assert_eq!(
            key_for("panel", Scope::Light),
            "appearance.colours.light.panel"
        );
        assert_eq!(
            key_for("panel", Scope::Dark),
            "appearance.colours.dark.panel"
        );
        let key = key_for("panel", Scope::Dark);
        assert!(settings::set("format = 1\n", &key, "#11111b").is_ok());
    }

    #[test]
    fn the_swatch_chosen_is_the_accent_the_value_names() {
        assert_eq!(swatch_of(None), "blue");
        assert_eq!(swatch_of(Some("red")), "red");
        assert_eq!(swatch_of(Some("wallpaper")), "wallpaper");
        assert_eq!(swatch_of(Some("#ff0000")), "custom");
        assert_eq!(swatch_of(Some("nonsense")), "blue");
    }

    #[test]
    fn the_swatches_are_the_accents_then_the_wallpaper_and_custom() {
        let names = swatch_names();
        assert_eq!(names.len(), tokens::accents().len() + 2);
        assert_eq!(names[0], "blue");
        assert_eq!(names[names.len() - 2], "wallpaper");
        assert_eq!(names[names.len() - 1], "custom");
    }

    #[test]
    fn an_accent_is_its_shade_and_one_without_a_colour_is_the_empty_ring() {
        let red = tokens::accent_named("red", Scheme::Dark).unwrap();
        assert_eq!(fill_of("red", Scheme::Dark), Fill::One(red));
        assert_eq!(fill_of("nonesuch", Scheme::Light), Fill::Empty);
    }
}
