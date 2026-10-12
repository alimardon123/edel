//! The Appearance page's Wallpaper group (M5.10c4c): a card per wallpaper
//! in `places::WALLPAPERS_DIR`, its picture the light half and the dark
//! half side by side, then None, Your picture (a file of the person's) and
//! Colour (a colour chosen in the system's dialog). Each choice is one
//! line of the person's settings file, `appearance.wallpaper`, written with
//! `Files::choose_over` so a choice that is what would apply is taken out
//! and never written (ADR-008). Your picture takes one picture or two: a
//! pair is `appearance.wallpaper` (the light one) and
//! `appearance.wallpaper_dark` (M5.10c4d), and one picture sets the mode to
//! suit it. What applies is `shown`: the person's or
//! the machine's value, else `release_value`, the default wallpaper when the
//! `wallpapers` feature carries it, else none. The group follows the files
//! through `layout.rs`'s `Ui::update`, as `tray.rs` does, and every GTK call
//! is here or in `widgets.rs` (ADR-004).

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};

use gtk::gdk_pixbuf::{Colorspace, Pixbuf};
use gtk::prelude::*;
use gtk::{gdk, glib};

use edel::i18n::{tr, trf};
use edel::places;
use edel::tokens::{Colour, Tokens};
use edel::wallpapers;

use crate::files::{self, Files};
use crate::icon;
use crate::style::Theme;
use crate::themes::Edit;
use crate::{rows, widgets};

/// The key this group chooses.
const KEY: &str = "appearance.wallpaper";
/// The picture for dark mode of a pair the person picked (M5.10c4d).
const DARK_KEY: &str = "appearance.wallpaper_dark";
/// The mode key one picture sets, and the brightness it is light from.
const MODE_KEY: &str = "appearance.mode";
const LIGHT_FROM: f32 = 0.5;
/// A card's picture in logical pixels: the size of the pictures' thumbnails.
const WIDTH: i32 = 192;
const HEIGHT: i32 = 120;
/// The keys of the three cards that are not a wallpaper's name, as the
/// log names them.
const NONE_CARD: &str = "none";
const PICTURE_CARD: &str = "picture";
const COLOUR_CARD: &str = "colour";

/// What applies when nobody chose: the default wallpaper when `dir` has
/// it, else none, the background colour alone.
pub fn release_value(dir: &Path) -> String {
    if wallpapers::find(dir, wallpapers::DEFAULT, false).is_some() {
        wallpapers::DEFAULT.to_string()
    } else {
        wallpapers::NONE.to_string()
    }
}

/// The value that applies: the chosen one, else what the release gives.
pub fn shown(chosen: Option<String>, dir: &Path) -> String {
    chosen.unwrap_or_else(|| release_value(dir))
}

/// What a value asks for, as the cards show it.
#[derive(Debug, PartialEq)]
enum Kind {
    Pair(String),
    None,
    Picture(PathBuf),
    Colour(String),
}

fn kind(value: &str) -> Kind {
    if value == wallpapers::NONE {
        Kind::None
    } else if value.starts_with('#') {
        Kind::Colour(value.to_string())
    } else if value.starts_with('/') {
        Kind::Picture(PathBuf::from(value))
    } else {
        Kind::Pair(value.to_string())
    }
}

/// A value as its line names it: a wallpaper's name with a capital, None,
/// the file name of the person's picture, or the colour.
pub fn title_of(value: &str) -> String {
    match kind(value) {
        Kind::Pair(name) => files::title(&name),
        Kind::None => tr("None").to_string(),
        Kind::Picture(path) => path.file_name().map_or_else(
            || tr("Your picture").to_string(),
            |name| name.to_string_lossy().into_owned(),
        ),
        Kind::Colour(hex) => hex,
    }
}

/// The Wallpaper group: a card per wallpaper, None, Your picture and
/// Colour, all in one group of toggles, with the group's source line and
/// Reset and Copy as command.
pub struct Card {
    dir: PathBuf,
    theme: Rc<Theme>,
    /// Every card in the order people see them: each wallpaper's name, then
    /// `none`, `picture` and `colour`, each with its badge.
    cards: Vec<(String, gtk::ToggleButton, gtk::Box)>,
    /// How many of `cards` are wallpapers' names.
    pairs: usize,
    /// The colour card's picture and the colour it shows, none for the accent.
    colour_area: gtk::DrawingArea,
    colour_shown: Rc<Cell<Option<Colour>>>,
    source: widgets::Source,
    problem: gtk::Label,
    /// Your picture's card: a prompt, or the pair's two halves.
    slot: gtk::Box,
    /// The pair the slot shows, so it is drawn again only when it changes.
    paired: RefCell<Option<(PathBuf, PathBuf)>>,
    /// Set while `show` moves a toggle, so that is not a choice.
    quiet: Cell<bool>,
}

