//! The Appearance page's theme (M5.12b6b, ADR-008, ADR-010): the theme in
//! use with what the person set over it, the three styles, the themes row
//! and the pictures sheet of the theme in use. A theme is never changed by
//! a click on its picture (ADR-008's "nothing saved without the person's
//! word"); a value the person sets over a theme makes it Changed, and Reset
//! or Keep as new says what happens next. The library's writers do the
//! editing of the person's file (`edel::themes`, `edel::settings`), and the
//! page follows the files through `layout.rs`'s `Ui::update`, as
//! `wallpaper.rs` does. Every GTK call is here or in `widgets.rs`, `style.rs`
//! and `wallpaper.rs` (ADR-004's own look).

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk::cairo::Context;
use gtk::gdk;
use gtk::gdk::prelude::GdkCairoContextExt;
use gtk::gdk_pixbuf::Pixbuf;
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

use edel::i18n::{tr, trf};
use edel::places;
use edel::settings::{self, SettingsFile};
use edel::theme_file;
use edel::themes;
use edel::tokens::{Colour, Tokens};
use edel::wallpapers;

use crate::files::Files;
use crate::style::{self, Theme};
use crate::{icon, rows, wallpaper, widgets};

/// What a picture's menu, its drag or the sheet's Add pictures tile does to
/// the person's file (M5.12b6, part c2). `Card::edit` writes it through the
/// library's writers and shows the result.
#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    /// Adds pictures to the theme in use (Add pictures, Add to this theme).
    Add(Vec<String>),
    /// Adds a picture to another of the person's own themes (Add to theme,
    /// and a drop on its tile).
    AddTo { theme: String, picture: String },
    /// Moves a picture of the theme in use to another of the person's own
    /// themes (Move to theme).
    MoveTo { theme: String, picture: String },
    /// Takes a picture out of the theme in use (Remove).
    Remove(String),
}

/// The keys this card chooses.
const STYLE_KEY: &str = "appearance.mode";
const THEME_KEY: &str = "appearance.theme";
/// The three styles, as `appearance.mode` takes them (`auto` is the key table's name
/// for Automatic), in the order shown.
const STYLES: [&str; 3] = ["light", "dark", "auto"];
/// The size of a style's picture, a theme's tile and a theme's thumbnail,
/// in logical pixels, and the corners of each (the window's radius token,
/// the small one for thumbnails).
const STYLE_HEIGHT: i32 = 128;
/// The narrowest a tile is (its picture, with the frame around it adds 8):
/// six of them with their gaps fit the page, so the row scrolls past six.
const TILE_WIDTH: i32 = 92;
const TILE_HEIGHT: i32 = 70;
const THUMB: (i32, i32) = (20, 14);
const THUMB_STEP: i32 = 14;
const RADIUS: f64 = 10.0;
/// The corners of a style's window (7 px) and of a thumbnail (4 px).
const WINDOW_RADIUS: f64 = 7.0;
const THUMB_RADIUS: f64 = 4.0;
/// A style's window: its title strip's height, the panel strip along the
/// picture's foot, and the shadow's blur, all in logical pixels.
const TITLE_STRIP: f64 = 11.0;
const FOOT_STRIP: f64 = 13.0;
const SHADOW_BLUR: f64 = 20.0;
/// The themes row's columns: six, whatever the number of themes.
const COLUMNS: i32 = 6;
/// The largest a picture is loaded, in pixels: drawn cover-fitted from it.
const LOAD: (i32, i32) = (320, 220);

/// One scheme of a theme as its pictures are drawn: the theme's first
/// picture in that scheme, or the background where there is none, with the
/// colours a style's window is drawn in over it.
#[derive(Clone)]
struct Look {
    picture: Option<Pixbuf>,
    background: Colour,
    window: Colour,
    /// The title strip, its dots' grey and red, and the text of the foot strip.
    strip: Colour,
    muted: Colour,
    close: Colour,
    ink: Colour,
    accent: Colour,
    dark: bool,
}

impl Look {
    fn of(tokens: &Tokens, picture: Option<Pixbuf>, dark: bool) -> Look {
        Look {
            picture,
            background: tokens.background,
            window: tokens.window,
            // The card is darker than the window in dark, so there the strip
            // is the title bar's, which is lighter.
            strip: if dark {
                tokens.title_bar_focused
            } else {
                tokens.card
            },
            muted: tokens.title_text_unfocused,
            close: tokens.title_close_hover,
            ink: tokens.title_text,
            accent: tokens.accent,
            dark,
        }
    }
}

/// A theme's looks in both schemes.
#[derive(Clone)]
struct Looks {
    light: Look,
    dark: Look,
}

/// The theme's looks: its first picture in each scheme.
fn looks_of(dir: &Path, pictures: &[String]) -> Looks {
    let first = pictures.first();
    let picture = |dark: bool| first.and_then(|name| picture_of(dir, name, dark));
    Looks {
        light: Look::of(&style::load(false), picture(false), false),
        dark: Look::of(&style::load(true), picture(true), true),
    }
}

/// A picture a theme names: a wallpaper's name in the scheme's variant, or
/// a path, loaded fitted to `LOAD`; None when it is not there.
fn picture_of(dir: &Path, name: &str, dark: bool) -> Option<Pixbuf> {
    let path: PathBuf = if name.starts_with('/') {
        PathBuf::from(name)
    } else {
        wallpapers::find(dir, name, dark)?
    };
    Pixbuf::from_file_at_scale(path, LOAD.0, LOAD.1, true).ok()
}

/// What a drawing shows: one scheme's look, or the two split diagonally,
/// the light one top left and the dark one bottom right.
#[derive(Clone, Copy)]
enum Sides<'a> {
    One(&'a Look),
    Split(&'a Look, &'a Look),
}

/// Draws `sides` in the rectangle `w` by `h` with corners of `radius`, and
/// the windows over them when `windows` is set.
fn draw(cr: &Context, w: f64, h: f64, radius: f64, sides: Sides, windows: bool) {
    let _ = cr.save();
    rounded(cr, 0.0, 0.0, w, h, radius);
    cr.clip();
    match sides {
        Sides::One(look) => scene(cr, look, w, h, windows),
        Sides::Split(light, dark) => {
            let _ = cr.save();
            cr.move_to(0.0, 0.0);
            cr.line_to(w, 0.0);
            cr.line_to(0.0, h);
            cr.close_path();
            cr.clip();
            scene(cr, light, w, h, windows);
            let _ = cr.restore();
            let _ = cr.save();
            cr.move_to(w, 0.0);
            cr.line_to(w, h);
            cr.line_to(0.0, h);
            cr.close_path();
            cr.clip();
            scene(cr, dark, w, h, windows);
            let _ = cr.restore();
        }
    }
    let _ = cr.restore();
}

/// One look drawn over the whole rectangle, its window over it when asked.
fn scene(cr: &Context, look: &Look, w: f64, h: f64, windows: bool) {
    let _ = cr.save();
    cover(cr, look.picture.as_ref(), look.background, w, h);
    if windows {
        window(cr, look, w, h);
    }
    let _ = cr.restore();
}

/// The picture cover-fitted to the rectangle, as CSS draws `object-fit:
/// cover`, or the background colour when there is no picture.
fn cover(cr: &Context, picture: Option<&Pixbuf>, background: Colour, w: f64, h: f64) {
    match picture {
        Some(picture) => {
            let (pw, ph) = (f64::from(picture.width()), f64::from(picture.height()));
            let scale = (w / pw).max(h / ph);
            let _ = cr.save();
            cr.translate((w - pw * scale) / 2.0, (h - ph * scale) / 2.0);
            cr.scale(scale, scale);
            cr.set_source_pixbuf(picture, 0.0, 0.0);
            let _ = cr.paint();
            let _ = cr.restore();
        }
        None => {
            set_colour(cr, background);
            cr.rectangle(0.0, 0.0, w, h);
            let _ = cr.fill();
        }
    }
}

