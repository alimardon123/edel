//! The Lock screen page (M5.10c5c): the look the lock and login screens
//! show, what lies behind it, the clock's face and hours, the date's style
//! and whether the count of unread notifications shows. Each choice is one
//! key of the person's settings file, written with `Files` as the other
//! pages write theirs (ADR-008): `lock_screen.look`, `lock_screen.background`,
//! `lock_screen.notifications`, `lock_screen.looks` (Save as) and
//! `lock_screen.parts`, which holds the whole list when a clock's or date's
//! option changes or a look is replaced. The cards are drawn from the parts
//! the lock draws, laid out by `edel::lock_screen::arrange` on a 1280 by
//! 800 screen, so a card shows where each part stands.

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;

use adw::prelude::*;
use gtk::cairo::{Context, LinearGradient};
use gtk::{gdk, gio, glib};

use edel::i18n::{n_, tr, trf};
use edel::lock_screen::{
    self, BACKGROUND, Background, DateStyle, Face, Item, LOOK, Part, Placed, Rect,
};
use edel::panel_edit::EDIT_LOCK_SCREEN;
use edel::settings::{LockScreen, Source};
use edel::tokens::{Colour, Tokens};
use edel::wallpapers::{self, Wallpaper};

use crate::files::Files;
use crate::style::Theme;
use crate::{icon, panels, preview, rows, widgets};

const INTRO: &str = n_("How the lock and login screens look.");

/// The keys this page chooses, and the one it reads for the wallpaper that
/// the blurred and sharp tiles show (`wallpaper.rs` chooses it).
const PARTS: &str = lock_screen::PARTS;
const NOTIFICATIONS: &str = "lock_screen.notifications";
const OWN_LOOKS: &str = "lock_screen.looks";
const WALLPAPER: &str = "appearance.wallpaper";

/// The background's tiles in the order people see them, by the names the
/// log gives them (M5.32b).
const TILE_NAMES: [&str; 6] = ["look", "navy", "blurred", "sharp", "colour", "picture"];

/// The look that applies while none is chosen, and the notifications'
/// values, as `edel::lock_screen` owns them.
use lock_screen::{CLASSIC, COUNT_HIDDEN as NONE, COUNT_SHOWN as COUNT};

/// The faces and date styles the segments show, in their order.
const FACES: [Face; 4] = [Face::Digital, Face::Stacked, Face::Analog, Face::Minimal];
const DATE_STYLES: [DateStyle; 3] = [DateStyle::Long, DateStyle::Short, DateStyle::Numeric];

/// The screen a look is drawn on, in logical pixels: 1280 by 800, the size
/// the mockups draw the lock screen at.
const SCREEN: (f32, f32) = (1280.0, 800.0);
/// The room kept from the screen's edges and between two parts.
const MARGIN: f32 = 24.0;
const GAP: f32 = 16.0;

/// A look's picture on its card in logical pixels: 16:10.
const CARD_W: i32 = 160;
const CARD_H: i32 = 100;
/// The picture of what applies, at the top of the page: 16:10 (M5.32b).
const HERO_W: i32 = 240;
const HERO_H: i32 = 150;
/// A background's tile in logical pixels: 16:10 too.
const TILE_W: i32 = 96;
const TILE_H: i32 = 60;
/// The blur of a wallpaper behind the glass: shell-ui blurs a picture 320
/// px wide with 12 px, so a 192 px picture gets 7 px.
const BLUR_RADIUS: f32 = 7.0;
/// The size a tile's picture is read at: twice the tile, so it stays sharp
/// on a screen of twice the density.
const TILE_READ: (u32, u32) = (192, 120);

/// What a look's picture draws: its parts and whether its background is the
/// navy scene. Shared with the picture, so a change is drawn in place.
type LookShown = (Vec<Placed>, bool);

/// Each part's size on the screen, width and height in logical pixels.
const DATE_SIZE: (f32, f32) = (220.0, 25.0);
const CLOCK_SIZE: (f32, f32) = (420.0, 180.0);
const COUNT_SIZE: (f32, f32) = (160.0, 36.0);
const PERSON_SIZE: (f32, f32) = (300.0, 200.0);
const PLAYER_SIZE: (f32, f32) = (400.0, 116.0);
const MESSAGE_SIZE: (f32, f32) = (300.0, 50.0);
const PICTURE_SIZE: (f32, f32) = (112.0, 112.0);
const BATTERY_SIZE: (f32, f32) = (80.0, 36.0);

/// A part's corner radius on the screen, which the card scales.
const PART_RADIUS: f64 = 12.0;
/// The alpha a part is drawn at on a card: the clock and the person a
/// little more than the rest.
const PART_ALPHA: f32 = 0.25;
const STRONG_ALPHA: f32 = 0.45;

/// The size a part stands at on the screen.
fn size_of(part: Part) -> (f32, f32) {
    match part {
        Part::Date => DATE_SIZE,
        Part::Clock => CLOCK_SIZE,
        Part::Count => COUNT_SIZE,
        Part::Person => PERSON_SIZE,
        Part::Player => PLAYER_SIZE,
        Part::Message => MESSAGE_SIZE,
        Part::Picture => PICTURE_SIZE,
        Part::Battery => BATTERY_SIZE,
    }
}

/// The layout items of `parts`, each at its anchor and offset, at its size.
fn items_of(parts: &[Placed]) -> Vec<Item> {
    parts
        .iter()
        .map(|p| {
            let (w, h) = size_of(p.part);
            Item {
                at: p.at,
                x: p.x as f32,
                y: p.y as f32,
                w,
                h,
            }
        })
        .collect()
}

/// Where each of `parts` stands on the screen, or None for one that stands
/// nowhere (the same function the lock uses, `edel::lock_screen::arrange`).
fn on_screen(parts: &[Placed]) -> Vec<Option<Rect>> {
    lock_screen::arrange(SCREEN, &items_of(parts), &[], GAP, MARGIN)
}

/// `parts` with each clock changed by `change`.
fn with_clock(parts: &[Placed], change: impl Fn(&mut Placed)) -> Vec<Placed> {
    parts
        .iter()
        .cloned()
        .map(|mut p| {
            if p.part == Part::Clock {
                change(&mut p);
            }
            p
        })
        .collect()
}

/// `parts` with the clock drawn as `face`; digital is the default, so it is
/// written as none.
pub fn clock_face(parts: &[Placed], face: Face) -> Vec<Placed> {
    with_clock(parts, |p| p.face = (face != Face::Digital).then_some(face))
}

/// `parts` with the clock at 12 hours when `twelve`, else at 24, which is
/// the default and so written as none.
pub fn clock_hours(parts: &[Placed], twelve: bool) -> Vec<Placed> {
    with_clock(parts, |p| p.hours = twelve.then_some(12))
}

/// `parts` with the date written as `style`; long is the default, so it is
/// written as none.
pub fn date_style(parts: &[Placed], style: DateStyle) -> Vec<Placed> {
    parts
        .iter()
        .cloned()
        .map(|mut p| {
            if p.part == Part::Date {
                p.style = (style != DateStyle::Long).then_some(style);
            }
            p
        })
        .collect()
}