impl Card {
    /// Shows the value that applies: its card chosen and badged, the
    /// colour card's colour, the pair's halves, and where the value comes
    /// from. In dark mode the label names the dark picture when there is one.
    pub fn show(&self, files: &Files) {
        let chosen = files.chosen(KEY);
        let chosen_dark = files.chosen(DARK_KEY);
        let value = shown(chosen, &self.dir);
        let dark = adw::StyleManager::default().is_dark();
        let applies = wallpapers::value_for(Some(value.as_str()), chosen_dark.as_deref(), dark)
            .unwrap_or(value.as_str())
            .to_string();
        self.show_pair(pair_of(&value, chosen_dark.as_deref()));
        let at = match kind(&value) {
            Kind::Pair(name) => self.cards[..self.pairs]
                .iter()
                .position(|(key, _, _)| *key == name),
            Kind::None => Some(self.pairs),
            Kind::Picture(_) => Some(self.pairs + 1),
            Kind::Colour(_) => Some(self.pairs + 2),
        };
        self.quiet.set(true);
        for (index, (_, button, badge)) in self.cards.iter().enumerate() {
            let on = Some(index) == at;
            button.set_active(on);
            badge.set_visible(on);
        }
        self.quiet.set(false);
        self.colour_shown.set(match kind(&value) {
            Kind::Colour(hex) => Colour::parse(&hex),
            _ => None,
        });
        self.colour_area.queue_draw();
        let source = files.source(KEY);
        self.source
            .label
            .set_label(&rows::describe(&source, &title_of(&applies)));
        self.source.reset.set_visible(rows::resettable(&source));
    }

    /// Draws Your picture's slot as `pair` says: the two halves, else the
    /// prompt. Drawn again only when the pair changes.
    fn show_pair(&self, pair: Option<(PathBuf, PathBuf)>) {
        if *self.paired.borrow() == pair {
            return;
        }
        while let Some(child) = self.slot.first_child() {
            self.slot.remove(&child);
        }
        match &pair {
            Some((light, dark)) => self.slot.append(&halves(light, dark, WIDTH, HEIGHT)),
            None => self.slot.append(&prompt()),
        }
        *self.paired.borrow_mut() = pair;
    }

    /// Writes `value` as the choice, and takes a person's dark picture out
    /// (a pair's second half never outlives another choice), then shows
    /// what applies now.
    fn write(&self, value: &str) {
        let release = release_value(&self.dir);
        let files = Files::here();
        let result = files
            .choose_over(KEY, value, Some(&release))
            .and_then(|()| unpair(&files));
        self.report(result);
        self.show(&Files::here());
    }