/// A window of the look over the picture, as the style pictures draw it
/// (M5.12b6c, to the board's sizes): its shadow, its body, the title strip
/// with three dots, two lines and an accent pill of content, and the
/// panel's strip along the foot with its two marks.
fn window(cr: &Context, look: &Look, w: f64, h: f64) {
    let (x, y, ww, wh) = (w * 0.17, h * 0.17, w * 0.54, h * 0.54);
    shadow(cr, x, y + 8.0, ww, wh, WINDOW_RADIUS, 0.22);
    rounded(cr, x, y, ww, wh, WINDOW_RADIUS);
    set_colour(cr, look.window);
    let _ = cr.fill();

    // Inside the window's corners: the title strip and its dots, then the
    // content.
    let _ = cr.save();
    rounded(cr, x, y, ww, wh, WINDOW_RADIUS);
    cr.clip();
    cr.rectangle(x, y, ww, TITLE_STRIP);
    set_colour(cr, look.strip);
    let _ = cr.fill();
    // Three 4 px dots, 3 px apart and 5 px from the right edge: the last is
    // red, the two before it grey.
    for (index, colour) in [look.close, look.muted, look.muted].into_iter().enumerate() {
        set_colour(cr, colour);
        cr.arc(
            x + ww - 7.0 - 7.0 * index as f64,
            y + TITLE_STRIP / 2.0,
            2.0,
            0.0,
            std::f64::consts::TAU,
        );
        let _ = cr.fill();
    }
    // The content, 9 px in from the left: a line 58 % wide, one 40 % wide
    // 5 px under it, and 9 px under those a pill of the accent.
    let (r, g, b, a) = if look.dark {
        (1.0, 1.0, 1.0, 0.18)
    } else {
        (0.0, 0.0, 0.0, 0.10)
    };
    cr.set_source_rgba(r, g, b, a);
    let top = y + TITLE_STRIP + 9.0;
    rounded(cr, x + 9.0, top, ww * 0.58, 5.0, 3.0);
    let _ = cr.fill();
    rounded(cr, x + 9.0, top + 10.0, ww * 0.40, 5.0, 3.0);
    let _ = cr.fill();
    set_colour(cr, look.accent);
    rounded(cr, x + 9.0, top + 24.0, ww * 0.30, 11.0, 6.0);
    let _ = cr.fill();
    let _ = cr.restore();

    // The panel's strip along the foot: white at 62 percent in light, black
    // at 55 in dark, with the text's colour as a square at its left and a
    // bar at its right, 8 px in from the edges.
    let (r, g, b, a) = if look.dark {
        (0.0, 0.0, 0.0, 0.55)
    } else {
        (1.0, 1.0, 1.0, 0.62)
    };
    cr.set_source_rgba(r, g, b, a);
    cr.rectangle(0.0, h - FOOT_STRIP, w, FOOT_STRIP);
    let _ = cr.fill();
    set_colour(cr, fade(look.ink, 0.7));
    rounded(cr, 8.0, h - FOOT_STRIP + 3.5, 6.0, 6.0, 2.0);
    let _ = cr.fill();
    set_colour(cr, fade(look.ink, 0.6));
    rounded(cr, w - 26.0, h - FOOT_STRIP + 4.5, 18.0, 4.0, 2.0);
    let _ = cr.fill();
}

/// A rounded rectangle's path, its corners no larger than half its sides.
fn rounded(cr: &Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    use std::f64::consts::{FRAC_PI_2, PI};
    let r = r.min(w / 2.0).min(h / 2.0);
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -FRAC_PI_2, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, FRAC_PI_2);
    cr.arc(x + r, y + h - r, r, FRAC_PI_2, PI);
    cr.arc(x + r, y + r, r, PI, 3.0 * FRAC_PI_2);
    cr.close_path();
}

/// A colour at `alpha`, the rest of it kept.
fn fade(colour: Colour, alpha: f32) -> Colour {
    Colour { a: alpha, ..colour }
}

/// A soft shadow of a rounded rectangle, black at `alpha` at its edge and
/// fading out over `SHADOW_BLUR` pixels as CSS's blur does, drawn as rings
/// of it growing out from the rectangle.
fn shadow(cr: &Context, x: f64, y: f64, w: f64, h: f64, radius: f64, alpha: f64) {
    const RINGS: u32 = 10;
    cr.set_source_rgba(0.0, 0.0, 0.0, alpha / f64::from(RINGS));
    for ring in 1..=RINGS {
        let grow = SHADOW_BLUR * f64::from(ring) / f64::from(RINGS);
        rounded(
            cr,
            x - grow,
            y - grow,
            w + 2.0 * grow,
            h + 2.0 * grow,
            radius + grow,
        );
        let _ = cr.fill();
    }
}

/// Sets `colour` as the source of the next drawing.
fn set_colour(cr: &Context, colour: Colour) {
    cr.set_source_rgba(
        f64::from(colour.r),
        f64::from(colour.g),
        f64::from(colour.b),
        f64::from(colour.a),
    );
}

/// The words of the changed keys, as the theme line says them: each row's
/// name in plain words, one each, then at most two and "and N more" for the
/// rest (`accent, corners and 1 more`).
pub fn changed_words(keys: &[String]) -> String {
    let mut words: Vec<String> = Vec::new();
    for key in keys {
        let word = rows::plain(key);
        if !words.contains(&word) {
            words.push(word);
        }
    }
    match words.as_slice() {
        [] => String::new(),
        [one] => one.clone(),
        [first, second] => trf(
            "{first} and {second}",
            &[("first", first), ("second", second)],
        ),
        [first, second, rest @ ..] => trf(
            "{first}, {second} and {more} more",
            &[
                ("first", first),
                ("second", second),
                ("more", &rest.len().to_string()),
            ],
        ),
    }
}

/// What the page shows of the theme in use, read from the files: the
/// person's file as it is (not with the theme laid under it), the machine's
/// for the theme only, as `edel::settings::chosen` reads it.
struct Shown {
    in_use: String,
    title: String,
    built_in: bool,
    pictures: Vec<String>,
    changed: Vec<String>,
    /// The person's own themes, as the file has them, with the theme in use
    /// named in it.
    file: SettingsFile,
}

impl Shown {
    fn of(files: &Files) -> Shown {
        let machine = files.machine_text();
        let person = files.person_text();
        let in_use = settings::chosen(THEME_KEY, Some(&machine), Some(&person))
            .unwrap_or_else(|| themes::DEFAULT.to_string());
        let mut file = settings::read(&person)
            .map(|read| read.file)
            .unwrap_or_default();
        if file.appearance.theme.is_none() {
            file.appearance.theme = Some(in_use.clone());
        }
        Shown {
            title: themes::title(&in_use, &file.themes),
            built_in: themes::is_built_in(&in_use),
            pictures: themes::pictures_of(&file, &in_use),
            changed: themes::changed(&file),
            in_use,
            file,
        }
    }

    /// The names of the person's own themes, for the copies' names.
    fn taken(&self) -> impl Fn(&str) -> bool + '_ {
        move |name| self.file.themes.contains_key(name)
    }
}