/// The text `lock_screen.parts` takes, as `edel settings set` reads it: the
/// list of tables in one line, absent options left out.
fn parts_text(parts: &[Placed]) -> Result<String, String> {
    lock_screen::parts_value(parts)
}

/// The built-in and the person's own looks, each with the parts it places,
/// in the order people see them: the built-in ones, then the person's own
/// by name.
fn card_list(lock: &LockScreen) -> Vec<(String, Vec<Placed>)> {
    let built_in = lock_screen::LOOKS.iter().filter_map(|name| {
        lock_screen::look_parts(name, false).map(|parts| (name.to_string(), parts))
    });
    let own = lock
        .looks
        .iter()
        .flat_map(|looks| looks.iter())
        .filter(|(name, _)| !lock_screen::LOOKS.contains(&name.as_str()))
        .map(|(name, look)| (name.clone(), look.parts.clone()));
    built_in.chain(own).collect()
}

/// The title of what applies now: the person's own arrangement while
/// `lock_screen.parts` is set, else the look's name.
fn current_title(lock: &LockScreen) -> String {
    if lock.parts.is_some() {
        tr("Your own arrangement").to_string()
    } else {
        lock_screen::look_title(lock.look.as_deref().unwrap_or(CLASSIC))
    }
}

/// The title of a background tile, by its index in the order people see
/// them: the look's own, navy, blurred, sharp, a colour and a picture.
fn tile_title(index: usize) -> String {
    let title = match index {
        0 => tr("The look's own"),
        1 => tr("Navy"),
        2 => tr("Blurred"),
        3 => tr("Sharp"),
        4 => tr("Colour"),
        _ => tr("Picture"),
    };
    title.to_string()
}

/// The tile that shows what `background` chooses: none is the look's own
/// (the first), then navy, the wallpaper blurred and sharp, a colour and a
/// picture. A value that is not one of them is the look's own too.
fn choice_at(background: Option<&str>) -> usize {
    match background.and_then(lock_screen::parse_background) {
        None => 0,
        Some(Background::Backdrop) => 1,
        Some(Background::Blurred) => 2,
        Some(Background::Sharp) => 3,
        Some(Background::Colour(_)) => 4,
        Some(Background::Picture(_)) => 5,
    }
}

/// The background a look has when nothing is chosen, as the file writes it.
fn background_release(files: &Files) -> String {
    let look = files.lock().look.unwrap_or_else(|| CLASSIC.to_string());
    lock_screen::background_value(&lock_screen::look_background(&look))
}

/// Takes `key` out of the person's file, when they have it: nothing is
/// written when they have none, as writers never write a default.
fn reset_person(files: &Files, key: &str) -> Result<(), String> {
    match files.source(key) {
        Source::Person(_) => files.set(key, None),
        _ => Ok(()),
    }
}

/// Writes `parts` as the person's list of parts.
fn write_parts(parts: &[Placed]) -> Result<(), String> {
    Files::here().set(PARTS, Some(&parts_text(parts)?))
}

/// The parts that apply now, from the person's file over the machine's.
fn applied_parts() -> Vec<Placed> {
    lock_screen::applied(&Files::here().lock(), false).parts
}

/// The command that sets what applies for `key` on any machine, as Copy as
/// command gives it.
fn command_for(key: &str) -> String {
    let files = Files::here();
    let lock = files.lock();
    let applied = lock_screen::applied(&lock, false);
    let value = match key {
        PARTS => parts_text(&applied.parts).unwrap_or_default(),
        BACKGROUND => lock_screen::background_value(&applied.background),
        NOTIFICATIONS if applied.notifications => COUNT.to_string(),
        NOTIFICATIONS => NONE.to_string(),
        _ => lock.look.unwrap_or_else(|| CLASSIC.to_string()),
    };
    rows::command(&[(key, value)])
}

/// A row's line: what it does, then where its value comes from.
fn line(what: &str, note: Option<&str>) -> String {
    match note {
        Some(note) => format!("{what} · {note}"),
        None => what.to_string(),
    }
}

/// Sets a row's line, hidden when there is none.
fn set_line(row: &widgets::Row, text: &str) {
    row.subtitle.set_label(text);
    row.subtitle.set_visible(!text.is_empty());
}

/// Chooses `name` as the look: classic resets `lock_screen.look` (as it is
/// what applies without it), the others set it. Either way the person's own
/// arrangement of parts is taken out, as a look replaces it.
fn choose_look(name: &str) -> Result<(), String> {
    let files = Files::here();
    if name == CLASSIC {
        files.choose_over(LOOK, CLASSIC, Some(CLASSIC))?;
    } else {
        files.set(LOOK, Some(&format!("{name:?}")))?;
    }
    reset_person(&files, PARTS)
}

/// Saves what applies as a look called `name` (Save as): the person's own
/// looks with this one added, then the look chosen and the arrangement
/// taken out. The key's own check says what a name must be.
fn save_look(name: &str) -> Result<(), String> {
    let files = Files::here();
    let look = lock_screen::as_own_look(&files.lock());
    let own = files.person_file().lock_screen.looks;
    let text = lock_screen::looks_value(own.as_ref(), name, Some(look));
    files.set(OWN_LOOKS, text.as_deref())?;
    files.set(LOOK, Some(&format!("{name:?}")))?;
    reset_person(&files, PARTS)
}

/// A card of a look: its button, and the badge that shows it is chosen.
struct LookCard {
    name: String,
    button: gtk::ToggleButton,
    badge: gtk::Box,
}

/// A background's tile: its button and the check that shows it is chosen.
struct Tile {
    button: gtk::ToggleButton,
    badge: gtk::Box,
}

/// The page's widgets, so every change shows everywhere on it.
struct Ui {
    theme: Rc<Theme>,
    /// The card at the top (M5.32b): its name, and what its picture draws,
    /// redrawn in place when `current_view` is asked to.
    current_title: gtk::Label,
    current_shown: Rc<RefCell<LookShown>>,
    current_view: gtk::DrawingArea,
    /// The heading of the looks, which the page scrolls to when asked for.
    looks_head: gtk::Widget,
    looks: gtk::FlowBox,
    cards: RefCell<Vec<LookCard>>,
    /// The cards' names and parts as they were built, so they are built
    /// again only when a look changes.
    shown: RefCell<Vec<(String, Vec<Placed>)>>,
    look_source: widgets::Source,
    /// The heading of the background, the tiles' flow and the tiles.
    background_head: gtk::Widget,
    background_source: widgets::Source,
    background_flow: gtk::FlowBox,
    tiles: RefCell<Vec<Tile>>,
    /// What the tiles draw, as it was built, so they are built again only
    /// when it changes.
    tiles_shown: RefCell<String>,
    face: Rc<widgets::Segments>,
    face_row: widgets::Row,
    hours: gtk::Switch,
    hours_row: widgets::Row,
    date: Rc<widgets::Segments>,
    date_row: widgets::Row,
    count: gtk::Switch,
    count_row: widgets::Row,
    problem: gtk::Label,
    /// Set while the page itself moves a switch or a card, which is not a
    /// choice.
    quiet: Cell<bool>,
    /// Kept so the page follows the files while it is open.
    monitors: Vec<gio::FileMonitor>,
}

