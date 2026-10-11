//! The Lock screen page (M5.10c5c, M5.32b, M5.10c7b): a gallery of looks, each
//! a preset of its own with its parts and its background. The three built-in
//! looks never change; the person's own are kept in `lock_screen.looks`, made
//! from the look in use (New look, Duplicate, Keep as a look), renamed or
//! deleted. Under the gallery is the look in use, with Customize and a More
//! menu, then the notifications' switch. Each choice is one key of the
//! person's settings file, written with `Files` as the other pages write
//! theirs (ADR-008): `lock_screen.look`, `lock_screen.looks` and, for a look
//! changed from the command line, `lock_screen.parts` and
//! `lock_screen.background`. The cards are drawn from the parts the lock
//! draws, laid out by `edel::lock_screen::arrange` on a 1280 by 800 screen, so
//! a card shows where each part stands.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;

use adw::prelude::*;
use gtk::cairo::{Context, LinearGradient};
use gtk::{gio, glib};

use edel::i18n::{n_, tr, trf};
use edel::lock_screen::{self, BACKGROUND, Background, Face, Item, LOOK, Part, Placed, Rect};
use edel::panel_edit::EDIT_LOCK_SCREEN;
use edel::settings::{LockScreen, OwnLook, Source};
use edel::tokens::{Colour, Tokens};

use crate::files::Files;
use crate::style::Theme;
use crate::{icon, panels, preview, rows, widgets};

const INTRO: &str = n_("Pick a look. Each look keeps its own arrangement and background.");

/// The keys this page chooses.
const PARTS: &str = lock_screen::PARTS;
const NOTIFICATIONS: &str = "lock_screen.notifications";
const OWN_LOOKS: &str = "lock_screen.looks";

/// The look that applies while none is chosen, and the notifications'
/// values, as `edel::lock_screen` owns them.
use lock_screen::{CLASSIC, COUNT_HIDDEN as NONE, COUNT_SHOWN as COUNT};

/// The screen a look is drawn on, in logical pixels: 1280 by 800, the size
/// the mockups draw the lock screen at.
const SCREEN: (f32, f32) = (1280.0, 800.0);
/// The room kept from the screen's edges and between two parts.
const MARGIN: f32 = 24.0;
const GAP: f32 = 16.0;

/// A look's picture on its card in logical pixels: 256 by 160, 16:10, and the
/// room between the cards.
const CARD_W: i32 = 256;
const CARD_H: i32 = 160;
const CARD_GAP: i32 = 16;

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

/// What a look's picture draws: its parts and whether its background is the
/// navy scene. Shared with the picture, so a change is drawn in place.
type LookShown = (Vec<Placed>, bool);

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

/// The text `lock_screen.parts` takes, as `edel settings set` reads it: the
/// list of tables in one line, absent options left out.
fn parts_text(parts: &[Placed]) -> Result<String, String> {
    lock_screen::parts_value(parts)
}

/// The built-in looks, then the person's own by name, each with what its
/// picture draws: the parts and whether the background is the navy scene.
/// Each look is applied on its own, so a card shows the look as it is, not
/// what the person has changed over it.
fn card_list(lock: &LockScreen) -> Vec<(String, LookShown)> {
    let built_in = lock_screen::LOOKS.iter().map(|name| name.to_string());
    let own = lock
        .looks
        .iter()
        .flat_map(|looks| looks.keys())
        .filter(|name| !lock_screen::is_built_in(name.as_str()))
        .cloned();
    built_in
        .chain(own)
        .map(|name| {
            let look = LockScreen {
                look: Some(name.clone()),
                looks: lock.looks.clone(),
                ..LockScreen::default()
            };
            let applied = lock_screen::applied(&look, false);
            let shown = (applied.parts, applied.background == Background::Backdrop);
            (name, shown)
        })
        .collect()
}

/// The look in use: the one the file names, else classic.
fn current_look(lock: &LockScreen) -> String {
    lock.look.clone().unwrap_or_else(|| CLASSIC.to_string())
}

/// The title of what applies now: the person's own arrangement while
/// `lock_screen.parts` is set, else the look's name.
fn current_title(lock: &LockScreen) -> String {
    if lock.parts.is_some() {
        tr("Your own arrangement").to_string()
    } else {
        lock_screen::look_title(&current_look(lock))
    }
}