/// The New tile of the themes row (M5.12b6c): a dashed outline the size of
/// a tile with a plus in the accent, and its name under it. The lock's New
/// card is `widgets::new_card`, which this page does not share.
pub fn new_tile(caption: &str) -> gtk::Button {
    let plus = gtk::Label::builder()
        .label("+")
        .hexpand(true)
        .vexpand(true)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .css_classes(["edel-theme-new-plus"])
        .build();
    let outline = gtk::Box::builder()
        .css_classes(["edel-theme-new-look"])
        .hexpand(true)
        .build();
    outline.set_size_request(-1, TILE_HEIGHT);
    outline.append(&plus);
    let words = gtk::Label::builder()
        .label(caption)
        .justify(gtk::Justification::Center)
        .css_classes(["edel-theme-new-name"])
        .build();
    let inner = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(7)
        .hexpand(true)
        .build();
    inner.append(&outline);
    inner.append(&words);
    let button = gtk::Button::builder()
        .child(&inner)
        .hexpand(true)
        .css_classes(["edel-theme-new"])
        .build();
    button.update_property(&[gtk::accessible::Property::Label(caption)]);
    button
}

/// The Appearance page's theme card: the theme line, the styles, the themes
/// row and the pictures sheet.
pub struct Card {
    dir: PathBuf,
    theme: Rc<Theme>,
    problem: gtk::Label,
    /// Set while the page itself moves a control, which is not a choice.
    quiet: Cell<bool>,
    // The theme line.
    title: gtk::Label,
    tag: gtk::Label,
    stack: gtk::Fixed,
    stack_key: RefCell<String>,
    pictures: gtk::Button,
    pictures_words: gtk::Label,
    changed: gtk::Box,
    changed_label: gtk::Label,
    reset: gtk::Button,
    keep: gtk::Button,
    more: gtk::MenuButton,
    duplicate: gtk::Button,
    rename: gtk::Button,
    export: gtk::Button,
    delete: gtk::Button,
    // The styles.
    style_buttons: Vec<gtk::ToggleButton>,
    style_pictures: Vec<gtk::DrawingArea>,
    /// The words under Automatic, when the sun changes the style next, and its button (M7.14).
    auto_when: Option<(gtk::Label, gtk::ToggleButton)>,
    looks: Rc<RefCell<Looks>>,
    looks_key: RefCell<String>,
    pictures_now: RefCell<Vec<String>>,
    // The themes row.
    tiles_box: gtk::Grid,
    tiles_key: RefCell<String>,
    tiles: RefCell<Vec<(String, gtk::ToggleButton)>>,
    new_card: gtk::Button,
    new_menu: gtk::Popover,
    import: gtk::Button,
    /// The More menu's Import…, which does what the New card's does.
    more_import: gtk::Button,
    // The pictures sheet.
    sheet: gtk::Revealer,
    sheet_content: gtk::Box,
    sheet_key: RefCell<String>,
    wallpapers: RefCell<Option<Rc<wallpaper::Card>>>,
    /// The last theme line logged, so a line is logged once per change.
    changed_log: RefCell<String>,
}