impl Ui {
    /// Shows what the files say now: the current look, the cards, the
    /// background, the clock, the date and the count, each with where its
    /// value comes from.
    fn update(self: &Rc<Self>) {
        let files = Files::here();
        let lock = files.lock();
        let applied = lock_screen::applied(&lock, false);
        self.quiet.set(true);
        self.current_title.set_label(&current_title(&lock));
        *self.current_shown.borrow_mut() = (
            applied.parts.clone(),
            applied.background == Background::Backdrop,
        );
        self.current_view.queue_draw();
        self.show_looks(&lock);
        let source = if lock.parts.is_some() {
            files.source(PARTS)
        } else {
            files.source(LOOK)
        };
        self.look_source
            .label
            .set_label(&rows::describe(&source, &current_title(&lock)));
        self.look_source.reset.set_visible(
            rows::resettable(&files.source(LOOK)) || rows::resettable(&files.source(PARTS)),
        );

        self.show_background(&lock);
        let background = files.source(BACKGROUND);
        let chosen = choice_at(lock.background.as_deref());
        self.background_source
            .label
            .set_label(&rows::describe(&background, &tile_title(chosen)));
        self.background_source
            .reset
            .set_visible(rows::resettable(&background));

        let clock = applied.parts.iter().find(|p| p.part == Part::Clock);
        let date = applied.parts.iter().find(|p| p.part == Part::Date);
        let face = clock.and_then(|c| c.face).unwrap_or(Face::Digital);
        self.face
            .set_selected(FACES.iter().position(|f| *f == face).unwrap_or(0));
        self.hours
            .set_active(clock.and_then(|c| c.hours) != Some(12));
        let style = date.and_then(|d| d.style).unwrap_or(DateStyle::Long);
        self.date
            .set_selected(DATE_STYLES.iter().position(|s| *s == style).unwrap_or(0));
        self.face.widget().set_sensitive(clock.is_some());
        self.hours.set_sensitive(clock.is_some());
        self.date.widget().set_sensitive(date.is_some());
        let parts = files.source(PARTS);
        let note = rows::note(&parts, false);
        set_line(&self.face_row, &line(tr("How the clock is drawn"), note));
        set_line(
            &self.hours_row,
            &line(tr("Off shows the hours from 1 to 12"), note),
        );
        set_line(&self.date_row, &line(tr("How the date is written"), note));
        for row in [&self.face_row, &self.hours_row, &self.date_row] {
            row.reset.set_visible(rows::resettable(&parts));
        }

        let count = files.source(NOTIFICATIONS);
        self.count.set_active(applied.notifications);
        set_line(
            &self.count_row,
            &line(
                tr("Off hides the number of notifications that wait"),
                rows::note(&count, false),
            ),
        );
        self.count_row.reset.set_visible(rows::resettable(&count));
        self.quiet.set(false);
        self.problem.set_visible(false);
    }

    /// Shows the cards: built again when a look changes, and the one that
    /// applies chosen. Nothing is chosen while the person's own arrangement
    /// is set.
    fn show_looks(self: &Rc<Self>, lock: &LockScreen) {
        let list = card_list(lock);
        if *self.shown.borrow() != list {
            self.build_cards(&list);
        }
        let chosen = match lock.parts {
            Some(_) => None,
            None => Some(lock.look.as_deref().unwrap_or(CLASSIC)),
        };
        for card in self.cards.borrow().iter() {
            let on = chosen == Some(card.name.as_str());
            card.button.set_active(on);
            card.badge.set_visible(on);
        }
    }

    /// Builds the cards of `list` in the flow, each a picture of its parts
    /// with its name, and wires each to a choice.
    fn build_cards(self: &Rc<Self>, list: &[(String, Vec<Placed>)]) {
        while let Some(child) = self.looks.first_child() {
            self.looks.remove(&child);
        }
        let mut cards = Vec::new();
        let mut first: Option<gtk::ToggleButton> = None;
        for (name, parts) in list {
            let backdrop = lock_screen::look_background(name) == Background::Backdrop;
            let shown = Rc::new(RefCell::new((parts.clone(), backdrop)));
            let picture = look_picture(&self.theme, &shown, CARD_W, CARD_H);
            let (button, badge) =
                widgets::picture_card(&lock_screen::look_title(name), &picture.upcast());
            match &first {
                Some(first) => button.set_group(Some(first)),
                None => first = Some(button.clone()),
            }
            self.looks.append(&button);
            // The card takes the focus, not the box the flow wraps it in.
            if let Some(child) = button.parent() {
                child.set_focusable(false);
            }
            let weak = Rc::downgrade(self);
            let chosen = name.clone();
            button.connect_toggled(move |button| {
                if !button.is_active() {
                    return;
                }
                let Some(ui) = weak.upgrade() else {
                    return;
                };
                if !ui.quiet.get() {
                    ui.report(choose_look(&chosen));
                    ui.update();
                }
            });
            cards.push(LookCard {
                name: name.clone(),
                button,
                badge,
            });
        }
        *self.cards.borrow_mut() = cards;
        *self.shown.borrow_mut() = list.to_vec();
    }

    /// Shows the background's tiles (M5.32b): built again only when what
    /// they draw changes (the look, the wallpaper that applies and the
    /// background the file names), and the one that applies chosen.
    fn show_background(self: &Rc<Self>, lock: &LockScreen) {
        let look = lock.look.clone().unwrap_or_else(|| CLASSIC.to_string());
        let dark = adw::StyleManager::default().is_dark();
        let wallpaper =
            match wallpapers::on_this_machine(Files::here().chosen(WALLPAPER).as_deref(), dark) {
                Wallpaper::Picture(path) => Some(path),
                _ => None,
            };
        let signature = format!(
            "{:?}",
            (
                lock_screen::look_background(&look),
                &wallpaper,
                &lock.background,
                dark
            )
        );
        if *self.tiles_shown.borrow() != signature {
            self.build_tiles(lock, &look, wallpaper.as_deref());
            *self.tiles_shown.borrow_mut() = signature;
        }
        let chosen = choice_at(lock.background.as_deref());
        for (index, tile) in self.tiles.borrow().iter().enumerate() {
            tile.button.set_active(index == chosen);
            tile.badge.set_visible(index == chosen);
        }
    }