/// The line under the look in use: what the person can do with it. A look
/// changed over from the command line says so, whatever kind it is.
fn look_line(name: &str, changed: bool) -> String {
    if changed {
        trf(
            "Changed over {look} on the command line.",
            &[("look", &lock_screen::look_title(name))],
        )
    } else if lock_screen::is_built_in(name) {
        tr("Built in. Edit it on the lock; your changes become a new look.").to_string()
    } else {
        tr("Yours. Edit it on the lock, where it is.").to_string()
    }
}

/// Takes `key` out of the person's file, when they have it: nothing is
/// written when they have none, as writers never write a default.
fn reset_person(files: &Files, key: &str) -> Result<(), String> {
    match files.source(key) {
        Source::Person(_) => files.set(key, None),
        _ => Ok(()),
    }
}

/// The parts that apply now, from the person's file over the machine's.
/// The command that sets what applies for `key` on any machine, as Copy as
/// command gives it.
fn command_for(key: &str) -> String {
    let files = Files::here();
    let lock = files.lock();
    let applied = lock_screen::applied(&lock, false);
    let value = match key {
        PARTS => parts_text(&applied.parts).unwrap_or_default(),
        NOTIFICATIONS if applied.notifications => COUNT.to_string(),
        NOTIFICATIONS => NONE.to_string(),
        _ => current_look(&lock),
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
/// arrangement and background are taken out, as a look brings both.
fn choose_look(name: &str) -> Result<(), String> {
    let files = Files::here();
    if name == CLASSIC {
        files.choose_over(LOOK, CLASSIC, Some(CLASSIC))?;
    } else {
        files.set(LOOK, Some(&format!("{name:?}")))?;
    }
    reset_person(&files, PARTS)?;
    reset_person(&files, BACKGROUND)
}

/// Makes a look of what applies, named after `base` (the look in use): New
/// look, Duplicate and Keep as a look all do this. The new look is chosen,
/// and the person's arrangement and background are taken out, as it keeps
/// them. Returns the name it got.
fn save_as_look(base: &str) -> Result<String, String> {
    let files = Files::here();
    let lock = files.lock();
    let own = files.person_file().lock_screen.looks;
    let taken = |name: &str| own.as_ref().is_some_and(|own| own.contains_key(name));
    let name = lock_screen::new_look_name(base, taken);
    let text = lock_screen::looks_value(own.as_ref(), &name, Some(lock_screen::as_own_look(&lock)));
    files.set(OWN_LOOKS, text.as_deref())?;
    files.set(LOOK, Some(&format!("{name:?}")))?;
    reset_person(&files, PARTS)?;
    reset_person(&files, BACKGROUND)?;
    Ok(name)
}

/// The value of `lock_screen.looks` with the look `old` renamed `new`, its
/// parts and background kept; None when no look is left. A look that is not
/// in `own` leaves the others as they are.
fn renamed(own: Option<&BTreeMap<String, OwnLook>>, old: &str, new: &str) -> Option<String> {
    let mut own = own.cloned().unwrap_or_default();
    match own.remove(old) {
        Some(look) => lock_screen::looks_value(Some(&own), new, Some(look)),
        None => lock_screen::looks_value(Some(&own), old, None),
    }
}

/// Renames one of the person's looks, the one in use: the key's own check
/// says what a name must be, and a name another look has is refused.
fn rename_look(old: &str, new: &str) -> Result<(), String> {
    let files = Files::here();
    let own = files.person_file().lock_screen.looks;
    if new != old && own.as_ref().is_some_and(|own| own.contains_key(new)) {
        return Err(trf(
            "There is a look called {look} already. Choose another name.",
            &[("look", &format!("{new:?}"))],
        ));
    }
    files.set(OWN_LOOKS, renamed(own.as_ref(), old, new).as_deref())?;
    files.set(LOOK, Some(&format!("{new:?}")))
}

/// Deletes one of the person's looks, the one in use: the look named is
/// taken out, and classic applies, as no look is in use then.
fn delete_look(name: &str) -> Result<(), String> {
    let files = Files::here();
    let own = files.person_file().lock_screen.looks;
    // The look line first, so the file never names a look it has not got,
    // and the changes over the look go with it.
    if files.lock().look.as_deref() == Some(name) {
        for key in [LOOK, PARTS, BACKGROUND] {
            reset_person(&files, key)?;
        }
    }
    files.set(
        OWN_LOOKS,
        lock_screen::looks_value(own.as_ref(), name, None).as_deref(),
    )
}

/// A card of a look: its button, the badge that shows it is chosen and its
/// tag.
struct LookCard {
    name: String,
    button: gtk::ToggleButton,
    badge: gtk::Box,
    in_use: gtk::Box,
}

/// The page's widgets, so every change shows everywhere on it.
struct Ui {
    theme: Rc<Theme>,
    /// The look in use: its title, its line, and the buttons beside it. The
    /// card is `current_head`, which the page scrolls to when asked for the
    /// background or the parts.
    current_title: gtk::Label,
    /// The look in use drawn small beside its name, redrawn in place.
    current_shown: Rc<RefCell<LookShown>>,
    current_view: gtk::DrawingArea,
    current_line: gtk::Label,
    current_head: gtk::Widget,
    keep: gtk::Button,
    edit: gtk::Button,
    more: gtk::MenuButton,
    menu: gtk::Popover,
    duplicate: gtk::Button,
    rename: gtk::Button,
    delete: gtk::Button,
    /// The gallery: its heading, its flow, the cards and the New look card.
    looks_head: gtk::Widget,
    looks: gtk::FlowBox,
    cards: RefCell<Vec<LookCard>>,
    new_card: RefCell<Option<gtk::Button>>,
    /// The cards' names and what they draw, as they were built, so they are
    /// built again only when a look changes.
    shown: RefCell<Vec<(String, LookShown)>>,
    look_source: widgets::Source,
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
    /// Shows what the files say now: the gallery, the look in use with its
    /// line and buttons, and the notifications' count, each with where its
    /// value comes from.
    fn update(self: &Rc<Self>) {
        let files = Files::here();
        let lock = files.lock();
        let look = current_look(&lock);
        self.quiet.set(true);
        let changed = [PARTS, BACKGROUND]
            .iter()
            .any(|key| matches!(files.source(key), Source::Person(_)));
        self.current_title.set_label(&current_title(&lock));
        let applied = lock_screen::applied(&lock, false);
        *self.current_shown.borrow_mut() =
            (applied.parts, applied.background == Background::Backdrop);
        self.current_view.queue_draw();
        self.current_line.set_label(&look_line(&look, changed));
        self.keep.set_visible(changed);
        let own = !lock_screen::is_built_in(&look);
        self.rename.set_visible(own);
        self.delete.set_visible(own);
        let title = lock_screen::look_title(&look);
        self.more
            .update_property(&[gtk::accessible::Property::Label(
                trf("More for {look}", &[("look", &title)]).as_str(),
            )]);
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
            [LOOK, PARTS, BACKGROUND]
                .iter()
                .any(|key| rows::resettable(&files.source(key))),
        );

        let count = files.source(NOTIFICATIONS);
        self.count
            .set_active(lock_screen::applied(&lock, false).notifications);
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

    /// Shows the gallery: built again when a look changes, and the one that
    /// applies chosen, with its tag. Nothing is chosen while the person's own
    /// arrangement is set.
    fn show_looks(self: &Rc<Self>, lock: &LockScreen) {
        let list = card_list(lock);
        if *self.shown.borrow() != list {
            self.build_cards(&list);
        }
        let chosen = match lock.parts {
            Some(_) => None,
            None => Some(current_look(lock)),
        };
        for card in self.cards.borrow().iter() {
            let on = chosen.as_deref() == Some(card.name.as_str());
            card.button.set_active(on);
            // The In use pill says it, so the card's own check stays hidden.
            card.badge.set_visible(false);
            card.in_use.set_visible(on);
        }
    }

    /// Builds the cards of `list` in the flow, each a picture of its parts
    /// with its name and tag, then the New look card, and wires each to a
    /// choice.
    fn build_cards(self: &Rc<Self>, list: &[(String, LookShown)]) {
        while let Some(child) = self.looks.first_child() {
            self.looks.remove(&child);
        }
        let mut cards = Vec::new();
        let mut first: Option<gtk::ToggleButton> = None;
        for (name, shown) in list {
            let shown = Rc::new(RefCell::new(shown.clone()));
            let picture = look_picture(&self.theme, &shown, CARD_W, CARD_H);
            // The card may be wider than the picture's least size: the frame
            // keeps the screen's 16:10 shape at whatever width it is given.
            // A built-in look wears its badge on the picture's corner.
            let shown_picture = gtk::Overlay::builder().child(&picture).build();
            if lock_screen::is_built_in(name) {
                let built_in = widgets::pill("lock", tr("Built in"), "edel-built-in");
                built_in.set_halign(gtk::Align::Start);
                built_in.set_valign(gtk::Align::Start);
                built_in.set_margin_start(8);
                built_in.set_margin_top(8);
                shown_picture.add_overlay(&built_in);
            }
            let frame = gtk::AspectFrame::builder()
                .ratio(SCREEN.0 / SCREEN.1)
                .obey_child(false)
                .hexpand(true)
                .child(&shown_picture)
                .build();
            let (button, badge) =
                widgets::picture_card(&lock_screen::look_title(name), &frame.upcast());
            // In use sits after the name, on the same row.
            let in_use = widgets::pill("check", tr("In use"), "edel-in-use");
            in_use.set_visible(false);
            if let Some(row) = badge.parent().and_downcast::<gtk::Box>() {
                row.insert_child_after(&in_use, row.first_child().as_ref());
            }
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
                    let result = choose_look(&chosen);
                    ui.update();
                    ui.report(result);
                }
            });
            cards.push(LookCard {
                name: name.clone(),
                button,
                badge,
                in_use,
            });
        }
        let new = self.new_look_card();
        self.looks.append(&new);
        *self.new_card.borrow_mut() = Some(new);
        *self.cards.borrow_mut() = cards;
        *self.shown.borrow_mut() = list.to_vec();
    }

    /// The New look card (M5.10c7b): the same size as the looks, its picture
    /// a dashed outline with a round "+" and a line saying where the look
    /// comes from, and its caption under it. A click makes a look of the one
    /// in use.
    fn new_look_card(self: &Rc<Self>) -> gtk::Button {
        let button = widgets::new_card(CARD_W, CARD_H, tr("From the look in use"), tr("New look"));
        let weak = Rc::downgrade(self);
        button.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.make_look();
            }
        });
        button
    }

    /// Makes a look of the one in use, and chooses it (New look, Duplicate
    /// and Keep as a look).
    fn make_look(self: &Rc<Self>) {
        let base = current_look(&Files::here().lock());
        let result = save_as_look(&base);
        if let Ok(name) = &result {
            eprintln!("edel-settings: lock look {name} made");
        }
        self.update();
        self.report(result.map(|_| ()));
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

/// A look's picture of `w` by `h` logical pixels: its parts on the screen,
/// drawn as the lock draws them, over the navy scene where the look has it
/// and the mid grey-blue for the rest (the wallpaper behind them, blurred),
/// in the tokens' colours. It draws `shown` as it is each time, so a change
/// to it is drawn in place.
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
        let (x, y, rw, rh) = (
            f64::from(r.x) * scale,
            f64::from(r.y) * scale,
            f64::from(r.w) * scale,
            f64::from(r.h) * scale,
        );
        // The clock and the date are drawn as words, as the lock shows them,
        // so a card reads as a lock screen at a glance; the rest as shapes.
        let words = match (part.part, part.face) {
            (Part::Clock, Some(Face::Analog)) => None,
            (Part::Clock, Some(Face::Stacked)) => Some(("09\n41", 0.42)),
            (Part::Clock, _) => Some(("9:41", 0.78)),
            (Part::Date, _) => Some((SAMPLE_DATE, 0.62)),
            _ => None,
        };
        if let Some((text, share)) = words {
            preview::set(cr, t.backdrop_text);
            draw_words(cr, &t.font, text, (x, y, rw, rh), share);
            continue;
        }
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
        if part.part == Part::Clock {
            // An analog face: its ring and two hands.
            let radius = rw.min(rh) / 2.0 * 0.9;
            let (cx, cy) = (x + rw / 2.0, y + rh / 2.0);
            cr.set_line_width((radius / 10.0).max(1.0));
            cr.arc(cx, cy, radius, 0.0, std::f64::consts::TAU);
            cr.move_to(cx, cy);
            cr.line_to(cx + radius * 0.5, cy - radius * 0.2);
            cr.move_to(cx, cy);
            cr.line_to(cx - radius * 0.1, cy - radius * 0.75);
            let _ = cr.stroke();
            continue;
        }
        preview::rounded(cr, x, y, rw, rh, PART_RADIUS * scale);
        let _ = cr.fill();
    }
}