    /// Writes a pick (M5.10c4d): one picture as the choice with the mode
    /// that suits it, or a pair, light and dark. Logs what it wrote for CI.
    fn apply_pick(&self, pick: Pick) {
        let files = Files::here();
        let result = match &pick {
            Pick::One { path, mode } => {
                let release = release_value(&self.dir);
                files
                    .choose_over(KEY, path, Some(&release))
                    .and_then(|()| unpair(&files))
                    .and_then(|()| files.choose(MODE_KEY, mode))
                    .inspect(|()| {
                        eprintln!("edel-settings: wallpaper {path} is {mode}, mode set");
                    })
            }
            Pick::Pair { light, dark } => files
                .set(KEY, Some(light))
                .and_then(|()| files.set(DARK_KEY, Some(dark)))
                .inspect(|()| {
                    eprintln!("edel-settings: wallpaper pair {light} and {dark}");
                }),
        };
        self.report(result);
        self.show(&Files::here());
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

/// What a theme's pictures sheet adds to its cards (M5.12b6, part c2): the
/// person's own themes other than the one in use, by name and title, to add
/// pictures to or move them to, and `run`, which writes an [`Edit`] to the
/// person's file and shows the result (`themes.rs`).
pub struct Sheet {
    pub others: Vec<(String, String)>,
    pub run: Rc<dyn Fn(Edit)>,
}

/// The sheet's card in `content` after its heading (M5.10c4c, M5.12b6,
/// M5.12b6 part c2): the theme's own pictures first, each a reference (a
/// wallpaper's name, a picture's path or a colour) with a menu and a drag,
/// then every other wallpaper with the same menu, None, Your picture,
/// Colour and last the Add pictures tile. A click on a wallpaper's card
/// still writes `appearance.wallpaper`.
pub fn card(
    content: &gtk::Box,
    problem: &gtk::Label,
    theme: &Rc<Theme>,
    title: &str,
    first: &[String],
    sheet: &Sheet,
) -> Rc<Card> {
    let dir = PathBuf::from(places::WALLPAPERS_DIR);
    let source = widgets::section(content, title);
    // The theme's own pictures lead, each once, and the wallpapers it does
    // not hold follow; a picture of a path or a colour has a card too, so it
    // can be removed.
    let mut names: Vec<String> = Vec::new();
    for reference in first {
        if !names.contains(reference) {
            names.push(reference.clone());
        }
    }
    let split = names.len();
    let rest: Vec<String> = wallpapers::names(&dir)
        .into_iter()
        .filter(|name| !names.contains(name))
        .collect();
    names.extend(rest);
    let pairs = names.len();

    let colour_shown = Rc::new(Cell::new(None));
    let shown_colour = colour_shown.clone();
    let colour_area = swatch(theme, move |t| shown_colour.get().unwrap_or(t.accent));

    let mut parts: Vec<(String, String, gtk::Widget)> = names
        .iter()
        .map(|name| (name.clone(), title_of(name), reference_picture(&dir, name)))
        .collect();
    parts.push((
        NONE_CARD.to_string(),
        tr("None").to_string(),
        swatch(theme, |t| t.background).upcast::<gtk::Widget>(),
    ));
    let slot = picture_slot();
    parts.push((
        PICTURE_CARD.to_string(),
        tr("Your picture").to_string(),
        slot.clone().upcast::<gtk::Widget>(),
    ));
    parts.push((
        COLOUR_CARD.to_string(),
        tr("Colour").to_string(),
        colour_area.clone().upcast::<gtk::Widget>(),
    ));

    // Two flows when the theme has pictures of its own: those first, then a
    // heading, then the rest with None, Your picture and Colour.
    let flow = || {
        gtk::FlowBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .homogeneous(true)
            .min_children_per_line(1)
            .max_children_per_line(3)
            .column_spacing(12)
            .row_spacing(12)
            .build()
    };
    let (theme_flow, more_flow) = (flow(), flow());
    if split > 0 {
        content.append(&theme_flow);
    }
    widgets::heading(content, tr("More wallpapers"));
    content.append(&more_flow);
    let mut first: Option<gtk::ToggleButton> = None;
    let mut cards = Vec::new();
    for (index, (key, card_title, picture)) in parts.iter().enumerate() {
        let (button, badge) = widgets::picture_card(card_title, picture);
        if key == PICTURE_CARD {
            button.set_tooltip_text(Some(tr("One picture, or two for light and dark")));
        }
        match &first {
            Some(first) => button.set_group(Some(first)),
            None => first = Some(button.clone()),
        }
        // A reference of the theme or a wallpaper gets its menu and its drag;
        // None, Your picture and Colour do not.
        let placed: gtk::Widget = if index < names.len() {
            chrome(&button, key, index < split, sheet)
        } else {
            button.clone().upcast()
        };
        if index < split {
            theme_flow.append(&placed);
        } else {
            more_flow.append(&placed);
        }
        // The card takes the focus, not the box the flow wraps it in.
        if let Some(child) = placed.parent() {
            child.set_focusable(false);
        }
        cards.push((key.clone(), button, badge));
    }
    let add = crate::themes::new_tile(tr("Add pictures…"));
    let run = sheet.run.clone();
    add.connect_clicked(move |button| {
        let run = run.clone();
        widgets::choose_pictures(button.upcast_ref(), move |picked| {
            if let Some(paths) = picked.filter(|paths| !paths.is_empty()) {
                let paths = paths
                    .iter()
                    .map(|path| path.to_string_lossy().into_owned())
                    .collect();
                run(Edit::Add(paths));
            }
        });
    });
    more_flow.append(&add);

    let logged = cards.clone();
    let card = Rc::new(Card {
        dir,
        theme: theme.clone(),
        cards,
        pairs,
        colour_area,
        colour_shown,
        source,
        problem: problem.clone(),
        slot,
        paired: RefCell::new(None),
        quiet: Cell::new(false),
    });
    wire(&card);
    log_places(content.upcast_ref(), logged, card.source.reset.clone());
    card.show(&Files::here());
    card
}

/// A reference's card picture (M5.12b6, part c2): a wallpaper's pair of
/// halves, a picture's path on both halves, or a colour over the card.
fn reference_picture(dir: &Path, reference: &str) -> gtk::Widget {
    reference_picture_at(dir, reference, WIDTH, HEIGHT)
}

/// [`reference_picture`] at `width` by `height` logical pixels, for the
/// workspaces' small thumbnails (M5.12b5).
pub fn reference_picture_at(dir: &Path, reference: &str, width: i32, height: i32) -> gtk::Widget {
    match kind(reference) {
        Kind::Pair(name) => thumbnail(dir, &name, width, height),
        Kind::Picture(path) => halves(&path, &path, width, height),
        Kind::Colour(hex) => colour_picture(&hex, width, height),
        Kind::None => frame_at(width, height).upcast(),
    }
}

/// A colour's card picture: the colour over the whole card, or an empty
/// card when the colour does not parse.
fn colour_picture(hex: &str, width: i32, height: i32) -> gtk::Widget {
    let colour = Colour::parse(hex);
    let area = gtk::DrawingArea::builder()
        .content_width(width)
        .content_height(height)
        .css_classes(["edel-wallpaper-thumb"])
        .build();
    area.set_draw_func(move |_, cr, w, h| {
        let Some(c) = colour else {
            return;
        };
        cr.set_source_rgba(
            f64::from(c.r),
            f64::from(c.g),
            f64::from(c.b),
            f64::from(c.a),
        );
        cr.rectangle(0.0, 0.0, f64::from(w), f64::from(h));
        let _ = cr.fill();
    });
    area.upcast()
}

/// A picture card of the sheet with its menu and its drag (M5.12b6, part
/// c2): the reference is dragged onto another of the person's themes' tiles,
/// and its menu (`menu`) shows on hover and focus, and on a right click.
fn chrome(
    button: &gtk::ToggleButton,
    reference: &str,
    in_theme: bool,
    sheet: &Sheet,
) -> gtk::Widget {
    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(button));
    overlay.add_css_class("edel-picture-overlay");
    let drag = gtk::DragSource::new();
    drag.set_actions(gdk::DragAction::COPY);
    let payload = reference.to_string();
    drag.connect_prepare(move |_, _, _| Some(gdk::ContentProvider::for_value(&payload.to_value())));
    button.add_controller(drag);
    let more = menu(sheet, reference, in_theme);
    let weak = more.downgrade();
    overlay.add_overlay(&more);
    let click = gtk::GestureClick::builder().button(3).build();
    click.connect_pressed(move |_, _, _, _| {
        if let Some(more) = weak.upgrade() {
            more.popup();
        }
    });
    overlay.add_controller(click);
    overlay.upcast()
}

/// A picture's menu (M5.12b6, part c2): Add to this theme for a wallpaper
/// the theme does not hold, Add to theme and Move to theme for each of the
/// person's other own themes (by title, hidden when there are none), and
/// Remove for a picture of the theme in use. Each item writes its edit and
/// closes the menu.
fn menu(sheet: &Sheet, reference: &str, in_theme: bool) -> gtk::MenuButton {
    let more = gtk::MenuButton::builder()
        .child(&icon::image("more", 15))
        .tooltip_text(tr("More"))
        .css_classes(["edel-picture-more"])
        .halign(gtk::Align::End)
        .valign(gtk::Align::Start)
        .build();
    let items = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    if !in_theme {
        item(
            &items,
            &more,
            &sheet.run,
            tr("Add to this theme"),
            Edit::Add(vec![reference.to_string()]),
        );
    }
    if !sheet.others.is_empty() {
        items.append(&heading(tr("Add to theme")));
        for (name, title) in &sheet.others {
            item(
                &items,
                &more,
                &sheet.run,
                title,
                Edit::AddTo {
                    theme: name.clone(),
                    picture: reference.to_string(),
                },
            );
        }
    }
    if in_theme && !sheet.others.is_empty() {
        items.append(&heading(tr("Move to theme")));
        for (name, title) in &sheet.others {
            item(
                &items,
                &more,
                &sheet.run,
                title,
                Edit::MoveTo {
                    theme: name.clone(),
                    picture: reference.to_string(),
                },
            );
        }
    }
    if in_theme {
        let remove = item(
            &items,
            &more,
            &sheet.run,
            tr("Remove"),
            Edit::Remove(reference.to_string()),
        );
        remove.add_css_class("edel-danger");
    }
    let popover = gtk::Popover::builder()
        .css_classes(["edel-choice-list"])
        .child(&items)
        .build();
    more.set_popover(Some(&popover));
    more
}

/// One item of a picture's menu: it closes the menu and writes `edit`.
fn item(
    items: &gtk::Box,
    more: &gtk::MenuButton,
    run: &Rc<dyn Fn(Edit)>,
    label: &str,
    edit: Edit,
) -> gtk::Button {
    let button = widgets::menu_item(label);
    let run = run.clone();
    let weak = more.downgrade();
    button.connect_clicked(move |_| {
        if let Some(more) = weak.upgrade() {
            more.popdown();
        }
        // Written once the menu has closed: the write builds the sheet again.
        let (run, edit) = (run.clone(), edit.clone());
        glib::idle_add_local_once(move || run(edit));
    });
    items.append(&button);
    button
}

/// A heading inside a picture's menu, in the small secondary words.
fn heading(label: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(label)
        .xalign(0.0)
        .css_classes(["edel-source"])
        .build()
}

/// Connects each card, Reset and Copy as command. A wallpaper and None
/// are chosen when they turn on; Your picture and Colour open their
/// dialogs on every click, so a click on the one chosen asks again.
fn wire(card: &Rc<Card>) {
    let weak = Rc::downgrade(card);
    for (index, (key, button, _)) in card.cards.iter().enumerate() {
        let weak = weak.clone();
        match index.checked_sub(card.pairs) {
            None | Some(0) => {
                let value = key.clone();
                button.connect_toggled(move |button| {
                    if !button.is_active() {
                        return;
                    }
                    if let Some(card) = weak.upgrade() {
                        if !card.quiet.get() {
                            card.write(&value);
                        }
                    }
                });
            }
            Some(1) => {
                button.connect_clicked(move |button| {
                    pick_picture(weak.clone(), button.clone().upcast::<gtk::Widget>());
                });
            }
            Some(_) => {
                button.connect_clicked(move |button| pick_colour(weak.clone(), button.clone()));
            }
        }
    }
    let reset_weak = weak.clone();
    card.source.reset.connect_clicked(move |_| {
        let Some(card) = reset_weak.upgrade() else {
            return;
        };
        let files = Files::here();
        let result = files.set(KEY, None).and_then(|()| unpair(&files));
        card.report(result);
        card.show(&Files::here());
    });
    let dir = card.dir.clone();
    card.source.copy.connect_clicked(move |button| {
        let files = Files::here();
        let mut pairs = vec![(KEY, shown(files.chosen(KEY), &dir))];
        if let Some(dark) = files.chosen(DARK_KEY) {
            pairs.push((DARK_KEY, dark));
        }
        button.clipboard().set_text(&rows::command(&pairs));
        widgets::copied(button);
    });
}

/// Takes the person's dark picture out, if they have one: the second half
/// of a pair, which another choice ends. Nothing is written when they have
/// none, so a person with no home folder can still choose.
fn unpair(files: &Files) -> Result<(), String> {
    match files.source(DARK_KEY) {
        edel::settings::Source::Person(_) => files.set(DARK_KEY, None),
        _ => Ok(()),
    }
}

/// The two pictures a pair is, the light one and the dark one, when both
/// are pictures.
fn pair_of(light: &str, dark: Option<&str>) -> Option<(PathBuf, PathBuf)> {
    match (kind(light), dark.map(kind)) {
        (Kind::Picture(light), Some(Kind::Picture(dark))) => Some((light, dark)),
        _ => None,
    }
}

/// What the pictures a person picked choose (M5.10c4d).
#[derive(Debug, PartialEq)]
enum Pick {
    /// One picture, and the mode that suits it.
    One { path: String, mode: &'static str },
    /// Two pictures: the brighter is the light one.
    Pair { light: String, dark: String },
}

/// Chooses from the pictures picked: one, or two, and nothing else. A
/// picture that cannot be read or a choice of another number is the
/// message to show, and nothing is written.
fn pick(paths: &[PathBuf]) -> Result<Pick, String> {
    match paths {
        [one] => Ok(Pick::One {
            path: picture_value(one)?,
            mode: mode_for(brightness(one)?),
        }),
        [first, second] => {
            let (a, b) = (picture_value(first)?, picture_value(second)?);
            let (la, lb) = (brightness(first)?, brightness(second)?);
            Ok(if la >= lb {
                Pick::Pair { light: a, dark: b }
            } else {
                Pick::Pair { light: b, dark: a }
            })
        }
        _ => Err(tr("Choose one picture, or two for light and dark mode").to_string()),
    }
}

/// The mode a picture of this brightness suits: light when it is at least
/// half white.
fn mode_for(luminance: f32) -> &'static str {
    if luminance >= LIGHT_FROM {
        "light"
    } else {
        "dark"
    }
}

/// A picture's brightness, read from a small copy: its mean luminance,
/// 0 (black) to 1 (white). A picture that cannot be read says why.
fn brightness(path: &Path) -> Result<f32, String> {
    let small = Pixbuf::from_file_at_scale(path, 64, 64, true).map_err(|e| {
        trf(
            "could not read {path} as a picture: {why}",
            &[("path", &path.display().to_string()), ("why", e.message())],
        )
    })?;
    Ok(luminance(&to_rgba(&small)))
}

/// A pixbuf's pixels as RGBA, four bytes a pixel, whatever its channels
/// and rowstride (a picture without alpha has three).
fn to_rgba(pixbuf: &Pixbuf) -> Vec<u8> {
    let (width, height) = (pixbuf.width() as usize, pixbuf.height() as usize);
    let channels = pixbuf.n_channels() as usize;
    let stride = pixbuf.rowstride() as usize;
    let bytes = pixbuf.read_pixel_bytes();
    let mut out = Vec::with_capacity(width * height * 4);
    for y in 0..height {
        for x in 0..width {
            let at = y * stride + x * channels;
            match bytes.get(at..at + 3) {
                Some(p) => out.extend_from_slice(&[p[0], p[1], p[2], 255]),
                None => out.extend_from_slice(&[0, 0, 0, 255]),
            }
        }
    }
    out
}

/// The mean luminance of RGBA pixels, 0 (black) to 1 (white): each pixel's
/// 0.2126 red, 0.7152 green and 0.0722 blue, its alpha ignored.
pub fn luminance(rgba: &[u8]) -> f32 {
    let count = rgba.chunks_exact(4).len();
    if count == 0 {
        return 0.0;
    }
    let sum: f32 = rgba
        .chunks_exact(4)
        .map(|p| 0.2126 * f32::from(p[0]) + 0.7152 * f32::from(p[1]) + 0.0722 * f32::from(p[2]))
        .sum();
    sum / (count as f32 * 255.0)
}

/// Asks for one picture or two and chooses them (M5.10c4d). A cancelled
/// choice leaves the card as it was.
fn pick_picture(card: Weak<Card>, near: gtk::Widget) {
    widgets::choose_pictures(&near, move |picked| {
        let Some(card) = card.upgrade() else {
            return;
        };
        let Some(paths) = picked.filter(|paths| !paths.is_empty()) else {
            card.show(&Files::here());
            return;
        };
        match pick(&paths) {
            Ok(choice) => card.apply_pick(choice),
            Err(e) => {
                card.report(Err(e));
                card.show(&Files::here());
            }
        }
    });
}

/// The value a picture file chooses: its path, if it is a JPEG or PNG.
fn picture_value(path: &Path) -> Result<String, String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    match (ext.as_deref(), path.to_str()) {
        (Some("jpg" | "jpeg" | "png"), Some(text)) => Ok(text.to_string()),
        _ => Err(tr("Pick a JPEG or PNG picture").to_string()),
    }
}