    /// Builds the six tiles in the flow, each wired to its choice.
    fn build_tiles(self: &Rc<Self>, lock: &LockScreen, look: &str, wallpaper: Option<&Path>) {
        while let Some(child) = self.background_flow.first_child() {
            self.background_flow.remove(&child);
        }
        let backdrop = lock_screen::look_background(look) == Background::Backdrop;
        let background = lock_screen::applied(lock, false).background;
        let mut tiles = Vec::new();
        let mut first: Option<gtk::ToggleButton> = None;
        for index in 0..TILE_NAMES.len() {
            let picture = background_tile(&self.theme, index, backdrop, wallpaper, &background);
            let (button, badge) = widgets::picture_card(&tile_title(index), &picture);
            match &first {
                Some(first) => button.set_group(Some(first)),
                None => first = Some(button.clone()),
            }
            self.background_flow.append(&button);
            // The card takes the focus, not the box the flow wraps it in.
            if let Some(child) = button.parent() {
                child.set_focusable(false);
            }
            // A click always asks, so Colour and Picture open their dialogs
            // again when they are the one chosen.
            let weak = Rc::downgrade(self);
            button.connect_clicked(move |_| {
                if let Some(ui) = weak.upgrade() {
                    ui.pick_background(index);
                }
            });
            tiles.push(Tile { button, badge });
        }
        *self.tiles.borrow_mut() = tiles;
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

    /// Chooses the background segment `index` (M5.10c5c): the look's own
    /// takes the key out, navy and the wallpapers write their word, and a
    /// colour or a picture opens the same dialog the Wallpaper card does.
    fn pick_background(self: &Rc<Self>, index: usize) {
        match index {
            0 => {
                let result = reset_person(&Files::here(), BACKGROUND);
                self.report(result);
                self.update();
            }
            1 => self.set_background(&lock_screen::background_value(&Background::Backdrop)),
            2 => self.set_background(&lock_screen::background_value(&Background::Blurred)),
            3 => self.set_background(&lock_screen::background_value(&Background::Sharp)),
            4 => {
                let start = match lock_screen::parse_background(
                    &Files::here().lock().background.unwrap_or_default(),
                ) {
                    Some(Background::Colour(colour)) => colour,
                    _ => self.theme.tokens().accent,
                };
                let weak = Rc::downgrade(self);
                let near = self.background_flow.clone().upcast::<gtk::Widget>();
                widgets::choose_colour(&near, start, move |picked| {
                    let Some(ui) = weak.upgrade() else {
                        return;
                    };
                    if let Some(colour) = picked.and_then(|hex| Colour::parse(&hex)) {
                        let value = lock_screen::background_value(&Background::Colour(colour));
                        ui.report(write_background(&value));
                    }
                    ui.update();
                });
            }
            _ => {
                let weak = Rc::downgrade(self);
                let near = self.background_flow.clone().upcast::<gtk::Widget>();
                widgets::choose_pictures(&near, move |picked| {
                    let Some(ui) = weak.upgrade() else {
                        return;
                    };
                    let result = match picked.as_deref() {
                        None | Some([]) => Ok(()),
                        Some([one]) if one.to_str().is_some() => {
                            let value =
                                lock_screen::background_value(&Background::Picture(one.clone()));
                            write_background(&value)
                        }
                        Some([_]) => Err(tr("Choose one picture").to_string()),
                        Some(_) => Err(tr("Choose one picture").to_string()),
                    };
                    ui.report(result);
                    ui.update();
                });
            }
        }
    }

    /// Writes `value` as the background, and shows the page again.
    fn set_background(self: &Rc<Self>, value: &str) {
        let result = write_background(value);
        self.report(result);
        self.update();
    }
}

/// Writes `value` as the background, taken out again when it is what the
/// look gives without it.
fn write_background(value: &str) -> Result<(), String> {
    let files = Files::here();
    files.choose_over(BACKGROUND, value, Some(&background_release(&files)))
}

/// A look's picture of `w` by `h` logical pixels: its parts on the screen,
/// drawn as the lock draws them, over the navy scene for classic and the
/// mid grey-blue for the rest (the wallpaper behind them, blurred), in the
/// tokens' colours. It draws `shown` as it is each time, so a change to it
/// is drawn in place.
fn look_picture(
    theme: &Rc<Theme>,
    shown: &Rc<RefCell<LookShown>>,
    w: i32,
    h: i32,
) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::builder()
        .content_width(w)
        .content_height(h)
        .build();
    let drawn = theme.clone();
    let shown = shown.clone();
    area.set_draw_func(move |_, cr, w, h| {
        let t = drawn.tokens();
        let guard = shown.borrow();
        let (parts, backdrop) = &*guard;
        draw_look(cr, f64::from(w), f64::from(h), parts, *backdrop, &t);
    });
    let weak = area.downgrade();
    theme.watch(move || {
        if let Some(area) = weak.upgrade() {
            area.queue_draw();
        }
    });
    area
}

/// Draws a look on a `w` by `h` card: the background, then each part as a
/// rounded rectangle in the backdrop text's colour at a quarter alpha, the
/// clock and the person at 45 percent (`edel::lock_screen::arrange`'s
/// places, scaled to the card).
fn draw_look(cr: &Context, w: f64, h: f64, parts: &[Placed], backdrop: bool, t: &Tokens) {
    let scale = w / f64::from(SCREEN.0);
    clip(cr, w, h, f64::from(t.radius_window) * scale);
    if backdrop {
        gradient(cr, h, t.lock_backdrop, t.lock_backdrop_deep);
    } else {
        preview::set(cr, t.backdrop);
        let _ = cr.paint();
    }
    for (part, rect) in parts.iter().zip(on_screen(parts)) {
        let Some(r) = rect else {
            continue;
        };
        let alpha = match part.part {
            Part::Clock | Part::Person => STRONG_ALPHA,
            _ => PART_ALPHA,
        };
        preview::set(
            cr,
            Colour {
                a: alpha,
                ..t.backdrop_text
            },
        );
        preview::rounded(
            cr,
            f64::from(r.x) * scale,
            f64::from(r.y) * scale,
            f64::from(r.w) * scale,
            f64::from(r.h) * scale,
            PART_RADIUS * scale,
        );
        let _ = cr.fill();
    }
}

/// Clips `cr` to a `w` by `h` rectangle with corners of `radius`.
fn clip(cr: &Context, w: f64, h: f64, radius: f64) {
    preview::rounded(cr, 0.0, 0.0, w, h, radius);
    cr.clip();
}

/// Paints the current clip with a gradient from `top` at the top edge to
/// `deep` at the height `h`.
fn gradient(cr: &Context, h: f64, top: Colour, deep: Colour) {
    let gradient = LinearGradient::new(0.0, 0.0, 0.0, h);
    gradient.add_color_stop_rgba(
        0.0,
        f64::from(top.r),
        f64::from(top.g),
        f64::from(top.b),
        1.0,
    );
    gradient.add_color_stop_rgba(
        1.0,
        f64::from(deep.r),
        f64::from(deep.g),
        f64::from(deep.b),
        1.0,
    );
    let _ = cr.set_source(&gradient);
    let _ = cr.paint();
}