/// The date a card's date shows, as the lock's long style writes one.
const SAMPLE_DATE: &str = "Sunday 11 October";

/// Draws `text` (lines split at `\n`) centred in `rect` = (x, y, w, h) in the
/// interface font, each line `share` of the rectangle's height, made smaller
/// when it would be wider than the rectangle.
fn draw_words(cr: &Context, font: &str, text: &str, rect: (f64, f64, f64, f64), share: f64) {
    let (x, y, w, h) = rect;
    let lines: Vec<&str> = text.split('\n').collect();
    cr.select_font_face(
        font,
        gtk::cairo::FontSlant::Normal,
        gtk::cairo::FontWeight::Normal,
    );
    let mut size = h * share;
    cr.set_font_size(size);
    let widest = lines
        .iter()
        .filter_map(|line| cr.text_extents(line).ok())
        .map(|e| e.x_advance())
        .fold(0.0, f64::max);
    if widest > w * 0.96 && widest > 0.0 {
        size *= w * 0.96 / widest;
        cr.set_font_size(size);
    }
    let step = size * 1.05;
    let top = y + (h - step * lines.len() as f64) / 2.0;
    for (i, line) in lines.iter().enumerate() {
        let Ok(e) = cr.text_extents(line) else {
            continue;
        };
        cr.move_to(
            x + (w - e.x_advance()) / 2.0,
            top + step * i as f64 + size * 0.8,
        );
        let _ = cr.show_text(line);
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

/// The Lock screen page (M5.10c5c, M5.32b, M5.10c7b): the gallery of looks
/// with the New look card, the look in use with Customize and its More menu,
/// then the notifications' count, each row with its Reset, Copy as command
/// and where its value comes from.
pub fn page(theme: &Rc<Theme>) -> gtk::Widget {
    let files = Files::here();
    let (page, content, problem) = widgets::page(tr("Lock screen"), tr(INTRO));

    // The gallery: a heading with where the look in use comes from, then the
    // cards, at most three a line.
    let looks = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .homogeneous(true)
        .min_children_per_line(1)
        .max_children_per_line(3)
        .column_spacing(CARD_GAP as u32)
        .row_spacing(CARD_GAP as u32)
        .build();
    let look_source = widgets::section(&content, tr("Looks"));
    let looks_head = content
        .last_child()
        .unwrap_or_else(|| looks.clone().upcast());
    content.append(&looks);

    // The look in use: its title and line, then Keep as a look (only for a
    // look changed over from the command line), Customize and the More menu.
    let title = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .css_classes(["edel-current-title"])
        .build();
    let line = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .css_classes(["edel-intro"])
        .build();
    let words = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build();
    // The name with its In use tag, as the gallery's card says it.
    let name_row = gtk::Box::builder().spacing(8).build();
    name_row.append(&title);
    name_row.append(&widgets::pill("check", tr("In use"), "edel-in-use"));
    words.append(&name_row);
    words.append(&line);
    let keep = widgets::action(tr("Keep as a look"), false);
    // Customize, the word the lock's switcher uses for the same thing.
    let edit = widgets::action(tr("Customize"), true);
    for button in [&keep, &edit] {
        button.add_css_class("edel-current-button");
    }
    let more = gtk::MenuButton::builder()
        .child(&icon::image("more", 16))
        .tooltip_text(tr("More"))
        .valign(gtk::Align::Center)
        .css_classes(["edel-more"])
        .build();
    let menu = gtk::Popover::builder()
        .css_classes(["edel-choice-list"])
        .build();
    let items = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    let duplicate = widgets::menu_item(tr("Duplicate"));
    let rename = widgets::menu_item(tr("Rename…"));
    let delete = widgets::menu_item(tr("Delete…"));
    for item in [&duplicate, &rename, &delete] {
        items.append(item);
    }
    menu.set_child(Some(&items));
    more.set_popover(Some(&menu));
    let actions = gtk::Box::builder()
        .spacing(10)
        .valign(gtk::Align::Center)
        .build();
    actions.append(&keep);
    actions.append(&edit);
    actions.append(&more);
    let current = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(20)
        .hexpand(true)
        .css_classes(["edel-group", "edel-current"])
        .build();
    // The look in use, small, beside its name (16:10, as every card).
    let current_shown: Rc<RefCell<LookShown>> = Rc::new(RefCell::new((Vec::new(), false)));
    let current_view = look_picture(theme, &current_shown, 96, 60);
    current_view.set_valign(gtk::Align::Center);
    // Its own size when the card stacks too, never stretched across.
    current_view.set_halign(gtk::Align::Start);
    current.append(&current_view);
    current.append(&words);
    current.append(&actions);
    // The page's narrow width, as the window's: the card is what it
    // measures, and at that width it stacks.
    let card = current.clone();
    let end = actions.clone();
    let bin = widgets::folding(&current, move |narrow| {
        card.set_orientation(if narrow {
            gtk::Orientation::Vertical
        } else {
            gtk::Orientation::Horizontal
        });
        end.set_halign(if narrow {
            gtk::Align::Start
        } else {
            gtk::Align::End
        });
    });
    bin.set_margin_top(12);
    content.append(&bin);

    // The notifications' count, under its own heading.
    widgets::heading(&content, tr("On every look"));
    let count_group = widgets::group(&content);
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
        current_shown,
        current_view,
        current_line: line,
        current_head: bin.clone().upcast(),
        keep: keep.clone(),
        edit: edit.clone(),
        more: more.clone(),
        menu: menu.clone(),
        duplicate: duplicate.clone(),
        rename: rename.clone(),
        delete: delete.clone(),
        looks_head,
        looks: looks.clone(),
        cards: RefCell::new(Vec::new()),
        new_card: RefCell::new(None),
        shown: RefCell::new(Vec::new()),
        look_source,
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
    wire(&ui);
    log_places(&looks, &ui);

    // The page's state lives as long as the page does.
    let keep_ui = ui.clone();
    page.connect_destroy(move |_| {
        let _ = &keep_ui;
    });
    page
}

/// Connects the page's controls, the More menu, and the Reset and Copy
/// buttons of its rows and of the gallery's section.
fn wire(ui: &Rc<Ui>) {
    // Customize asks shell-ui for its edit mode over the session bus, as the
    // Panels group asks for the panel editor (the same call, given the
    // method's name); the page writes nothing itself.
    let weak = Rc::downgrade(ui);
    ui.edit.connect_clicked(move |_| {
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
    ui.keep.connect_clicked(move |_| {
        if let Some(ui) = weak.upgrade() {
            ui.make_look();
        }
    });

    let weak = Rc::downgrade(ui);
    ui.duplicate.connect_clicked(move |_| {
        if let Some(ui) = weak.upgrade() {
            ui.menu.popdown();
            ui.make_look();
        }
    });

    let weak = Rc::downgrade(ui);
    ui.rename.connect_clicked(move |_| {
        if let Some(ui) = weak.upgrade() {
            ui.menu.popdown();
            let current = current_look(&Files::here().lock());
            ask_name(&ui, &ui.current_head, &current);
        }
    });

    let weak = Rc::downgrade(ui);
    ui.delete.connect_clicked(move |_| {
        if let Some(ui) = weak.upgrade() {
            ui.menu.popdown();
            let current = current_look(&Files::here().lock());
            ask_delete(&ui, &ui.current_head, &current);
        }
    });

    let weak = Rc::downgrade(ui);
    ui.count.connect_active_notify(move |switch| {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if !ui.quiet.get() {
            let value = if switch.is_active() { COUNT } else { NONE };
            let result = Files::here().choose_over(NOTIFICATIONS, value, Some(COUNT));
            ui.update();
            ui.report(result);
        }
    });

    // Reset: the look, its arrangement and its background, all of them the
    // person's own choices, or the count.
    let weak = Rc::downgrade(ui);
    ui.look_source.reset.connect_clicked(move |_| {
        if let Some(ui) = weak.upgrade() {
            let files = Files::here();
            let result = reset_person(&files, LOOK)
                .and_then(|()| reset_person(&files, PARTS))
                .and_then(|()| reset_person(&files, BACKGROUND));
            ui.update();
            ui.report(result);
        }
    });
    let weak = Rc::downgrade(ui);
    ui.count_row.reset.connect_clicked(move |_| {
        if let Some(ui) = weak.upgrade() {
            let result = reset_person(&Files::here(), NOTIFICATIONS);
            ui.update();
            ui.report(result);
        }
    });

    // Copy as command: the section's line for what applies now, and the
    // count's.
    copy(&ui.look_source.copy, || {
        let files = Files::here();
        if files.lock().parts.is_some() {
            command_for(PARTS)
        } else {
            command_for(LOOK)
        }
    });
    copy(&ui.count_row.copy, || command_for(NOTIFICATIONS));
}

/// Makes `button` copy the line `text` gives when it is clicked.
fn copy(button: &gtk::Button, text: impl Fn() -> String + 'static) {
    button.connect_clicked(move |button| {
        button.clipboard().set_text(&text());
        widgets::copied(button);
    });
}

/// Asks for a new name for the look `current` (Rename...), in a small
/// dialog (`widgets::ask_name`). The name is checked by the settings file's
/// own rule for `lock_screen.looks`; a name another look has is refused.
fn ask_name(ui: &Rc<Ui>, near: &gtk::Widget, current: &str) {
    let weak = Rc::downgrade(ui);
    let old = current.to_string();
    widgets::ask_name(
        near,
        tr("Rename look"),
        tr(
            "Give this look another name. It keeps its parts and its background, and shows in the cards above.",
        ),
        current,
        tr("Name, such as home"),
        tr("Rename"),
        move |new| {
            let Some(ui) = weak.upgrade() else {
                return Ok(());
            };
            rename_look(&old, new)?;
            eprintln!("edel-settings: lock look {old} renamed {new}");
            ui.update();
            Ok(())
        },
    );
}

/// Asks whether to delete the look `current` (Delete), with Cancel as the
/// default and the escape key, and deletes it on Delete.
fn ask_delete(ui: &Rc<Ui>, near: &gtk::Widget, current: &str) {
    let name = current.to_string();
    let weak = Rc::downgrade(ui);
    widgets::ask_delete(
        near,
        &trf(
            "Delete the look {look}?",
            &[("look", &lock_screen::look_title(current))],
        ),
        tr("It cannot be brought back."),
        tr("Delete"),
        move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let result = delete_look(&name);
            if result.is_ok() {
                eprintln!("edel-settings: lock look {name} deleted");
            }
            ui.update();
            ui.report(result);
        },
    );
}

/// Logs where the looks' cards, the New look card and the section's Reset
/// lie in the window, once the page is laid out, for CI to click (logical
/// pixels), as `edel-settings: lock_screen group places classic X+Y+WxH
/// magazine X+Y+WxH minimal X+Y+WxH new X+Y+WxH, reset X+Y+WxH`. Asked for by
/// `lock_screen.look` the page first scrolls the gallery to the top and gives
/// the keyboard to the look in use; asked for by `lock_screen.background` or
/// `.parts` it scrolls the look in use to the top and gives the keyboard to
/// Customize. It logs once two frames agree, after the scroll.
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
        let new = ui.new_card.borrow();
        let (Some(new), false) = (new.as_ref(), cards.is_empty()) else {
            return glib::ControlFlow::Continue;
        };
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
        let places_line = format!(
            "edel-settings: lock_screen group places {} new {}, reset {}",
            places.join(" "),
            widgets::place(new.compute_bounds(&window)),
            widgets::place(ui.look_source.reset.compute_bounds(&window)),
        );
        // The view settles over a few frames, so the places are logged only
        // once two frames agree.
        if last.replace(places_line.clone()) != places_line {
            return glib::ControlFlow::Continue;
        }
        eprintln!("{places_line}");
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
            let chosen = cards
                .iter()
                .find(|card| card.button.is_active())
                .or(cards.first());
            if let Some(card) = chosen {
                widgets::take_asked(&page, &card.button);
            }
        }
        Some(BACKGROUND) | Some(PARTS) => {
            widgets::scroll_to_top(&ui.current_head);
            widgets::take_asked(&page, &ui.edit);
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
    }

    #[test]
    fn the_parts_written_read_back_to_the_same_parts() {
        for name in lock_screen::LOOKS {
            let parts = lock_screen::look_parts(name, false).unwrap();
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
        let own: BTreeMap<String, OwnLook> = [(
            "home".to_string(),
            OwnLook {
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
        let bad: BTreeMap<String, OwnLook> = [(
            "my look".to_string(),
            OwnLook {
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
    fn the_cards_are_the_built_in_looks_then_the_persons_own() {
        let lock = LockScreen::default();
        let names: Vec<String> = card_list(&lock).into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, ["classic", "magazine", "minimal"]);
        let own = OwnLook {
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
    fn a_card_draws_its_own_look_and_its_background() {
        let own = OwnLook {
            parts: lock_screen::classic(false),
            background: Some("backdrop".to_string()),
        };
        let lock = LockScreen {
            looks: Some([("home".to_string(), own)].into_iter().collect()),
            ..LockScreen::default()
        };
        let cards = card_list(&lock);
        let (_, (_, backdrop)) = cards.iter().find(|(n, _)| n == "home").unwrap();
        assert!(*backdrop, "the look's own navy background");
        let (_, (_, magazine)) = cards.iter().find(|(n, _)| n == "magazine").unwrap();
        assert!(!magazine, "magazine is blurred");
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
    fn the_line_under_the_look_in_use_says_what_it_is() {
        assert_eq!(
            look_line("classic", false),
            "Built in. Edit it on the lock; your changes become a new look."
        );
        assert_eq!(
            look_line("home", false),
            "Yours. Edit it on the lock, where it is."
        );
        assert_eq!(
            look_line("home", true),
            "Changed over Home on the command line."
        );
        assert_eq!(
            look_line("classic", true),
            "Changed over Classic on the command line."
        );
    }

    #[test]
    fn a_renamed_look_keeps_its_parts_under_the_new_name() {
        let parts = lock_screen::classic(false);
        let own: BTreeMap<String, OwnLook> = [
            (
                "home".to_string(),
                OwnLook {
                    parts: parts.clone(),
                    background: None,
                },
            ),
            (
                "work".to_string(),
                OwnLook {
                    parts: lock_screen::classic(true),
                    background: None,
                },
            ),
        ]
        .into_iter()
        .collect();
        let text = renamed(Some(&own), "home", "den").unwrap();
        let set = edel::settings::set("format = 1\n", OWN_LOOKS, &text).unwrap();
        let read = edel::settings::read(&set).unwrap();
        assert!(read.problems.is_empty(), "{:?}", read.problems);
        let looks = read.file.lock_screen.looks.unwrap();
        assert!(!looks.contains_key("home"), "the old name is gone");
        assert_eq!(looks["den"].parts, parts);
        assert!(looks.contains_key("work"), "the others stay");
    }

    #[test]
    fn deleting_the_last_own_look_gives_none() {
        let own: BTreeMap<String, OwnLook> = [(
            "home".to_string(),
            OwnLook {
                parts: lock_screen::classic(false),
                background: None,
            },
        )]
        .into_iter()
        .collect();
        assert_eq!(lock_screen::looks_value(Some(&own), "home", None), None);
        assert_eq!(renamed(None, "home", "den"), None);
    }

    #[test]
    fn the_pictures_keep_the_shape_of_sixteen_to_ten() {
        assert_eq!((CARD_W, CARD_H), (256, 160));
        assert_eq!(CARD_W * 10, CARD_H * 16);
    }
}