/// Asks for a colour, starting from the one chosen (else the accent), and
/// chooses it as `#rrggbb` (`widgets::choose_colour`). A cancelled choice
/// leaves the card as it was.
fn pick_colour(card: Weak<Card>, button: gtk::ToggleButton) {
    let Some(now) = card.upgrade() else {
        return;
    };
    let current = now
        .colour_shown
        .get()
        .unwrap_or_else(|| now.theme.tokens().accent);
    drop(now);
    widgets::choose_colour(button.upcast_ref(), current, move |picked| {
        let Some(card) = card.upgrade() else {
            return;
        };
        match picked {
            Some(hex) => card.write(&hex),
            None => card.show(&Files::here()),
        }
    });
}

/// An empty picture the card's size, which the thumbnails and the prompt
/// share.
fn frame() -> gtk::Box {
    frame_at(WIDTH, HEIGHT)
}

/// [`frame`] at `width` by `height`.
fn frame_at(width: i32, height: i32) -> gtk::Box {
    let frame = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .css_classes(["edel-wallpaper-thumb"])
        .build();
    frame.set_size_request(width, height);
    frame
}

/// Your picture's card before a pair is chosen: a prompt where a picture
/// would be, in a frame that holds the pair's halves once there is one.
fn picture_slot() -> gtk::Box {
    let slot = frame();
    slot.append(&prompt());
    slot
}