/// The tile of background `index` (M5.32b): the look's own and navy as the
/// lock draws them; blurred and sharp show the wallpaper that applies, else
/// the gradient; Colour the colour the file names, else the accent; Picture
/// the picture the file names, else the picture icon on the card.
fn background_tile(
    theme: &Rc<Theme>,
    index: usize,
    backdrop: bool,
    wallpaper: Option<&Path>,
    background: &Background,
) -> gtk::Widget {
    match index {
        0 => drawn_tile(theme, move |cr, w, h, t| {
            draw_look(cr, w, h, &[], backdrop, t)
        }),
        1 => drawn_tile(theme, |cr, w, h, t| {
            clip(cr, w, h, f64::from(t.radius_small));
            gradient(cr, h, t.lock_backdrop, t.lock_backdrop_deep);
        }),
        2 | 3 => match wallpaper.and_then(|path| picture_texture(path, index == 2)) {
            Some(texture) => texture_tile(&texture),
            None => drawn_tile(theme, |cr, w, h, t| {
                clip(cr, w, h, f64::from(t.radius_small));
                gradient(cr, h, t.backdrop, t.backdrop_deep);
            }),
        },
        4 => {
            let named = match background {
                Background::Colour(colour) => Some(*colour),
                _ => None,
            };
            drawn_tile(theme, move |cr, w, h, t| {
                clip(cr, w, h, f64::from(t.radius_small));
                preview::set(cr, named.unwrap_or(t.accent));
                let _ = cr.paint();
            })
        }
        _ => match background {
            Background::Picture(path) => match picture_texture(path, false) {
                Some(texture) => texture_tile(&texture),
                None => blank_tile(),
            },
            _ => blank_tile(),
        },
    }
}

/// A tile drawn by `draw` in the tokens' colours, drawn again when they
/// change.
fn drawn_tile(
    theme: &Rc<Theme>,
    draw: impl Fn(&Context, f64, f64, &Tokens) + 'static,
) -> gtk::Widget {
    let area = gtk::DrawingArea::builder()
        .content_width(TILE_W)
        .content_height(TILE_H)
        .css_classes(["edel-wallpaper-thumb"])
        .build();
    let drawn = theme.clone();
    area.set_draw_func(move |_, cr, w, h| {
        let t = drawn.tokens();
        draw(cr, f64::from(w), f64::from(h), &t);
    });
    let weak = area.downgrade();
    theme.watch(move || {
        if let Some(area) = weak.upgrade() {
            area.queue_draw();
        }
    });
    area.upcast()
}

/// A picture as a tile: the texture cropped to the tile's size.
fn texture_tile(texture: &gdk::Texture) -> gtk::Widget {
    let picture = gtk::Picture::for_paintable(texture);
    picture.set_content_fit(gtk::ContentFit::Cover);
    picture.set_can_shrink(false);
    picture.set_size_request(TILE_W, TILE_H);
    picture.set_overflow(gtk::Overflow::Hidden);
    picture.add_css_class("edel-wallpaper-thumb");
    picture.upcast()
}

/// The picture at `path` as a texture the tile's size, cropped to fill it,
/// blurred when `blur` as the lock blurs its wallpaper. None when it cannot
/// be read, and the tile then shows what it shows without a picture.
fn picture_texture(path: &Path, blur: bool) -> Option<gdk::Texture> {
    let mut pixmap = edel::pictures::load(path, TILE_READ).ok()?;
    if blur {
        edel::pictures::blur(&mut pixmap, BLUR_RADIUS);
    }
    let (w, h) = (pixmap.width(), pixmap.height());
    let bytes = glib::Bytes::from(pixmap.data());
    Some(
        gdk::MemoryTexture::new(
            w as i32,
            h as i32,
            gdk::MemoryFormat::R8g8b8a8Premultiplied,
            &bytes,
            w as usize * 4,
        )
        .upcast(),
    )
}

/// A tile with no picture: the picture icon centred on the card's colour.
fn blank_tile() -> gtk::Widget {
    let tile = gtk::Box::builder()
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .css_classes(["edel-tile-blank"])
        .build();
    tile.set_size_request(TILE_W, TILE_H);
    tile.append(&icon::image("picture", 24));
    tile.upcast()
}