/// Builds the theme card in `content`, after the page's intro: the theme
/// line and the styles, then the themes row, and the pictures sheet that
/// the theme line's button opens. `problem` shows what a change refused.
pub fn card(content: &gtk::Box, problem: &gtk::Label, theme: &Rc<Theme>) -> Rc<Card> {
    let dir = PathBuf::from(places::WALLPAPERS_DIR);
    let shown = Shown::of(&Files::here());

    // The theme line: the title and its tag, the pictures and the changes,
    // then Reset, Keep as new and the More menu.
    let title = gtk::Label::builder()
        .xalign(0.0)
        .valign(gtk::Align::Baseline)
        .css_classes(["edel-theme-title"])
        .build();
    let tag = gtk::Label::builder()
        .xalign(0.0)
        .valign(gtk::Align::Baseline)
        .css_classes(["edel-theme-tag"])
        .build();
    let names = gtk::Box::builder().spacing(10).build();
    names.append(&title);
    names.append(&tag);
    let stack = gtk::Fixed::new();
    // The pictures' words, and their arrow in a fainter colour.
    let pictures_words = gtk::Label::builder().xalign(0.0).build();
    let arrow = gtk::Label::builder()
        .label("›")
        .css_classes(["edel-pictures-arrow"])
        .build();
    let words_box = gtk::Box::builder().spacing(4).build();
    words_box.append(&pictures_words);
    words_box.append(&arrow);
    let pictures = gtk::Button::builder()
        .child(&words_box)
        .css_classes(["edel-pictures-link"])
        .valign(gtk::Align::Center)
        .build();
    let dot = gtk::Box::builder()
        .valign(gtk::Align::Center)
        .css_classes(["edel-changed-dot"])
        .build();
    dot.set_size_request(6, 6);
    let changed_label = gtk::Label::builder()
        .xalign(0.0)
        .css_classes(["edel-theme-changed"])
        .build();
    let changed = gtk::Box::builder().spacing(6).build();
    changed.append(&dot);
    changed.append(&changed_label);
    // The stack and its words 8 px apart, the changed line 14 px after them.
    let stacked = gtk::Box::builder()
        .spacing(8)
        .valign(gtk::Align::Center)
        .build();
    stacked.append(&stack);
    stacked.append(&pictures);
    let line = gtk::Box::builder().spacing(14).build();
    line.append(&stacked);
    line.append(&changed);
    let words = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build();
    words.append(&names);
    words.append(&line);
    let reset = widgets::action(tr("Reset"), false);
    reset.set_css_classes(&["edel-theme-reset"]);
    let keep = widgets::action(tr("Keep as new"), true);
    keep.set_css_classes(&["edel-theme-keep"]);
    let more = gtk::MenuButton::builder()
        .child(&icon::image("more", 15))
        .tooltip_text(tr("More"))
        .valign(gtk::Align::Center)
        .css_classes(["edel-theme-more"])
        .build();
    let menu = gtk::Popover::builder()
        .css_classes(["edel-choice-list"])
        .build();
    let items = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    let duplicate = widgets::menu_item(tr("Duplicate"));
    let rename = widgets::menu_item(tr("Rename…"));
    let export = widgets::menu_item(tr("Export…"));
    // Import beside Export, so a theme's file goes out and comes in from
    // the same menu (Alimardon, 2026-10-12); the New card offers it too.
    let more_import = widgets::menu_item(tr("Import…"));
    let delete = widgets::menu_item(tr("Delete…"));
    delete.add_css_class("edel-danger");
    for item in [&duplicate, &rename, &export, &more_import, &delete] {
        items.append(item);
    }
    menu.set_child(Some(&items));
    more.set_popover(Some(&menu));
    let actions = gtk::Box::builder()
        .spacing(8)
        .valign(gtk::Align::Center)
        .build();
    actions.append(&reset);
    actions.append(&keep);
    actions.append(&more);
    let head = gtk::Box::builder().spacing(16).margin_top(12).build();
    head.append(&words);
    head.append(&actions);
    content.append(&head);

    // The styles: three equal pictures, each with its word under it.
    let looks = Rc::new(RefCell::new(looks_of(&dir, &shown.pictures)));
    let styles = gtk::Box::builder()
        .spacing(16)
        .homogeneous(true)
        .margin_top(20)
        .build();
    let mut style_buttons: Vec<gtk::ToggleButton> = Vec::new();
    let mut style_pictures = Vec::new();
    let mut auto_when = None;
    for (index, name) in STYLES.iter().enumerate() {
        let picture = gtk::DrawingArea::builder()
            .content_height(STYLE_HEIGHT)
            .hexpand(true)
            .css_classes(["edel-style-picture"])
            .build();
        let drawn = looks.clone();
        let style = index;
        picture.set_draw_func(move |_, cr, w, h| {
            let looks = drawn.borrow();
            let (w, h) = (f64::from(w), f64::from(h));
            let sides = match style {
                0 => Sides::One(&looks.light),
                1 => Sides::One(&looks.dark),
                _ => Sides::Split(&looks.light, &looks.dark),
            };
            draw(cr, w, h, RADIUS, sides, true);
        });
        let frame = gtk::Box::builder()
            .css_classes(["edel-style-frame"])
            .hexpand(true)
            .build();
        frame.append(&picture);
        let title = match *name {
            "light" => tr("Light"),
            "dark" => tr("Dark"),
            _ => tr("Automatic"),
        };
        let word = gtk::Label::builder()
            .label(title)
            .css_classes(["edel-style-name"])
            .build();
        let inner = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(9)
            .build();
        inner.append(&frame);
        inner.append(&word);
        // Only Automatic says when the sun changes the style next (M7.14).
        let when = (index == 2).then(|| {
            let when = gtk::Label::builder()
                .css_classes(["edel-source"])
                .visible(false)
                .build();
            inner.append(&when);
            when
        });
        let button = gtk::ToggleButton::builder()
            .child(&inner)
            .css_classes(["edel-style"])
            .build();
        button.update_property(&[gtk::accessible::Property::Label(title)]);
        if let Some(first) = style_buttons.first() {
            button.set_group(Some(first));
        }
        styles.append(&button);
        if let Some(when) = when {
            auto_when = Some((when, button.clone()));
        }
        style_buttons.push(button);
        style_pictures.push(picture);
    }
    content.append(&styles);

    // The themes row: a heading with its aside, then the tiles in a row
    // that scrolls sideways when there are more than fit.
    let heading = widgets::heading(content, tr("Themes"));
    let aside = gtk::Label::builder()
        .label(tr("Light and dark in each"))
        .xalign(1.0)
        .css_classes(["edel-source"])
        .build();
    heading.append(&aside);
    let tiles_box = gtk::Grid::builder()
        .column_spacing(12)
        .column_homogeneous(true)
        .hexpand(true)
        .build();
    let row = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .child(&tiles_box)
        .build();
    content.append(&row);

    // The pictures sheet: the wallpaper cards of the theme in use, opened
    // under the theme line by its pictures button and built again when the
    // theme's pictures change. It opens on the page rather than in a popover,
    // because a popover is a window of its own, and CI clicks by the places
    // the window logs (M5.12b6b).
    let sheet_content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .css_classes(["edel-group", "edel-pictures-sheet"])
        .margin_top(12)
        .build();
    let sheet = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::SlideDown)
        .child(&sheet_content)
        .build();
    // Under the theme line, before the styles.
    content.insert_child_after(&sheet, Some(&head));
    // The New tile's menu.
    let new_card = new_tile(tr("New"));
    let new_items = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    let from_use = widgets::menu_item(tr("From the theme in use"));
    let blank = widgets::menu_item(tr("Blank"));
    let import = widgets::menu_item(tr("Import from a file…"));
    new_items.append(&from_use);
    new_items.append(&blank);
    new_items.append(&import);
    let new_menu = gtk::Popover::builder()
        .css_classes(["edel-choice-list"])
        .child(&new_items)
        .build();
    new_menu.set_parent(&new_card);

    let card = Rc::new(Card {
        dir,
        theme: theme.clone(),
        problem: problem.clone(),
        quiet: Cell::new(false),
        title,
        tag,
        stack,
        stack_key: RefCell::new(String::new()),
        pictures,
        pictures_words,
        changed,
        changed_label,
        reset: reset.clone(),
        keep: keep.clone(),
        more,
        duplicate: duplicate.clone(),
        rename: rename.clone(),
        export: export.clone(),
        delete: delete.clone(),
        style_buttons,
        style_pictures,
        auto_when,
        looks,
        looks_key: RefCell::new(shown.pictures.join("\n")),
        pictures_now: RefCell::new(shown.pictures.clone()),
        tiles_box,
        tiles_key: RefCell::new(String::new()),
        tiles: RefCell::new(Vec::new()),
        new_card: new_card.clone(),
        new_menu: new_menu.clone(),
        import: import.clone(),
        more_import: more_import.clone(),
        sheet,
        sheet_content,
        sheet_key: RefCell::new(String::new()),
        wallpapers: RefCell::new(None),
        changed_log: RefCell::new(String::new()),
    });
    wire(&card, &from_use, &blank);
    drop_file(&card, &row);
    card.show_now(&shown);
    card.show_when();
    // The tokens change with the scheme and the person's colours: the
    // styles' windows and panels are taken again then. The scheme also
    // flips at sunset, so the words under Automatic move on then (M7.14).
    let weak = Rc::downgrade(&card);
    theme.watch(move || {
        if let Some(card) = weak.upgrade() {
            card.looks_key.borrow_mut().clear();
            let pictures = card.pictures_now.borrow().clone();
            card.set_looks(&pictures);
            card.show_when();
        }
    });
    log_style(&card, content);
    log_themes(&card);
    card
}

/// Logs where the styles and the pictures button lie once the page is laid
/// out, for CI to click (logical pixels), as `edel-settings: appearance
/// group places style L D A, pictures X+Y+WxH`. Asked for by
/// `appearance.mode`, the page first scrolls the styles to the top and gives
/// the keyboard to the chosen one, as the other groups do. It logs once two
/// frames agree.
fn log_style(card: &Rc<Card>, content: &gtk::Box) {
    let styles = card.style_buttons.clone();
    let pictures = card.pictures.clone();
    let scrolled = Cell::new(false);
    let last = RefCell::new(String::new());
    let content: gtk::Widget = content.clone().upcast();
    content.clone().add_tick_callback(move |content, _| {
        let Some(window) = content.root().and_downcast::<gtk::Window>() else {
            return glib::ControlFlow::Continue;
        };
        if !content.is_mapped() || content.width() == 0 {
            return glib::ControlFlow::Continue;
        }
        if !scrolled.replace(true) {
            if widgets::asked_row() == Some(STYLE_KEY) {
                widgets::scroll_to_top(content);
                let chosen = styles.iter().find(|button| button.is_active());
                let page = std::iter::successors(Some(content.clone()), gtk::Widget::parent)
                    .find(|w| w.has_css_class(widgets::ASKED));
                if let (Some(page), Some(chosen)) = (page, chosen) {
                    widgets::take_asked(&page, chosen);
                }
            }
            return glib::ControlFlow::Continue;
        }
        let places: Vec<String> = styles
            .iter()
            .map(|button| widgets::place(button.compute_bounds(&window)))
            .collect();
        let line = format!(
            "edel-settings: appearance group places style {}, pictures {}",
            places.join(" "),
            widgets::place(pictures.compute_bounds(&window)),
        );
        if last.replace(line.clone()) != line {
            return glib::ControlFlow::Continue;
        }
        eprintln!("{line}");
        glib::ControlFlow::Break
    });
}