/// The prompt in Your picture's card.
fn prompt() -> gtk::Label {
    gtk::Label::builder()
        .label(tr("Choose a picture"))
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .vexpand(true)
        .build()
}

/// A wallpaper's card picture: its light picture on the left half and its
/// dark picture on the right, each scaled to the card. A name with only one
/// picture uses it for both halves; a picture that will not load leaves an
/// empty card.
fn thumbnail(dir: &Path, name: &str, width: i32, height: i32) -> gtk::Widget {
    match (
        wallpapers::find(dir, name, false),
        wallpapers::find(dir, name, true),
    ) {
        (Some(light), Some(dark)) => halves(&light, &dark, width, height),
        (Some(one), None) | (None, Some(one)) => halves(&one, &one, width, height),
        (None, None) => frame_at(width, height).upcast(),
    }
}

/// Two pictures' card picture: the light one on the left half and the dark
/// one on the right, each scaled to the card (M5.10c4d). A picture that
/// will not load leaves an empty card.
fn halves(light: &Path, dark: &Path, width: i32, height: i32) -> gtk::Widget {
    let load = |path: &Path| Pixbuf::from_file_at_scale(path, width, height, false).ok();
    let (Some(light), Some(dark)) = (load(light), load(dark)) else {
        return frame_at(width, height).upcast();
    };
    let Some(both) = Pixbuf::new(Colorspace::Rgb, false, 8, width, height) else {
        return frame_at(width, height).upcast();
    };
    let half = width / 2;
    light.copy_area(0, 0, half, height, &both, 0, 0);
    dark.copy_area(half, 0, half, height, &both, half, 0);
    let picture = gtk::Picture::for_paintable(&gdk::Texture::for_pixbuf(&both));
    picture.set_can_shrink(false);
    picture.set_content_fit(gtk::ContentFit::Cover);
    picture.set_size_request(width, height);
    picture.add_css_class("edel-wallpaper-thumb");
    picture.set_overflow(gtk::Overflow::Hidden);
    picture.upcast()
}