/// The Lock screen page (M5.10c5c, M5.32b): what applies at the top, then
/// the looks, the background, the clock and the date, and the count of
/// notifications, each row with its Reset, Copy as command and where its
/// value comes from.
pub fn page(theme: &Rc<Theme>) -> gtk::Widget {
    let files = Files::here();
    let (page, content, problem) = widgets::page(tr("Lock screen"), tr(INTRO));

    // What applies now (M5.32b): its picture, its name, a line, and Edit
    // lock screen beside the quiet Save as. Narrow, the picture stacks on
    // top at full width (the aspect frame keeps its 16:10 shape).
    let shown: Rc<RefCell<LookShown>> = Rc::new(RefCell::new((Vec::new(), false)));
    let picture = look_picture(theme, &shown, HERO_W, HERO_H);
    let frame = gtk::AspectFrame::builder()
        .ratio(16.0 / 10.0)
        .obey_child(false)
        .child(&picture)
        .build();
    let title = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .css_classes(["edel-current-title"])
        .build();
    let line = gtk::Label::builder()
        .label(tr(
            "Edit it where it is: move, add or take away parts on the lock itself.",
        ))
        .xalign(0.0)
        .wrap(true)
        .css_classes(["edel-intro"])
        .build();
    let edit = widgets::action(tr("Edit lock screen"), true);
    let save = widgets::action(tr("Save as"), false);
    for button in [&edit, &save] {
        button.add_css_class("edel-current-button");
    }
    let buttons = gtk::Box::builder()
        .spacing(10)
        .halign(gtk::Align::Start)
        .margin_top(12)
        .build();
    buttons.append(&edit);
    buttons.append(&save);
    let words = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build();
    words.append(&title);
    words.append(&line);
    words.append(&buttons);
    let current = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(20)
        .hexpand(true)
        .css_classes(["edel-group", "edel-current"])
        .build();
    current.append(&frame);
    current.append(&words);
    let bin = adw::BreakpointBin::builder()
        .hexpand(true)
        .child(&current)
        .build();
    // The page's narrow breakpoint, as the window's: the card is what it
    // measures, and at that width it stacks.
    if let Ok(condition) = adw::BreakpointCondition::parse(crate::NARROW) {
        let narrow = adw::Breakpoint::new(condition);
        narrow.add_setter(
            &current,
            "orientation",
            Some(&gtk::Orientation::Vertical.to_value()),
        );
        bin.add_breakpoint(narrow);
    }
    content.append(&bin);

    let looks = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .homogeneous(true)
        .min_children_per_line(1)
        .max_children_per_line(3)
        .column_spacing(12)
        .row_spacing(12)
        .build();
    let look_source = widgets::section(&content, tr("Looks"));
    let looks_head = content
        .last_child()
        .unwrap_or_else(|| looks.clone().upcast());
    content.append(&looks);

    // The background's tiles (M5.32b): six pictures, wrapping to the width.
    let tiles = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .homogeneous(true)
        .min_children_per_line(1)
        .max_children_per_line(6)
        .column_spacing(12)
        .row_spacing(12)
        .build();
    let background_source = widgets::section(&content, tr("Background"));
    let background_head = content
        .last_child()
        .unwrap_or_else(|| tiles.clone().upcast());
    content.append(&tiles);

    widgets::heading(&content, tr("Clock and date"));
    let clock_group = widgets::group(&content);
    let face = widgets::Segments::new(&[
        tr("Digital").to_string(),
        tr("Stacked").to_string(),
        tr("Analog").to_string(),
        tr("Minimal").to_string(),
    ]);
    let face_row = widgets::row(&clock_group, tr("Face"), &face.widget());
    let hours = gtk::Switch::new();
    let hours_row = widgets::row(&clock_group, tr("24-hour clock"), &hours.clone().upcast());
    let date = widgets::Segments::new(&[
        tr("Long").to_string(),
        tr("Short").to_string(),
        tr("Numeric").to_string(),
    ]);
    let date_row = widgets::row(&clock_group, tr("Date"), &date.widget());

    let count_group = widgets::group(&content);
    count_group.set_margin_top(20);
    let count = gtk::Switch::new();
    let count_row = widgets::row(
        &count_group,
        tr("Unread notifications count"),
        &count.clone().upcast(),
    );

    let mut monitors = Vec::new();
    for path in std::iter::once(&files.machine).chain(files.person.as_ref()) {
        let file = gio::File::for_path(path);
        if let Ok(monitor) =
            file.monitor_file(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
        {
            monitors.push(monitor);
        }
    }
    let ui = Rc::new(Ui {
        theme: theme.clone(),
        current_title: title,
        current_shown: shown,
        current_view: picture,
        looks_head,
        looks: looks.clone(),
        cards: RefCell::new(Vec::new()),
        shown: RefCell::new(Vec::new()),
        look_source,
        background_head,
        background_source,
        background_flow: tiles.clone(),
        tiles: RefCell::new(Vec::new()),
        tiles_shown: RefCell::new(String::new()),
        face: face.clone(),
        face_row,
        hours: hours.clone(),
        hours_row,
        date: date.clone(),
        date_row,
        count: count.clone(),
        count_row,
        problem,
        quiet: Cell::new(false),
        monitors,
    });
    ui.update();
    for monitor in &ui.monitors {
        let weak = Rc::downgrade(&ui);
        monitor.connect_changed(move |_, _, _, _| {
            if let Some(ui) = weak.upgrade() {
                ui.update();
            }
        });
    }
    wire(&ui, (&save, &edit), &face, &date, &hours, &count);
    log_places(&looks, &ui);

    // The page's state lives as long as the page does.
    let keep = ui.clone();
    page.connect_destroy(move |_| {
        let _ = &keep;
    });
    page
}

/// Connects the page's controls and the Reset and Copy buttons of its rows
/// and of the cards' section.
fn wire(
    ui: &Rc<Ui>,
    (save, edit): (&gtk::Button, &gtk::Button),
    face: &Rc<widgets::Segments>,
    date: &Rc<widgets::Segments>,
    hours: &gtk::Switch,
    count: &gtk::Switch,
) {
    // Edit lock screen asks shell-ui for its edit mode over the session bus,
    // as the Panels group asks for the panel editor (the same call, given the
    // method's name); the page writes nothing itself.
    let weak = Rc::downgrade(ui);
    edit.connect_clicked(move |_| {
        let weak = weak.clone();
        glib::spawn_future_local(async move {
            let asked = gio::spawn_blocking(|| panels::ask_editor(EDIT_LOCK_SCREEN))
                .await
                .unwrap_or_else(|_| Err(tr("the call to the desktop stopped").to_string()));
            let Some(ui) = weak.upgrade() else {
                return;
            };
            match asked {
                Ok(()) => {
                    eprintln!("edel-settings: asked the lock screen to be edited");
                    ui.report(Ok(()));
                }
                Err(why) => ui.report(Err(trf(
                    "The desktop's lock screen is not running, so it cannot be edited now:\n{why}",
                    &[("why", &why)],
                ))),
            }
        });
    });

    let weak = Rc::downgrade(ui);
    save.connect_clicked(move |button| {
        if let Some(ui) = weak.upgrade() {
            ask_name(&ui, button.upcast_ref());
        }
    });

    let weak = Rc::downgrade(ui);
    face.connect_changed(move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if !ui.quiet.get() {
            let parts = clock_face(&applied_parts(), FACES[ui.face.selected()]);
            ui.report(write_parts(&parts));
            ui.update();
        }
    });
    let weak = Rc::downgrade(ui);
    hours.connect_active_notify(move |switch| {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if !ui.quiet.get() {
            let parts = clock_hours(&applied_parts(), !switch.is_active());
            ui.report(write_parts(&parts));
            ui.update();
        }
    });
    let weak = Rc::downgrade(ui);
    date.connect_changed(move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if !ui.quiet.get() {
            let parts = date_style(&applied_parts(), DATE_STYLES[ui.date.selected()]);
            ui.report(write_parts(&parts));
            ui.update();
        }
    });
    let weak = Rc::downgrade(ui);
    count.connect_active_notify(move |switch| {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if !ui.quiet.get() {
            let value = if switch.is_active() { COUNT } else { NONE };
            let result = Files::here().choose_over(NOTIFICATIONS, value, Some(COUNT));
            ui.report(result);
            ui.update();
        }
    });

    // Reset: the look's own arrangement and look, the background, the
    // option's default in the person's list, or the count.
    let weak = Rc::downgrade(ui);
    ui.look_source.reset.connect_clicked(move |_| {
        if let Some(ui) = weak.upgrade() {
            let files = Files::here();
            let result = reset_person(&files, LOOK).and_then(|()| reset_person(&files, PARTS));
            ui.report(result);
            ui.update();
        }
    });
    let weak = Rc::downgrade(ui);
    ui.background_source.reset.connect_clicked(move |_| {
        if let Some(ui) = weak.upgrade() {
            ui.report(reset_person(&Files::here(), BACKGROUND));
            ui.update();
        }
    });
    let weak = Rc::downgrade(ui);
    ui.face_row.reset.connect_clicked(move |_| {
        if let Some(ui) = weak.upgrade() {
            ui.report(write_parts(&clock_face(&applied_parts(), Face::Digital)));
            ui.update();
        }
    });
    let weak = Rc::downgrade(ui);
    ui.hours_row.reset.connect_clicked(move |_| {
        if let Some(ui) = weak.upgrade() {
            ui.report(write_parts(&clock_hours(&applied_parts(), false)));
            ui.update();
        }
    });
    let weak = Rc::downgrade(ui);
    ui.date_row.reset.connect_clicked(move |_| {
        if let Some(ui) = weak.upgrade() {
            ui.report(write_parts(&date_style(&applied_parts(), DateStyle::Long)));
            ui.update();
        }
    });
    let weak = Rc::downgrade(ui);
    ui.count_row.reset.connect_clicked(move |_| {
        if let Some(ui) = weak.upgrade() {
            ui.report(reset_person(&Files::here(), NOTIFICATIONS));
            ui.update();
        }
    });

    // Copy as command: each row's line for what applies now.
    copy(&ui.look_source.copy, || {
        let files = Files::here();
        if files.lock().parts.is_some() {
            command_for(PARTS)
        } else {
            command_for(LOOK)
        }
    });
    copy(&ui.background_source.copy, || command_for(BACKGROUND));
    for row in [&ui.face_row, &ui.hours_row, &ui.date_row] {
        copy(&row.copy, || command_for(PARTS));
    }
    copy(&ui.count_row.copy, || command_for(NOTIFICATIONS));
}