/// Logs where the tiles and the New tile lie once the row is laid out, for
/// CI and for people reading the log, as `edel-settings: themes places NAME
/// X+Y+WxH, ..., new X+Y+WxH`; it logs once two frames agree.
fn log_themes(card: &Rc<Card>) {
    let weak = Rc::downgrade(card);
    let last = RefCell::new(String::new());
    let row: gtk::Widget = card.tiles_box.clone().upcast();
    row.clone().add_tick_callback(move |row, _| {
        let Some(window) = row.root().and_downcast::<gtk::Window>() else {
            return glib::ControlFlow::Continue;
        };
        if !row.is_mapped() || row.width() == 0 {
            return glib::ControlFlow::Continue;
        }
        let Some(card) = weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        let mut places: Vec<String> = card
            .tiles
            .borrow()
            .iter()
            .map(|(name, button)| {
                format!("{name} {}", widgets::place(button.compute_bounds(&window)))
            })
            .collect();
        places.push(format!(
            "new {}",
            widgets::place(card.new_card.compute_bounds(&window))
        ));
        let line = format!("edel-settings: themes places {}", places.join(", "));
        drop(card);
        if last.replace(line.clone()) != line {
            return glib::ControlFlow::Continue;
        }
        eprintln!("{line}");
        glib::ControlFlow::Break
    });
}

/// Connects the theme line's buttons, the styles, the tiles' New menu and
/// the More menu, which each write a choice or ask for a name.
fn wire(card: &Rc<Card>, from_use: &gtk::Button, blank: &gtk::Button) {
    for (index, button) in card.style_buttons.iter().enumerate() {
        let weak = Rc::downgrade(card);
        let value = STYLES[index];
        button.connect_toggled(move |button| {
            if !button.is_active() {
                return;
            }
            if let Some(card) = weak.upgrade() {
                if !card.quiet.get() {
                    let result = Files::here().choose(STYLE_KEY, value);
                    card.report(result);
                    card.show_now(&Shown::of(&Files::here()));
                }
            }
        });
    }
    let weak = Rc::downgrade(card);
    card.pictures.connect_clicked(move |_| {
        if let Some(card) = weak.upgrade() {
            card.sheet.set_reveal_child(!card.sheet.reveals_child());
        }
    });
    let weak = Rc::downgrade(card);
    card.reset.connect_clicked(move |_| {
        if let Some(card) = weak.upgrade() {
            let result = Files::here().edit_person(|text| {
                themes::reset_text(text).map(|edited| edited.unwrap_or_else(|| text.to_string()))
            });
            card.report(result);
            card.show_now(&Shown::of(&Files::here()));
        }
    });
    let weak = Rc::downgrade(card);
    card.keep.connect_clicked(move |button| {
        let Some(card) = weak.upgrade() else {
            return;
        };
        let shown = Shown::of(&Files::here());
        let suggestion = themes::copy_name(&shown.title, shown.taken());
        let weak = Rc::downgrade(&card);
        widgets::ask_name(
            button.upcast_ref(),
            tr("Keep as new"),
            tr("Give the theme a name. It keeps the changes you made over it, and the theme in use becomes it."),
            &suggestion,
            tr("Name, such as my-aurora"),
            tr("Keep as new"),
            move |name| {
                let Some(card) = weak.upgrade() else {
                    return Ok(());
                };
                Files::here()
                    .edit_person(|text| themes::save_text(text, name))?;
                card.show_now(&Shown::of(&Files::here()));
                Ok(())
            },
        );
    });
    let weak = Rc::downgrade(card);
    card.duplicate.connect_clicked(move |_| {
        if let Some(card) = weak.upgrade() {
            card.more.popdown();
            let shown = Shown::of(&Files::here());
            let name = themes::copy_name(&shown.title, shown.taken());
            let result = Files::here().edit_person(|text| themes::save_text(text, &name));
            card.report(result);
            card.show_now(&Shown::of(&Files::here()));
        }
    });
    let weak = Rc::downgrade(card);
    card.rename.connect_clicked(move |button| {
        let Some(card) = weak.upgrade() else {
            return;
        };
        card.more.popdown();
        let shown = Shown::of(&Files::here());
        let old = shown.in_use.clone();
        let weak = Rc::downgrade(&card);
        widgets::ask_name(
            button.upcast_ref(),
            tr("Rename theme"),
            tr("Give this theme another name. It keeps its values and its pictures."),
            &old.clone(),
            tr("Name, such as my-aurora"),
            tr("Rename"),
            move |new| {
                let Some(card) = weak.upgrade() else {
                    return Ok(());
                };
                Files::here().edit_person(|text| themes::rename_text(text, &old, new))?;
                card.show_now(&Shown::of(&Files::here()));
                Ok(())
            },
        );
    });
    let weak = Rc::downgrade(card);
    card.export.connect_clicked(move |button| {
        let Some(card) = weak.upgrade() else {
            return;
        };
        card.more.popdown();
        let name = Shown::of(&Files::here()).in_use;
        export(&card, button.upcast_ref(), name);
    });
    let weak = Rc::downgrade(card);
    card.import.connect_clicked(move |button| {
        if let Some(card) = weak.upgrade() {
            card.new_menu.popdown();
            pick_import(&card, button.upcast_ref());
        }
    });
    let weak = Rc::downgrade(card);
    card.more_import.connect_clicked(move |button| {
        if let Some(card) = weak.upgrade() {
            card.more.popdown();
            pick_import(&card, button.upcast_ref());
        }
    });
    let weak = Rc::downgrade(card);
    card.delete.connect_clicked(move |button| {
        let Some(card) = weak.upgrade() else {
            return;
        };
        card.more.popdown();
        let shown = Shown::of(&Files::here());
        let name = shown.in_use.clone();
        let weak = Rc::downgrade(&card);
        widgets::ask_delete(
            button.upcast_ref(),
            &trf("Delete the theme {theme}?", &[("theme", &shown.title)]),
            tr("It cannot be brought back."),
            tr("Delete"),
            move || {
                let Some(card) = weak.upgrade() else {
                    return;
                };
                // The theme in use cannot be deleted, so Edel is chosen first,
                // in the same write, and the screen shows Edel after it.
                let result = Files::here().edit_person(|text| {
                    let text = settings::unset(text, THEME_KEY)?;
                    themes::delete_text(&text, &name)
                });
                if result.is_ok() {
                    eprintln!("edel-settings: theme {name} deleted");
                }
                card.report(result);
                card.show_now(&Shown::of(&Files::here()));
            },
        );
    });
    let weak = Rc::downgrade(card);
    from_use.connect_clicked(move |_| {
        if let Some(card) = weak.upgrade() {
            card.new_menu.popdown();
            let shown = Shown::of(&Files::here());
            let name = themes::copy_name(&shown.title, shown.taken());
            let result = Files::here().edit_person(|text| themes::save_text(text, &name));
            card.report(result);
            card.show_now(&Shown::of(&Files::here()));
        }
    });
    let weak = Rc::downgrade(card);
    blank.connect_clicked(move |_| {
        if let Some(card) = weak.upgrade() {
            card.new_menu.popdown();
            let shown = Shown::of(&Files::here());
            let name = themes::free_name("my theme", shown.taken());
            let result = Files::here().edit_person(|text| themes::blank_text(text, &name));
            card.report(result);
            card.show_now(&Shown::of(&Files::here()));
        }
    });
    let weak = Rc::downgrade(card);
    card.new_card.connect_clicked(move |_| {
        if let Some(card) = weak.upgrade() {
            card.new_menu.popup();
        }
    });
}