/// A card's colour as drawn: the colour `pick` gives over the whole card,
/// with a hairline in the tokens' line colour. Drawn again when the tokens
/// change.
fn swatch(theme: &Rc<Theme>, pick: impl Fn(&Tokens) -> Colour + 'static) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::builder()
        .content_width(WIDTH)
        .content_height(HEIGHT)
        .css_classes(["edel-wallpaper-thumb"])
        .build();
    let drawn = theme.clone();
    area.set_draw_func(move |_, cr, w, h| {
        let t = drawn.tokens();
        let (w, h) = (f64::from(w), f64::from(h));
        let c = pick(&t);
        cr.set_source_rgba(
            f64::from(c.r),
            f64::from(c.g),
            f64::from(c.b),
            f64::from(c.a),
        );
        cr.rectangle(0.0, 0.0, w, h);
        let _ = cr.fill();
        let line = t.line;
        cr.set_source_rgba(
            f64::from(line.r),
            f64::from(line.g),
            f64::from(line.b),
            f64::from(line.a),
        );
        cr.set_line_width(1.0);
        cr.rectangle(0.5, 0.5, w - 1.0, h - 1.0);
        let _ = cr.stroke();
    });
    let weak = area.downgrade();
    theme.watch(move || {
        if let Some(area) = weak.upgrade() {
            area.queue_draw();
        }
    });
    area
}