/// Makes `button` copy the line `text` gives when it is clicked.
fn copy(button: &gtk::Button, text: impl Fn() -> String + 'static) {
    button.connect_clicked(move |button| {
        button.clipboard().set_text(&text());
        widgets::copied(button);
    });
}

/// Asks for the name of a look to save (Save as), in a small dialog. The
/// name is checked by the settings file's own rule for `lock_screen.looks`,
/// whose message shows under the entry; a good name closes the dialog.
fn ask_name(ui: &Rc<Ui>, near: &gtk::Widget) {
    let window = gtk::Window::builder()
        .title(tr("Save as"))
        .modal(true)
        .resizable(false)
        .default_width(360)
        .build();
    if let Some(parent) = near.root().and_downcast::<gtk::Window>() {
        window.set_transient_for(Some(&parent));
    }
    let entry = gtk::Entry::builder()
        .placeholder_text(tr("Name, such as home"))
        .activates_default(true)
        .build();
    entry.update_property(&[gtk::accessible::Property::Label(tr("Name of the look"))]);
    let error = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .visible(false)
        .css_classes(["edel-error"])
        .build();
    let text = gtk::Label::builder()
        .label(tr(
            "Name this look. It keeps the parts you see now and shows in the cards above.",
        ))
        .xalign(0.0)
        .wrap(true)
        .build();
    let cancel = widgets::action(tr("Cancel"), false);
    let save = widgets::action(tr("Save"), true);
    let buttons = gtk::Box::builder()
        .spacing(8)
        .halign(gtk::Align::End)
        .build();
    buttons.append(&cancel);
    buttons.append(&save);
    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(20)
        .margin_bottom(20)
        .margin_start(20)
        .margin_end(20)
        .build();
    body.append(&text);
    body.append(&entry);
    body.append(&error);
    body.append(&buttons);
    window.set_child(Some(&body));

    let close = window.clone();
    cancel.connect_clicked(move |_| close.close());
    let weak = Rc::downgrade(ui);
    let (typed, shown, dialog) = (entry.clone(), error.clone(), window.clone());
    save.connect_clicked(move |_| {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        match save_look(typed.text().as_str()) {
            Ok(()) => {
                dialog.close();
                ui.update();
            }
            Err(why) => {
                shown.set_label(&why);
                shown.set_visible(true);
            }
        }
    });
    let save_again = save.clone();
    entry.connect_activate(move |_| save_again.emit_clicked());
    window.present();
    entry.grab_focus();
}

/// Logs where the looks' cards and the section's Reset, and the background's
/// tiles, lie in the window, once the page is laid out, for CI to click
/// (logical pixels), as `edel-settings: lock_screen group places classic
/// X+Y+WxH magazine X+Y+WxH minimal X+Y+WxH, reset X+Y+WxH` and `edel-settings:
/// lock background places look X+Y+WxH navy X+Y+WxH ...`. Asked for by
/// `lock_screen.look` or `lock_screen.background`, the page first scrolls that
/// section to the top and gives the keyboard to its first card or the chosen
/// tile (M5.32b). It logs once two frames agree, after the scroll.
fn log_places(looks: &gtk::FlowBox, ui: &Rc<Ui>) {
    let weak = Rc::downgrade(ui);
    let last = RefCell::new(String::new());
    let scrolled = Cell::new(false);
    looks.add_tick_callback(move |flow, _| {
        let Some(window) = flow.root().and_downcast::<gtk::Window>() else {
            return glib::ControlFlow::Continue;
        };
        if !flow.is_mapped() || flow.width() == 0 {
            return glib::ControlFlow::Continue;
        }
        let Some(ui) = weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        let cards = ui.cards.borrow();
        if cards.is_empty() {
            return glib::ControlFlow::Continue;
        }
        if !scrolled.replace(true) {
            ask_for(&ui, &cards, flow.upcast_ref());
            return glib::ControlFlow::Continue;
        }
        let places: Vec<String> = cards
            .iter()
            .map(|card| {
                format!(
                    "{} {}",
                    card.name,
                    widgets::place(card.button.compute_bounds(&window))
                )
            })
            .collect();
        let looks_line = format!(
            "edel-settings: lock_screen group places {}, reset {}",
            places.join(" "),
            widgets::place(ui.look_source.reset.compute_bounds(&window)),
        );
        let tiles = ui.tiles.borrow();
        let tile_places: Vec<String> = tiles
            .iter()
            .zip(TILE_NAMES)
            .map(|(tile, name)| {
                format!(
                    "{name} {}",
                    widgets::place(tile.button.compute_bounds(&window))
                )
            })
            .collect();
        let background_line = format!(
            "edel-settings: lock background places {}",
            tile_places.join(" ")
        );
        let both = format!("{looks_line}\n{background_line}");
        // The view settles over a few frames, so the places are logged only
        // once two frames agree.
        if last.replace(both.clone()) != both {
            return glib::ControlFlow::Continue;
        }
        eprintln!("{looks_line}");
        eprintln!("{background_line}");
        glib::ControlFlow::Break
    });
}