/// Asks where to save theme `name` (More's Export…) and writes its
/// `.edeltheme` file there, as `edel theme export` does; a refusal or a
/// failure shows in the page's problem line, and nothing is written then.
fn export(card: &Rc<Card>, near: &gtk::Widget, name: String) {
    let weak = Rc::downgrade(card);
    let near = near.clone();
    glib::spawn_future_local(async move {
        let dialog = gtk::FileDialog::builder()
            .title(tr("Export theme"))
            .modal(true)
            .build();
        dialog.set_initial_name(Some(&format!("{name}.{}", theme_file::EXTENSION)));
        let window = near.root().and_downcast::<gtk::Window>();
        let Some(path) = dialog
            .save_future(window.as_ref())
            .await
            .ok()
            .and_then(|file| file.path())
        else {
            return;
        };
        let Some(card) = weak.upgrade() else {
            return;
        };
        let result = export_to(&name, &path);
        if result.is_ok() {
            eprintln!(
                "edel-settings: themes exported {name} to {}",
                path.display()
            );
        }
        card.report(result);
    });
}

/// Writes theme `name` of the person's, as the library exports it, to `path`.
fn export_to(name: &str, path: &Path) -> Result<(), String> {
    let shown = Shown::of(&Files::here());
    let theme = themes::lookup(name, &shown.file.themes).ok_or_else(|| {
        trf(
            "there is no theme called {name}",
            &[("name", &format!("{name:?}"))],
        )
    })?;
    theme_file::export(name, &theme, path).map_err(|err| format!("{err:#}"))
}

/// Asks for a theme file (New's Import from a file…) in the system's file
/// chooser, which shows only `.edeltheme` files, and offers what it holds.
fn pick_import(card: &Rc<Card>, near: &gtk::Widget) {
    let weak = Rc::downgrade(card);
    let near = near.clone();
    glib::spawn_future_local(async move {
        let filter = gtk::FileFilter::new();
        filter.set_name(Some(tr("Edel OS themes")));
        filter.add_pattern(&format!("*.{}", theme_file::EXTENSION));
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let dialog = gtk::FileDialog::builder()
            .title(tr("Import a theme"))
            .modal(true)
            .filters(&filters)
            .build();
        let window = near.root().and_downcast::<gtk::Window>();
        let Some(path) = dialog
            .open_future(window.as_ref())
            .await
            .ok()
            .and_then(|file| file.path())
        else {
            return;
        };
        let Some(card) = weak.upgrade() else {
            return;
        };
        offer(&card, &near, path);
    });
}

/// A file dropped on the themes row does what Import from a file… does
/// with it (M5.12b6).
fn drop_file(card: &Rc<Card>, row: &gtk::ScrolledWindow) {
    let target = gtk::DropTarget::new(
        <gio::File as glib::prelude::StaticType>::static_type(),
        gtk::gdk::DragAction::COPY,
    );
    let weak = Rc::downgrade(card);
    let near: gtk::Widget = row.clone().upcast();
    target.connect_drop(move |_, value, _, _| {
        let Some(card) = weak.upgrade() else {
            return false;
        };
        let Some(path) = value.get::<gio::File>().ok().and_then(|file| file.path()) else {
            return false;
        };
        offer(&card, &near, path);
        true
    });
    row.add_controller(target);
}

/// Reads a theme file the person picked or dropped and checks it against
/// the settings file (no write yet): a file that does not check shows its
/// message in the problem line, and a good one opens the dialog that says
/// what it brings.
fn offer(card: &Rc<Card>, near: &gtk::Widget, path: PathBuf) {
    if path.extension().and_then(|ext| ext.to_str()) != Some(theme_file::EXTENSION) {
        let shown = path.display().to_string();
        card.report(Err(trf(
            "{file} is not a theme file; a theme file ends in .edeltheme",
            &[("file", &shown)],
        )));
        return;
    }
    let Some(dir) = edel::places::person_themes_dir() else {
        card.report(Err(tr(
            "there is no home folder for a theme's pictures; set HOME or XDG_DATA_HOME",
        )
        .to_string()));
        return;
    };
    let planned = theme_file::read(&path)
        .and_then(|theme| theme_file::plan(theme, &Files::here().person_text(), &dir));
    match planned {
        Ok(plan) => {
            card.report(Ok(()));
            eprintln!("edel-settings: themes import shown {}", plan.title);
            import_dialog(card, near, plan);
        }
        Err(err) => card.report(Err(format!("{err:#}"))),
    }
}

/// The words for a theme's pictures in the import dialog.
fn pictures_text(count: usize) -> String {
    match count {
        0 => tr("No pictures").to_string(),
        1 => tr("1 picture").to_string(),
        count => trf("{count} pictures", &[("count", &count.to_string())]),
    }
}

/// The words for the values an imported theme sets, or that it sets none.
fn values_text(keys: &[String]) -> String {
    if keys.is_empty() {
        tr("It sets no values of its own.").to_string()
    } else {
        trf("It sets {what}.", &[("what", &changed_words(keys))])
    }
}

/// The dialog an imported theme opens with: its title, what it brings and
/// what it sets, Cancel and Add theme (the default), as `widgets::ask_name`
/// builds its dialog.
fn import_dialog(card: &Rc<Card>, near: &gtk::Widget, plan: theme_file::Import) {
    let window = widgets::modal(near, tr("Add theme"), 360, |b| b);
    let title = gtk::Label::builder()
        .label(&plan.title)
        .xalign(0.0)
        .css_classes(["edel-theme-title"])
        .build();
    let pictures = gtk::Label::builder()
        .label(pictures_text(plan.pictures))
        .xalign(0.0)
        .wrap(true)
        .build();
    let values = gtk::Label::builder()
        .label(values_text(&plan.keys))
        .xalign(0.0)
        .wrap(true)
        .build();
    let cancel = widgets::action(tr("Cancel"), false);
    let add = widgets::action(tr("Add theme"), true);
    let buttons = widgets::dialog_buttons(&[&cancel, &add]);
    let body = widgets::dialog_body(12);
    body.append(&title);
    body.append(&pictures);
    body.append(&values);
    body.append(&buttons);
    window.set_child(Some(&body));
    window.set_default_widget(Some(&add));

    let close = window.clone();
    cancel.connect_clicked(move |_| close.close());
    let weak = Rc::downgrade(card);
    let plan = Rc::new(plan);
    let close = window.clone();
    add.connect_clicked(move |_| {
        close.close();
        if let Some(card) = weak.upgrade() {
            add_import(&card, &plan);
        }
    });
    window.present();
    add.grab_focus();
}

/// Adds an imported theme and chooses it (the dialog's Add theme): its
/// pictures go into the person's themes folder first, then one write names
/// the theme in `appearance.theme` and adds its table, so the file never
/// names a picture that is not there.
fn add_import(card: &Rc<Card>, plan: &theme_file::Import) {
    let result = plan
        .install()
        .map_err(|err| format!("{err:#}"))
        .and_then(|_| {
            Files::here().edit_person(|text| {
                let text = plan.add(text)?;
                settings::set(&text, THEME_KEY, &format!("{:?}", plan.name))
            })
        });
    if result.is_ok() {
        eprintln!("edel-settings: themes imported {}", plan.name);
    }
    card.report(result);
    card.show_now(&Shown::of(&Files::here()));
}

impl Card {
    /// Shows what the files say now (the page's update calls this).
    pub fn show(self: &Rc<Self>, files: &Files) {
        self.show_now(&Shown::of(files));
    }