/// Logs where the group's cards and its Reset lie in the window, once the
/// flow is laid out, for CI to click (logical pixels), as `edel-settings:
/// wallpaper group places NAME X+Y+WxH ..., reset X+Y+WxH`. Asked for by
/// this row's key, the page scrolls the group to the top (again on each
/// frame while the sheet opens) and gives the keyboard to the chosen card,
/// as the Style group does; it logs once two frames agree.
fn log_places(
    flow: &gtk::Widget,
    cards: Vec<(String, gtk::ToggleButton, gtk::Box)>,
    reset: gtk::Button,
) {
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
            let chosen = cards
                .iter()
                .find(|(_, button, _)| button.is_active())
                .map(|(_, button, _)| button.clone());
            if let (Some(page), Some(chosen)) = (page, chosen) {
                widgets::take_asked(&page, &chosen);
            }
            return glib::ControlFlow::Continue;
        }
        // The pictures sheet opens with an animation, so the page may not be
        // tall enough yet on the first frame: the group is scrolled to the
        // top again on every frame until the places settle.
        widgets::scroll_to_top(flow.upcast_ref());
        let places: Vec<String> = cards
            .iter()
            .map(|(key, button, _)| {
                format!("{key} {}", widgets::place(button.compute_bounds(&window)))
            })
            .collect();
        let line = format!(
            "edel-settings: wallpaper group places {}, reset {}",
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

    /// A folder of the test's own, named after the test and this process.
    fn folder(test: &str, files: &[&str]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("edel-settings-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for file in files {
            std::fs::write(dir.join(file), b"").unwrap();
        }
        dir
    }

    #[test]
    fn nobody_choosing_gives_the_default_when_carried_else_none() {
        let with = folder("release_default", &["hills-light.jpg"]);
        assert_eq!(release_value(&with), "hills");
        let without = folder("release_none", &["earth.png"]);
        assert_eq!(release_value(&without), "none");
    }

    #[test]
    fn the_value_shown_is_the_chosen_one_else_the_release() {
        let dir = folder("shown", &["hills-light.jpg"]);
        assert_eq!(shown(Some("#1f2b48".into()), &dir), "#1f2b48");
        assert_eq!(shown(Some("none".into()), &dir), "none");
        assert_eq!(shown(None, &dir), "hills");
    }

    #[test]
    fn a_value_is_none_a_colour_a_picture_or_a_name() {
        assert_eq!(kind("none"), Kind::None);
        assert_eq!(kind("#1f2b48"), Kind::Colour("#1f2b48".into()));
        assert_eq!(
            kind("/home/ali/sea.jpg"),
            Kind::Picture(PathBuf::from("/home/ali/sea.jpg"))
        );
        assert_eq!(kind("hills"), Kind::Pair("hills".into()));
    }

    #[test]
    fn a_value_reads_as_its_name_or_its_colour() {
        assert_eq!(title_of("hills"), "Hills");
        assert_eq!(title_of("none"), "None");
        assert_eq!(title_of("#1f2b48"), "#1f2b48");
        assert_eq!(title_of("/home/ali/sea.jpg"), "sea.jpg");
    }

    #[test]
    fn luminance_is_the_mean_brightness_and_alpha_is_ignored() {
        assert_eq!(luminance(&[0, 0, 0, 255, 0, 0, 0, 255]), 0.0);
        assert!((luminance(&[255, 255, 255, 255, 255, 255, 255, 255]) - 1.0).abs() < 1e-4);
        // Half black, half white: the mean is a half.
        assert!((luminance(&[0, 0, 0, 255, 255, 255, 255, 255]) - 0.5).abs() < 1e-4);
        // Alpha does not count.
        assert!((luminance(&[255, 255, 255, 0]) - 1.0).abs() < 1e-4);
        assert_eq!(luminance(&[]), 0.0);
    }

    #[test]
    fn one_picture_suits_light_mode_when_it_is_at_least_half_white() {
        assert_eq!(mode_for(0.5), "light");
        assert_eq!(mode_for(0.9), "light");
        assert_eq!(mode_for(0.49), "dark");
        assert_eq!(mode_for(0.0), "dark");
    }

    /// A PNG of one colour, written in a folder of the test's own.
    fn solid(dir: &Path, name: &str, pixel: u32, channels_alpha: bool) -> PathBuf {
        let _ = std::fs::create_dir_all(dir);
        let path = dir.join(name);
        let pixbuf = Pixbuf::new(Colorspace::Rgb, channels_alpha, 8, 4, 4).unwrap();
        pixbuf.fill(pixel);
        pixbuf.savev(&path, "png", &[]).unwrap();
        path
    }

    #[test]
    fn two_pictures_are_a_pair_with_the_brighter_as_light() {
        let dir = std::env::temp_dir().join(format!("edel-pick-pair-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let white = solid(&dir, "white.png", 0xffffffff, true);
        let black = solid(&dir, "black.png", 0x000000ff, true);
        assert_eq!(
            pick(&[black.clone(), white.clone()]),
            Ok(Pick::Pair {
                light: white.to_str().unwrap().to_string(),
                dark: black.to_str().unwrap().to_string(),
            })
        );
        let one = pick(std::slice::from_ref(&black)).unwrap();
        assert_eq!(
            one,
            Pick::One {
                path: black.to_str().unwrap().to_string(),
                mode: "dark",
            }
        );
        let light = pick(std::slice::from_ref(&white)).unwrap();
        assert!(matches!(light, Pick::One { mode: "light", .. }));
        assert!(pick(&[white.clone(), black.clone(), white.clone()]).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_picture_without_alpha_is_read_like_one_with() {
        let dir = std::env::temp_dir().join(format!("edel-pick-rgb-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let grey = solid(&dir, "grey.png", 0xffffffff, false);
        let lum = brightness(&grey).unwrap();
        assert!((lum - 1.0).abs() < 1e-3, "{lum}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_picture_that_cannot_be_read_says_so() {
        let missing = PathBuf::from("/no/such/edel-picture.png");
        let err = pick(std::slice::from_ref(&missing)).unwrap_err();
        assert!(
            err.starts_with("could not read /no/such/edel-picture.png as a picture"),
            "{err}"
        );
    }

    #[test]
    fn a_picture_is_a_jpeg_or_a_png() {
        assert_eq!(
            picture_value(Path::new("/home/ali/Sea.JPG")),
            Ok("/home/ali/Sea.JPG".to_string())
        );
        assert!(picture_value(Path::new("/home/ali/sea.gif")).is_err());
        assert!(picture_value(Path::new("/home/ali/sea")).is_err());
    }
}