/// Scrolls the section `--page` asked for to the top and gives the keyboard
/// to its control, once the page is laid out (M5.32b); a page not asked for
/// by one of these keys is left as it is.
fn ask_for(ui: &Ui, cards: &[LookCard], flow: &gtk::Widget) {
    // The page is the one asked for by name; a page opened from the sidebar
    // has no such class and keeps the keyboard where it is.
    let Some(page) = std::iter::successors(Some(flow.clone()), gtk::Widget::parent)
        .find(|w| w.has_css_class(widgets::ASKED))
    else {
        return;
    };
    match widgets::asked_row() {
        Some(LOOK) => {
            widgets::scroll_to_top(&ui.looks_head);
            if let Some(first) = cards.first() {
                widgets::take_asked(&page, &first.button);
            }
        }
        Some(BACKGROUND) => {
            widgets::scroll_to_top(&ui.background_head);
            let tiles = ui.tiles.borrow();
            if let Some(chosen) = tiles.iter().find(|tile| tile.button.is_active()) {
                widgets::take_asked(&page, &chosen.button);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every built-in look's parts stand on the card's screen at the sizes
    /// the page draws them at.
    #[test]
    fn every_built_in_look_places_each_part_on_the_card_screen() {
        for name in lock_screen::LOOKS {
            let parts = lock_screen::look_parts(name, false).expect(name);
            let items = items_of(&parts);
            for (part, item) in parts.iter().zip(&items) {
                assert_eq!((item.w, item.h), size_of(part.part), "{name}");
            }
            let placed = on_screen(&parts);
            assert!(placed.iter().all(Option::is_some), "{name}: {placed:?}");
        }
    }

    #[test]
    fn the_part_sizes_are_the_card_screens_sizes() {
        assert_eq!(size_of(Part::Date), (220.0, 25.0));
        assert_eq!(size_of(Part::Clock), (420.0, 180.0));
        assert_eq!(size_of(Part::Count), (160.0, 36.0));
        assert_eq!(size_of(Part::Person), (300.0, 200.0));
        assert_eq!(size_of(Part::Player), (400.0, 116.0));
        assert_eq!(size_of(Part::Message), (300.0, 50.0));
        assert_eq!(size_of(Part::Picture), (112.0, 112.0));
        assert_eq!(size_of(Part::Battery), (80.0, 36.0));
        assert_eq!(CARD_W * 10, CARD_H * 16);
    }

    #[test]
    fn the_clock_face_is_analog_then_none_for_digital() {
        let classic = lock_screen::classic(false);
        let analog = clock_face(&classic, Face::Analog);
        let clock = analog.iter().find(|p| p.part == Part::Clock).unwrap();
        assert_eq!(clock.face, Some(Face::Analog));
        let digital = clock_face(&analog, Face::Digital);
        let clock = digital.iter().find(|p| p.part == Part::Clock).unwrap();
        assert_eq!(clock.face, None);
        // Nothing else changes.
        assert_eq!(digital.len(), classic.len());
        assert_eq!(digital[0], classic[0]);
    }

    #[test]
    fn the_clock_hours_are_twelve_or_none_for_twenty_four() {
        let classic = lock_screen::classic(false);
        let twelve = clock_hours(&classic, true);
        assert_eq!(
            twelve.iter().find(|p| p.part == Part::Clock).unwrap().hours,
            Some(12)
        );
        let twenty_four = clock_hours(&twelve, false);
        assert_eq!(
            twenty_four
                .iter()
                .find(|p| p.part == Part::Clock)
                .unwrap()
                .hours,
            None
        );
    }

    #[test]
    fn the_date_is_long_then_none_for_long() {
        let classic = lock_screen::classic(false);
        let short = date_style(&classic, DateStyle::Short);
        assert_eq!(
            short.iter().find(|p| p.part == Part::Date).unwrap().style,
            Some(DateStyle::Short)
        );
        let long = date_style(&short, DateStyle::Long);
        assert_eq!(
            long.iter().find(|p| p.part == Part::Date).unwrap().style,
            None
        );
    }

    #[test]
    fn the_parts_written_read_back_to_the_same_parts() {
        for name in lock_screen::LOOKS {
            let parts = clock_hours(
                &clock_face(&lock_screen::look_parts(name, false).unwrap(), Face::Analog),
                true,
            );
            let text = parts_text(&parts).unwrap();
            let set = edel::settings::set("format = 1\n", PARTS, &text).unwrap();
            let read = edel::settings::read(&set).unwrap();
            assert!(read.problems.is_empty(), "{name}: {:?}", read.problems);
            assert_eq!(read.file.lock_screen.parts, Some(parts), "{name}");
        }
    }

    #[test]
    fn a_saved_look_reads_back_to_the_same_parts() {
        let parts = lock_screen::classic(false);
        let own: std::collections::BTreeMap<String, edel::settings::OwnLook> = [(
            "home".to_string(),
            edel::settings::OwnLook {
                parts: parts.clone(),
                background: None,
            },
        )]
        .into_iter()
        .collect();
        let text = toml::Value::try_from(&own).unwrap().to_string();
        let set = edel::settings::set("format = 1\n", OWN_LOOKS, &text).unwrap();
        let read = edel::settings::read(&set).unwrap();
        assert!(read.problems.is_empty(), "{:?}", read.problems);
        let looks = read.file.lock_screen.looks.unwrap();
        assert_eq!(looks["home"].parts, parts);
        // The key's own check says what a name must be.
        let bad: std::collections::BTreeMap<String, edel::settings::OwnLook> = [(
            "my look".to_string(),
            edel::settings::OwnLook {
                parts,
                background: None,
            },
        )]
        .into_iter()
        .collect();
        let text = toml::Value::try_from(&bad).unwrap().to_string();
        let error = edel::settings::set("format = 1\n", OWN_LOOKS, &text).unwrap_err();
        assert!(
            error.to_string().contains("is not a look's name"),
            "{error}"
        );
    }

    #[test]
    fn the_background_segment_is_the_one_that_applies() {
        assert_eq!(choice_at(None), 0);
        assert_eq!(choice_at(Some("backdrop")), 1);
        assert_eq!(choice_at(Some("blurred")), 2);
        assert_eq!(choice_at(Some("sharp")), 3);
        assert_eq!(choice_at(Some("#1f2b48")), 4);
        assert_eq!(choice_at(Some("/home/you/lock.png")), 5);
        assert_eq!(choice_at(Some("lock.png")), 0);
    }

    #[test]
    fn the_cards_are_the_built_in_looks_then_the_persons_own() {
        let lock = LockScreen::default();
        let names: Vec<String> = card_list(&lock).into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, ["classic", "magazine", "minimal"]);
        let own = edel::settings::OwnLook {
            parts: lock_screen::classic(false),
            background: None,
        };
        let lock = LockScreen {
            looks: Some([("home".to_string(), own)].into_iter().collect()),
            ..LockScreen::default()
        };
        let names: Vec<String> = card_list(&lock).into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, ["classic", "magazine", "minimal", "home"]);
        assert_eq!(lock_screen::look_title("magazine"), "Magazine");
        assert_eq!(lock_screen::look_title("home"), "Home");
    }

    #[test]
    fn the_current_look_is_titled_by_its_name_or_as_the_persons_own() {
        let by_name = LockScreen {
            look: Some("magazine".to_string()),
            ..LockScreen::default()
        };
        assert_eq!(current_title(&by_name), "Magazine");
        assert_eq!(current_title(&LockScreen::default()), "Classic");
        let own = LockScreen {
            parts: Some(lock_screen::classic(false)),
            look: Some("magazine".to_string()),
            ..LockScreen::default()
        };
        assert_eq!(current_title(&own), "Your own arrangement");
    }

    #[test]
    fn the_background_tiles_are_six_in_the_order_people_see_them() {
        let titles: Vec<String> = (0..6).map(tile_title).collect();
        assert_eq!(
            titles,
            [
                "The look's own",
                "Navy",
                "Blurred",
                "Sharp",
                "Colour",
                "Picture"
            ]
        );
        assert_eq!(TILE_NAMES.len(), titles.len());
    }

    #[test]
    fn the_tile_chosen_is_the_one_the_background_value_names() {
        // Absent and a value that is no background are the look's own.
        assert_eq!(choice_at(None), 0);
        assert_eq!(choice_at(Some("look")), 0);
        assert_eq!(choice_at(Some("backdrop")), 1, "navy is written backdrop");
        assert_eq!(choice_at(Some("blurred")), 2);
        assert_eq!(choice_at(Some("sharp")), 3);
        assert_eq!(choice_at(Some("#336699")), 4);
        assert_eq!(choice_at(Some("/home/you/lock.png")), 5);
    }

    #[test]
    fn the_pictures_keep_the_shape_of_sixteen_to_ten() {
        assert_eq!((HERO_W, HERO_H), (240, 150));
        assert_eq!((CARD_W, CARD_H), (160, 100));
        assert_eq!((TILE_W, TILE_H), (96, 60));
        for (w, h) in [(HERO_W, HERO_H), (CARD_W, CARD_H), (TILE_W, TILE_H)] {
            assert_eq!(w * 10, h * 16, "{w} by {h}");
        }
    }
}