    /// Shows `shown`: the theme line, the styles, the tiles and the sheet.
    fn show_now(self: &Rc<Self>, shown: &Shown) {
        self.quiet.set(true);
        self.title.set_label(&shown.title);
        self.tag.set_label(&if shown.built_in {
            tr("Built in theme").to_string()
        } else {
            tr("Your theme").to_string()
        });
        let text = match shown.pictures.len() {
            1 => tr("1 picture ›").to_string(),
            count => trf("{count} pictures ›", &[("count", &count.to_string())]),
        };
        self.pictures_words.set_label(text.replace('›', "").trim());
        let any_changed = !shown.changed.is_empty();
        self.changed.set_visible(any_changed);
        self.changed_label.set_label(&trf(
            "Changed: {what}",
            &[("what", &changed_words(&shown.changed))],
        ));
        self.reset.set_visible(any_changed);
        self.keep.set_visible(any_changed);
        self.rename.set_visible(!shown.built_in);
        self.delete.set_visible(!shown.built_in);
        self.more
            .update_property(&[gtk::accessible::Property::Label(
                trf("More for {theme}", &[("theme", &shown.title)]).as_str(),
            )]);
        self.show_stack(shown);
        self.set_looks(&shown.pictures);
        self.show_styles();
        self.show_tiles(shown);
        self.show_sheet(shown);
        self.quiet.set(false);
        self.problem.set_visible(false);
        // The theme line's state for CI and for people reading the log, as
        // `edel-settings: theme aurora changed accent` (or `nothing`); logged
        // when it changes, in English.
        let words: Vec<String> = shown.changed.iter().map(|key| rows::plain(key)).collect();
        let line = format!(
            "edel-settings: theme {} changed {}",
            shown.in_use,
            if words.is_empty() {
                "nothing".to_string()
            } else {
                words.join(" ")
            },
        );
        if self.changed_log.replace(line.clone()) != line {
            eprintln!("{line}");
        }
    }

    /// The looks of the theme in use, built again when its pictures change;
    /// the styles' pictures are drawn again at once.
    fn set_looks(&self, pictures: &[String]) {
        let key = pictures.join("\n");
        if *self.looks_key.borrow() == key {
            return;
        }
        *self.looks.borrow_mut() = looks_of(&self.dir, pictures);
        *self.looks_key.borrow_mut() = key;
        *self.pictures_now.borrow_mut() = pictures.to_vec();
        self.redraw_styles();
    }

    /// Draws the styles' pictures again, as the tokens or the pictures
    /// changed (the theme's watch calls this too).
    fn redraw_styles(&self) {
        for picture in &self.style_pictures {
            picture.queue_draw();
        }
    }

    /// Makes the styles' looks and draws the chosen style.
    fn show_styles(&self) {
        let mode = Files::here().layout().value(STYLE_KEY).unwrap_or_default();
        // Automatic is the release's choice while nothing is chosen.
        for (index, button) in self.style_buttons.iter().enumerate() {
            let on = STYLES.get(index).copied() == Some(mode.as_str())
                || (mode.is_empty() && index == 2);
            button.set_active(on);
        }
        self.show_when();
    }

    /// Shows under Automatic when the sun changes the style next, and says
    /// so on its button for a screen reader (M7.14). Nothing shows while the
    /// style is not Automatic's sun, or without a place.
    fn show_when(&self) {
        let Some((label, button)) = &self.auto_when else {
            return;
        };
        match Files::here().scheme().1 {
            Some(change) => {
                let words = edel::sun::change_words(change, edel::sun::now());
                label.set_label(&words);
                label.set_visible(true);
                button.update_property(&[gtk::accessible::Property::Label(
                    trf("Automatic, {when}", &[("when", &words)]).as_str(),
                )]);
            }
            None => {
                label.set_label("");
                label.set_visible(false);
                button.update_property(&[gtk::accessible::Property::Label(tr("Automatic"))]);
            }
        }
    }

    /// The theme's first three pictures as thumbnails, overlapping, drawn
    /// again only when the pictures change.
    fn show_stack(&self, shown: &Shown) {
        let key = format!("{}\n{}", shown.in_use, shown.pictures.join("\n"));
        if *self.stack_key.borrow() == key {
            return;
        }
        while let Some(child) = self.stack.first_child() {
            self.stack.remove(&child);
        }
        let looks = looks_of(&self.dir, &shown.pictures);
        let ring = style::load(false).window;
        let shown_count = shown.pictures.len().min(3) as i32;
        for index in 0..shown_count {
            let thumb = gtk::DrawingArea::builder()
                .content_width(THUMB.0)
                .content_height(THUMB.1)
                .css_classes(["edel-theme-thumb"])
                .build();
            let (light_look, dark_look) = (looks.light.clone(), looks.dark.clone());
            thumb.set_draw_func(move |_, cr, w, h| {
                let (w, h) = (f64::from(w), f64::from(h));
                draw(
                    cr,
                    w,
                    h,
                    THUMB_RADIUS,
                    Sides::Split(&light_look, &dark_look),
                    false,
                );
                // The ring of the window colour, the thumbnails' edge over
                // the next one.
                rounded(cr, 0.75, 0.75, w - 1.5, h - 1.5, THUMB_RADIUS);
                set_colour(cr, ring);
                cr.set_line_width(1.5);
                let _ = cr.stroke();
            });
            self.stack.put(&thumb, f64::from(index * THUMB_STEP), 0.0);
        }
        self.stack
            .set_size_request(THUMB.0 + THUMB_STEP * (shown_count - 1).max(0), THUMB.1);
        *self.stack_key.borrow_mut() = key;
    }

    /// The themes row: a tile per theme, built again when the list changes,
    /// the one in use chosen, and the New tile after them.
    fn show_tiles(self: &Rc<Self>, shown: &Shown) {
        let rows = themes::rows(&shown.file);
        let key: String = rows
            .iter()
            .map(|row| {
                let pictures = themes::pictures_of(&shown.file, &row.name);
                format!("{}|{}|{pictures:?}|{}", row.name, row.title, row.built_in)
            })
            .collect::<Vec<_>>()
            .join(";");
        if *self.tiles_key.borrow() != key {
            while let Some(child) = self.tiles_box.first_child() {
                self.tiles_box.remove(&child);
            }
            let mut tiles = Vec::new();
            let mut first: Option<gtk::ToggleButton> = None;
            let mut column = 0;
            for row in &rows {
                let pictures = themes::pictures_of(&shown.file, &row.name);
                let tile = self.tile(&row.name, &row.title, row.built_in, &pictures);
                match &first {
                    Some(first) => tile.set_group(Some(first)),
                    None => first = Some(tile.clone()),
                }
                self.tiles_box.attach(&tile, column, 0, 1, 1);
                column += 1;
                tiles.push((row.name.clone(), tile));
            }
            self.tiles_box.attach(&self.new_card, column, 0, 1, 1);
            column += 1;
            // Empty columns keep the row six wide, as the board has it, so
            // three themes and New leave two columns empty at the right.
            while column < COLUMNS {
                let empty = gtk::Box::builder().hexpand(true).build();
                self.tiles_box.attach(&empty, column, 0, 1, 1);
                column += 1;
            }
            *self.tiles.borrow_mut() = tiles;
            *self.tiles_key.borrow_mut() = key;
        }
        for (name, tile) in self.tiles.borrow().iter() {
            tile.set_active(*name == shown.in_use);
        }
    }

    /// One tile: the theme's picture split diagonally, the Built in badge on
    /// a built-in one, and its title under it. A click chooses it.
    fn tile(
        self: &Rc<Self>,
        name: &str,
        title: &str,
        built_in: bool,
        pictures: &[String],
    ) -> gtk::ToggleButton {
        let looks = Rc::new(looks_of(&self.dir, pictures));
        let picture = gtk::DrawingArea::builder()
            .content_width(TILE_WIDTH)
            .content_height(TILE_HEIGHT)
            .hexpand(true)
            .css_classes(["edel-tile-picture"])
            .build();
        let drawn = looks.clone();
        picture.set_draw_func(move |_, cr, w, h| {
            draw(
                cr,
                f64::from(w),
                f64::from(h),
                RADIUS,
                Sides::Split(&drawn.light, &drawn.dark),
                false,
            );
        });
        let overlay = gtk::Overlay::builder().child(&picture).build();
        let frame = gtk::Box::builder()
            .css_classes(["edel-tile-frame"])
            .hexpand(true)
            .build();
        frame.append(&overlay);
        if built_in {
            // The lock and the words at 9 px and 9.5 px, as the board draws
            // them: a small badge of its own rather than the pill's.
            let badge = gtk::Box::builder()
                .spacing(4)
                .halign(gtk::Align::Start)
                .valign(gtk::Align::Start)
                .margin_start(6)
                .margin_top(6)
                .css_classes(["edel-theme-built-in"])
                .build();
            badge.append(&icon::image("lock", 9));
            badge.append(&gtk::Label::new(Some(tr("Built in"))));
            overlay.add_overlay(&badge);
        }
        let label = gtk::Label::builder()
            .label(title)
            .justify(gtk::Justification::Center)
            .css_classes(["edel-tile-name"])
            .build();
        let inner = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(7)
            .hexpand(true)
            .build();
        inner.append(&frame);
        inner.append(&label);
        let tile = gtk::ToggleButton::builder()
            .child(&inner)
            .css_classes(["edel-tile"])
            .hexpand(true)
            .build();
        tile.update_property(&[gtk::accessible::Property::Label(title)]);
        // A picture dropped on one of the person's own themes is added to it;
        // the built-in ones take no drop (M5.12b6, part c2).
        if !built_in {
            let target = gtk::DropTarget::new(
                <String as glib::prelude::StaticType>::static_type(),
                gdk::DragAction::COPY,
            );
            let weak = Rc::downgrade(self);
            let to = name.to_string();
            target.connect_drop(move |_, value, _, _| {
                let Ok(picture) = value.get::<String>() else {
                    return false;
                };
                if let Some(card) = weak.upgrade() {
                    let edit = Edit::AddTo {
                        theme: to.clone(),
                        picture,
                    };
                    // Written once the drop is over: the write builds the tiles again.
                    glib::idle_add_local_once(move || card.edit(edit));
                }
                true
            });
            tile.add_controller(target);
        }
        let weak = Rc::downgrade(self);
        let chosen = name.to_string();
        tile.connect_toggled(move |tile| {
            if !tile.is_active() {
                return;
            }
            let Some(card) = weak.upgrade() else {
                return;
            };
            if card.quiet.get() {
                return;
            }
            let result = Files::here().choose_over(THEME_KEY, &chosen, Some(themes::DEFAULT));
            card.report(result);
            card.show_now(&Shown::of(&Files::here()));
        });
        tile
    }

    /// The pictures sheet: the theme in use's pictures first under its
    /// title, then every other wallpaper, as `wallpaper.rs` builds them.
    /// Built again when the theme's pictures change.
    fn show_sheet(self: &Rc<Self>, shown: &Shown) {
        let others: Vec<(String, String)> = themes::rows(&shown.file)
            .into_iter()
            .filter(|row| !row.built_in && !row.in_use)
            .map(|row| (row.name, row.title))
            .collect();
        let key = format!("{}\n{}\n{others:?}", shown.title, shown.pictures.join("\n"));
        if *self.sheet_key.borrow() != key {
            while let Some(child) = self.sheet_content.first_child() {
                self.sheet_content.remove(&child);
            }
            let title = trf("Pictures in {theme}", &[("theme", &shown.title)]);
            let weak = Rc::downgrade(self);
            let sheet = wallpaper::Sheet {
                others,
                run: Rc::new(move |edit| {
                    if let Some(card) = weak.upgrade() {
                        card.edit(edit);
                    }
                }),
            };
            let card = wallpaper::card(
                &self.sheet_content,
                &self.problem,
                &self.theme,
                &title,
                &shown.pictures,
                &sheet,
            );
            *self.wallpapers.borrow_mut() = Some(card);
            *self.sheet_key.borrow_mut() = key;
        }
        let files = Files::here();
        if let Some(card) = self.wallpapers.borrow().as_ref() {
            card.show(&files);
        }
    }

    /// Writes one edit of the pictures (M5.12b6, part c2) to the person's
    /// file through the library's writers, logs what it did for CI and shows
    /// the files as they are now. A refusal shows in the problem line.
    fn edit(self: &Rc<Self>, edit: Edit) {
        let files = Files::here();
        let shown = Shown::of(&files);
        let in_use = shown.in_use.clone();
        // How many of `pictures` the theme `to` did not hold yet.
        let new_in = |to: &str, pictures: &[String]| {
            let have = themes::pictures_of(&shown.file, to);
            pictures
                .iter()
                .filter(|picture| !have.contains(picture))
                .count()
        };
        let mut log: Option<String> = None;
        let result = files.edit_person(|text| match &edit {
            Edit::Add(pictures) => {
                log = Some(format!(
                    "themes pictures added {} to {in_use}",
                    new_in(&in_use, pictures)
                ));
                themes::add_pictures_text(text, &in_use, pictures)
            }
            Edit::AddTo { theme, picture } => {
                log = Some(format!(
                    "themes pictures added {} to {theme}",
                    new_in(theme, std::slice::from_ref(picture))
                ));
                themes::add_pictures_text(text, theme, std::slice::from_ref(picture))
            }
            Edit::MoveTo { theme, picture } => {
                log = Some(format!("themes picture moved from {in_use} to {theme}"));
                themes::move_picture_text(text, &in_use, theme, picture)
            }
            Edit::Remove(picture) => {
                log = Some(format!("themes picture removed from {in_use}"));
                themes::remove_picture_text(text, &in_use, picture)
            }
        });
        if result.is_ok() {
            if let Some(line) = log {
                eprintln!("edel-settings: {line}");
            }
        }
        self.report(result);
        self.show_now(&Shown::of(&Files::here()));
    }

    /// Shows why a change was not written, or takes an old message away.
    fn report(&self, result: Result<(), String>) {
        match result {
            Ok(()) => self.problem.set_visible(false),
            Err(e) => {
                self.problem.set_label(&e);
                self.problem.set_visible(true);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_changed_key_names_its_row() {
        assert_eq!(changed_words(&["appearance.accent".to_string()]), "accent");
    }

    #[test]
    fn three_changed_keys_name_two_and_the_rest() {
        let keys: Vec<String> = ["appearance.accent", "appearance.radius", "appearance.gaps"]
            .iter()
            .map(|key| key.to_string())
            .collect();
        assert_eq!(changed_words(&keys), "accent, corners and 1 more");
    }

    #[test]
    fn two_changed_keys_join_with_and_and_a_family_counts_once() {
        let keys: Vec<String> = [
            "appearance.colours.panel",
            "appearance.colours.window",
            "appearance.accent",
        ]
        .iter()
        .map(|key| key.to_string())
        .collect();
        assert_eq!(changed_words(&keys), "colours and accent");
        assert_eq!(changed_words(&[]), "");
    }

    #[test]
    fn the_import_dialog_counts_pictures_and_names_what_a_theme_sets() {
        assert_eq!(pictures_text(0), "No pictures");
        assert_eq!(pictures_text(1), "1 picture");
        assert_eq!(pictures_text(3), "3 pictures");
        assert_eq!(values_text(&[]), "It sets no values of its own.");
        let keys = vec!["appearance.accent".to_string()];
        assert_eq!(values_text(&keys), "It sets accent.");
    }
}
