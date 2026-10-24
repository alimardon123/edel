//! The Lock screen page (M5.10c5c, M5.32b, M5.10c7b): a gallery of looks, each
//! a preset of its own with its parts and its background. The three built-in
//! looks never change; the person's own are kept in `lock_screen.looks`, made
//! from the look in use (New look, Duplicate, Keep as a look), renamed or
//! deleted. Under the gallery is the look in use, with Customize and a More
//! menu, then the notifications' switch. Each choice is one key of the
//! person's settings file, written with `Files` as the other pages write
//! theirs (ADR-008): `lock_screen.look`, `lock_screen.looks` and, for a look
//! changed from the command line, `lock_screen.widgets` and
//! `lock_screen.wallpaper`. The cards are drawn from the parts the lock
//! draws, laid out by `edel::lock_screen::arrange` on a 1280 by 800 screen, so
//! a card shows where each part stands.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;

use adw::prelude::*;
use gtk::cairo::{Context, LinearGradient, RadialGradient};
use gtk::{gio, glib};

use edel::i18n::{n_, tr, trf};
use edel::lock_screen::{self, Background, Face, Item, LOOK, Part, Placed, Rect, WALLPAPER};
use edel::panel_edit::EDIT_LOCK_SCREEN;
use edel::settings::{LockScreen, OwnLook, Source};
use edel::tokens::{Colour, Tokens};

use crate::files::Files;
use crate::look_pictures::{self, Inputs};
use crate::style::Theme;
use crate::{icon, panels, preview, rows, widgets};

const INTRO: &str = n_("Pick a look. Each look keeps its own arrangement and wallpaper.");

/// The keys this page chooses.
const WIDGETS: &str = lock_screen::WIDGETS;
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

/// A look's least picture on its card in logical pixels: 184 by 115, 16:10, so
/// three stand on a line as the mockup has them, and the
/// room between the cards.
const CARD_W: i32 = 184;
const CARD_H: i32 = 115;
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
const PART_RADIUS: f64 = 8.0;
/// A look's picture corner radius on its card, in the card's pixels: the
/// menus' radius, the same as the ring round the chosen look.
const PICTURE_CORNER: f64 = 12.0;
/// The alphas of the backdrop text on a card (M5.10c7b): a part's fill and
/// edge, an analog clock's face, the date's words, and the light a wallpaper
/// look has at its top left.
const FILL_ALPHA: f32 = 0.14;
const EDGE_ALPHA: f32 = 0.16;
const FACE_ALPHA: f32 = 0.45;
const DATE_ALPHA: f32 = 0.78;
const GLOW_ALPHA: f32 = 0.35;
/// The person's pill on the screen, in logical pixels tall; the person's disc
/// stands over it when the person's rect is at least 2.5 times as tall.
const PILL_H: f64 = 36.0;
/// A backdrop look's gradient angle, as the mockup has it (160 degrees, CSS
/// style: 0 up, 90 right), and a wallpaper look's, corner to corner.
const BACKDROP_ANGLE: f64 = 160.0;
const WALLPAPER_ANGLE: f64 = 135.0;

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

/// The text `lock_screen.widgets` takes, as `edel settings set` reads it: the
/// list of tables in one line, absent options left out.
fn parts_text(parts: &[Placed]) -> Result<String, String> {
    lock_screen::widgets_value(parts)
}

/// The built-in looks, then the person's own by name, each with what its
/// picture draws: the parts and whether the background is the navy scene,
/// and the key of its picture (`look_pictures::key`). Each look is applied on
/// its own, so a card shows the look as it is, not what the person has
/// changed over it.
fn card_list(lock: &LockScreen, inputs: &Inputs) -> Vec<(String, LookShown, String)> {
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
            let key = look_pictures::key(&applied.parts, &applied.background, applied.dim, inputs);
            let shown = (applied.parts, applied.background == Background::Backdrop);
            (name, shown, key)
        })
        .collect()
}

/// The look in use: the one the file names, else classic.
fn current_look(lock: &LockScreen) -> String {
    lock.look.clone().unwrap_or_else(|| CLASSIC.to_string())
}

/// The title of what applies now: the person's own arrangement while
/// `lock_screen.widgets` is set, else the look's name.
fn current_title(lock: &LockScreen) -> String {
    if lock.parts.is_some() {
        tr("Your own arrangement").to_string()
    } else {
        lock_screen::look_title(&current_look(lock))
    }
}

/// The line under the look in use (M5.10c7b, as the mockup words it): what
/// the look shows and over what, such as "Clock, date, music player and
/// notification count over your wallpaper, blurred." A look changed over
/// from the command line says so instead.
fn look_line(name: &str, changed: bool, parts: &[Placed], background: &Background) -> String {
    if changed {
        return trf(
            "Changed over {look} on the command line.",
            &[("look", &lock_screen::look_title(name))],
        );
    }
    // In a fixed order, the clock first, whatever order the list has.
    let kinds = [
        (Part::Clock, tr("clock")),
        (Part::Date, tr("date")),
        (Part::Player, tr("music player")),
        (Part::Count, tr("notification count")),
        (Part::Message, tr("message")),
        (Part::Picture, tr("picture")),
        (Part::Battery, tr("battery")),
    ];
    let words: Vec<&str> = kinds
        .iter()
        .filter(|(kind, _)| parts.iter().any(|p| p.part == *kind))
        .map(|(_, word)| *word)
        .collect();
    let shown = match words.split_last() {
        None => tr("Only the sign in").to_string(),
        Some((last, [])) => (*last).to_string(),
        Some((last, rest)) => format!("{} {} {last}", rest.join(", "), tr("and")),
    };
    let mut shown = shown;
    if let Some(first) = shown.get(..1) {
        shown = first.to_uppercase() + &shown[1..];
    }
    let over = match background {
        Background::Backdrop => tr("the navy backdrop"),
        Background::Blurred => tr("your wallpaper, blurred"),
        Background::Sharp => tr("your wallpaper"),
        Background::Colour(_) => tr("a plain colour"),
        Background::Picture(_) => tr("a picture of yours"),
    };
    trf(
        "{parts} over {background}.",
        &[("parts", &shown), ("background", over)],
    )
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
        WIDGETS => parts_text(&applied.parts).unwrap_or_default(),
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
    let (machine, person) = files.texts();
    let text = lock_screen::chose_text(&person, machine.as_deref(), name)?;
    if text != person {
        files.write_person(&text)?;
    }
    Ok(())
}

/// Makes a look of what applies, named after `base` (the look in use): New
/// look, Duplicate and Keep as a look all do this. The new look is chosen,
/// and the person's arrangement and background are taken out, as it keeps
/// them. Returns the name it got. The text is the library's (M5.10c7h), as
/// the looks switcher on the lock writes it too.
fn save_as_look(base: &str) -> Result<String, String> {
    let files = Files::here();
    let (machine, person) = files.texts();
    let (text, name) = lock_screen::saved_as_text(&person, machine.as_deref(), base)?;
    files.write_person(&text)?;
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
        for key in [LOOK, WIDGETS, WALLPAPER] {
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
    /// Drawn by Settings, and the frame it sits in: a picture drawn by
    /// shell-ui replaces the drawing there (`look_pictures`).
    current_drawn: gtk::DrawingArea,
    current_view: gtk::Overlay,
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
    shown: RefCell<Vec<(String, LookShown, String)>>,
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
        let inputs = Inputs::of(
            files.chosen("appearance.wallpaper").as_deref(),
            files.chosen("appearance.wallpaper_dark").as_deref(),
            files.scheme().0 == edel::tokens::Scheme::Dark,
            &self.theme.tokens(),
        );
        self.quiet.set(true);
        let changed = [WIDGETS, WALLPAPER]
            .iter()
            .any(|key| matches!(files.source(key), Source::Person(_)));
        self.current_title.set_label(&current_title(&lock));
        let applied = lock_screen::applied(&lock, false);
        self.current_line.set_label(&look_line(
            &look,
            changed,
            &applied.parts,
            &applied.background,
        ));
        *self.current_shown.borrow_mut() =
            (applied.parts, applied.background == Background::Backdrop);
        self.current_drawn.queue_draw();
        // The look in use small is shell-ui's picture of the look while the
        // person changes nothing over it; Settings draws what they changed.
        let alone = lock_screen::applied(
            &LockScreen {
                look: Some(look.clone()),
                looks: lock.looks.clone(),
                ..LockScreen::default()
            },
            false,
        );
        let current_key = look_pictures::key(&alone.parts, &alone.background, alone.dim, &inputs);
        look_pictures::show_drawn(&self.current_view);
        if lock.parts.is_none() && lock.background.is_none() {
            look_pictures::want(&self.current_view, &look, &current_key);
        }
        self.keep.set_visible(changed);
        let own = !lock_screen::is_built_in(&look);
        self.rename.set_visible(own);
        self.delete.set_visible(own);
        let title = lock_screen::look_title(&look);
        self.more
            .update_property(&[gtk::accessible::Property::Label(
                trf("More for {look}", &[("look", &title)]).as_str(),
            )]);
        self.show_looks(&lock, &inputs);
        let source = if lock.parts.is_some() {
            files.source(WIDGETS)
        } else {
            files.source(LOOK)
        };
        self.look_source
            .label
            .set_label(&rows::describe(&source, &current_title(&lock)));
        self.look_source.reset.set_visible(
            [LOOK, WIDGETS, WALLPAPER]
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
        // The folder keeps the pictures this page shows, and no others.
        let mut keep: Vec<String> = self
            .shown
            .borrow()
            .iter()
            .map(|(_, _, key)| key.clone())
            .collect();
        keep.push(current_key);
        look_pictures::prune(&keep);
    }

    /// Shows the gallery: built again when a look changes, and the one that
    /// applies chosen, with its tag. Nothing is chosen while the person's own
    /// arrangement is set.
    fn show_looks(self: &Rc<Self>, lock: &LockScreen, inputs: &Inputs) {
        let list = card_list(lock, inputs);
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
            card.in_use.set_visible(on);
        }
    }

    /// Builds the cards of `list` in the flow, each a picture of its parts
    /// with its name and tag, then the New look card, and wires each to a
    /// choice.
    fn build_cards(self: &Rc<Self>, list: &[(String, LookShown, String)]) {
        while let Some(child) = self.looks.first_child() {
            self.looks.remove(&child);
        }
        let mut cards = Vec::new();
        let mut first: Option<gtk::ToggleButton> = None;
        for (name, shown, key) in list {
            let shown = Rc::new(RefCell::new(shown.clone()));
            let picture = look_picture(&self.theme, &shown, CARD_W, CARD_H, PICTURE_CORNER);
            // The picture fills its card's width, and its height follows the
            // width at the screen's 16:10 (`follow_width`): a flow box
            // measures its children without the width, so an aspect frame
            // would letterbox the picture.
            picture.set_hexpand(true);
            follow_width(&picture);
            // The picture is laid over the drawing by `look_pictures`; the
            // badges lie over both, so they stay on top when it comes.
            let shown_picture = gtk::Overlay::builder()
                .child(&picture)
                .hexpand(true)
                .css_classes(["edel-look-picture"])
                .build();
            let badges = gtk::Overlay::builder()
                .child(&shown_picture)
                .hexpand(true)
                .build();
            if lock_screen::is_built_in(name) {
                let built_in = widgets::pill("lock", tr("Built in"), "edel-built-in");
                built_in.set_halign(gtk::Align::End);
                built_in.set_valign(gtk::Align::Start);
                built_in.set_margin_end(8);
                built_in.set_margin_top(8);
                badges.add_overlay(&built_in);
            }
            // shell-ui's picture replaces the drawing once it is there.
            look_pictures::want(&shown_picture, name, key);
            // In use sits after the name, on the same row.
            let in_use = widgets::pill("check", tr("In use"), "edel-in-use");
            in_use.set_visible(false);
            let button = look_card(&lock_screen::look_title(name), &badges.upcast(), &in_use);
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
        let plus = gtk::Label::builder()
            .label("+")
            .hexpand(true)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .css_classes(["edel-new-plus"])
            .build();
        let circle = gtk::Box::builder()
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .css_classes(["edel-new-circle"])
            .build();
        circle.set_size_request(36, 36);
        circle.append(&plus);
        let hint = gtk::Label::builder()
            .label(tr("From the look in use"))
            .wrap(true)
            .css_classes(["edel-new-hint"])
            .build();
        let area = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .valign(gtk::Align::Fill)
            .vexpand(true)
            .hexpand(true)
            .css_classes(["edel-new-look"])
            .build();
        // The circle and the hint together, in the middle of the outline.
        let middle = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .vexpand(true)
            .valign(gtk::Align::Center)
            .build();
        middle.append(&circle);
        middle.append(&hint);
        area.append(&middle);
        let caption = gtk::Label::builder()
            .label(tr("New look"))
            .xalign(0.0)
            .css_classes(["edel-preset-name"])
            .build();
        let inner = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .build();
        // As tall as the looks' pictures, so the cards of a line stand level:
        // an empty picture that follows its width as theirs do, with the
        // outline over it.
        let room = gtk::DrawingArea::builder()
            .content_width(CARD_W)
            .content_height(CARD_H)
            .hexpand(true)
            .build();
        follow_width(&room);
        let level = gtk::Overlay::builder().child(&room).build();
        level.add_overlay(&area);
        inner.append(&level);
        inner.append(&caption);
        let button = gtk::Button::builder()
            .child(&inner)
            .css_classes(["edel-new-card"])
            .build();
        button.update_property(&[gtk::accessible::Property::Label(tr("New look"))]);
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

/// A look's card (M5.10c7b): its picture with nothing round it, then its name
/// and the In use pill on one row, 8 px below the picture. The chosen look's
/// ring and the hover are `style.rs`'s `edel-look-card` rules, so the cards
/// the Layout page shares (`widgets::picture_card`) keep their own look.
fn look_card(title: &str, picture: &gtk::Widget, in_use: &gtk::Box) -> gtk::ToggleButton {
    let name = gtk::Label::builder()
        .label(title)
        .xalign(0.0)
        .hexpand(true)
        .css_classes(["edel-look-name"])
        .build();
    let row = gtk::Box::builder().spacing(6).build();
    row.append(&name);
    row.append(in_use);
    let inner = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .build();
    inner.append(picture);
    inner.append(&row);
    let card = gtk::ToggleButton::builder()
        .child(&inner)
        .css_classes(["edel-look-card"])
        .build();
    card.update_property(&[gtk::accessible::Property::Label(title)]);
    card
}

/// A look's picture of `w` by `h` logical pixels: its parts on the screen,
/// drawn as the lock draws them, over the look's background (`draw_look`),
/// in the tokens' colours. It draws `shown` as it is each time, so a change
/// to it is drawn in place; `corner` is its corner radius in those pixels.
/// Keeps `area` as tall as the screen's 16:10 at whatever width it is
/// given, set when its size changes, so nothing runs while it stands still.
fn follow_width(area: &gtk::DrawingArea) {
    area.connect_resize(|area, w, _| {
        let wanted = w * CARD_H / CARD_W;
        if w > 0 && area.content_height() != wanted {
            area.set_content_height(wanted);
        }
    });
}

fn look_picture(
    theme: &Rc<Theme>,
    shown: &Rc<RefCell<LookShown>>,
    w: i32,
    h: i32,
    corner: f64,
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
        draw_look(cr, f64::from(w), f64::from(h), parts, *backdrop, corner, &t);
    });
    let weak = area.downgrade();
    theme.watch(move || {
        if let Some(area) = weak.upgrade() {
            area.queue_draw();
        }
    });
    area
}

/// Draws a look on a `w` by `h` card (M5.10c7b), clipped to `corner`: its
/// background, then each part as the lock draws it, in the tokens' colours.
/// The clock and the date are words, the person a glass pill with its disc,
/// the rest frosted rounded rectangles (`edel::lock_screen::arrange`'s places,
/// scaled to the card).
fn draw_look(
    cr: &Context,
    w: f64,
    h: f64,
    parts: &[Placed],
    backdrop: bool,
    corner: f64,
    t: &Tokens,
) {
    let scale = w / f64::from(SCREEN.0);
    clip(cr, w, h, corner);
    if backdrop {
        gradient(
            cr,
            w,
            h,
            BACKDROP_ANGLE,
            t.lock_backdrop,
            t.lock_backdrop_deep,
        );
    } else {
        gradient(cr, w, h, WALLPAPER_ANGLE, t.backdrop, t.backdrop_deep);
        glow(cr, w, h, t.lock_glow);
    }
    for (part, rect) in parts.iter().zip(on_screen(parts)) {
        let Some(r) = rect else {
            continue;
        };
        let rect = (
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
            (Part::Clock, _) => Some(("9:41", 0.82)),
            (Part::Date, _) => Some((SAMPLE_DATE, 0.62)),
            _ => None,
        };
        if let Some((text, share)) = words {
            let alpha = if part.part == Part::Date {
                DATE_ALPHA
            } else {
                1.0
            };
            preview::set(
                cr,
                Colour {
                    a: alpha,
                    ..t.backdrop_text
                },
            );
            draw_words(cr, &t.font, text, rect, share);
            continue;
        }
        match part.part {
            Part::Clock => face(cr, rect, t),
            Part::Person => person(cr, rect, scale, t),
            _ => plate(
                cr,
                rect,
                PART_RADIUS * scale,
                Colour {
                    a: FILL_ALPHA,
                    ..t.backdrop_text
                },
                Colour {
                    a: EDGE_ALPHA,
                    ..t.backdrop_text
                },
            ),
        }
    }
}

/// An analog clock's face: a ring and two hands, in the backdrop text's colour.
fn face(cr: &Context, rect: (f64, f64, f64, f64), t: &Tokens) {
    let (x, y, w, h) = rect;
    preview::set(
        cr,
        Colour {
            a: FACE_ALPHA,
            ..t.backdrop_text
        },
    );
    let radius = w.min(h) / 2.0 * 0.9;
    let (cx, cy) = (x + w / 2.0, y + h / 2.0);
    cr.set_line_width((radius / 10.0).max(1.0));
    cr.arc(cx, cy, radius, 0.0, std::f64::consts::TAU);
    cr.move_to(cx, cy);
    cr.line_to(cx + radius * 0.5, cy - radius * 0.2);
    cr.move_to(cx, cy);
    cr.line_to(cx - radius * 0.1, cy - radius * 0.75);
    let _ = cr.stroke();
}

/// The person's field: a glass pill along the bottom of its rect, and a disc
/// of glass centred over the pill when the rect is at least 2.5 pills tall.
fn person(cr: &Context, rect: (f64, f64, f64, f64), scale: f64, t: &Tokens) {
    let (x, y, w, h) = rect;
    let pill = PILL_H * scale;
    let top = y + h - pill;
    plate(
        cr,
        (x, top, w, pill),
        pill / 2.0,
        t.lock_glass,
        t.lock_glass_edge,
    );
    if h >= 2.5 * pill {
        let r = pill / 2.0;
        let (cx, cy) = (x + w / 2.0, top - pill * 0.3 - r);
        plate(
            cr,
            (cx - r, cy - r, 2.0 * r, 2.0 * r),
            r,
            t.lock_glass,
            t.lock_glass_edge,
        );
    }
}

/// A rounded rectangle of `rect` with corners of `radius`, filled with `fill`
/// and edged with a 1 px line of `edge` just inside it.
fn plate(cr: &Context, rect: (f64, f64, f64, f64), radius: f64, fill: Colour, edge: Colour) {
    let (x, y, w, h) = rect;
    preview::set(cr, fill);
    preview::rounded(cr, x, y, w, h, radius);
    let _ = cr.fill();
    preview::set(cr, edge);
    preview::rounded(
        cr,
        x + 0.5,
        y + 0.5,
        (w - 1.0).max(0.0),
        (h - 1.0).max(0.0),
        (radius - 0.5).max(0.0),
    );
    cr.set_line_width(1.0);
    let _ = cr.stroke();
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

/// Paints the current clip of a `w` by `h` area with a linear gradient from
/// `top` to `deep` along the CSS angle `degrees` (0 up, 90 right): the line
/// runs through the middle, so each corner gets its own end colour.
fn gradient(cr: &Context, w: f64, h: f64, degrees: f64, top: Colour, deep: Colour) {
    let (sin, cos) = degrees.to_radians().sin_cos();
    let (dx, dy) = (sin, -cos);
    let half = (w * dx.abs() + h * dy.abs()) / 2.0;
    let (cx, cy) = (w / 2.0, h / 2.0);
    let gradient = LinearGradient::new(
        cx - dx * half,
        cy - dy * half,
        cx + dx * half,
        cy + dy * half,
    );
    for (offset, c) in [(0.0, top), (1.0, deep)] {
        gradient.add_color_stop_rgba(
            offset,
            f64::from(c.r),
            f64::from(c.g),
            f64::from(c.b),
            f64::from(c.a),
        );
    }
    let _ = cr.set_source(&gradient);
    let _ = cr.paint();
}

/// A soft light at the top left of a wallpaper look: `colour` at GLOW_ALPHA,
/// fading to nothing across the card, so the wallpaper is not flat.
fn glow(cr: &Context, w: f64, h: f64, colour: Colour) {
    let (cx, cy) = (w * 0.2, h * 0.15);
    let light = RadialGradient::new(cx, cy, 0.0, cx, cy, w.max(h));
    let (r, g, b) = (
        f64::from(colour.r),
        f64::from(colour.g),
        f64::from(colour.b),
    );
    light.add_color_stop_rgba(0.0, r, g, b, f64::from(GLOW_ALPHA * colour.a));
    light.add_color_stop_rgba(1.0, r, g, b, 0.0);
    let _ = cr.set_source(&light);
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
    let edit_words = gtk::Box::builder().spacing(6).build();
    edit_words.append(&icon::image("edit", 14));
    edit_words.append(&gtk::Label::new(Some(tr("Customize"))));
    edit.set_child(Some(&edit_words));
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
    let current_drawn = look_picture(
        theme,
        &current_shown,
        96,
        60,
        PICTURE_CORNER * 96.0 / f64::from(CARD_W),
    );
    // A frame so shell-ui's picture can take the drawing's place.
    let current_view = gtk::Overlay::builder()
        .child(&current_drawn)
        .valign(gtk::Align::Center)
        .build();
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
        current_drawn,
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
                .unwrap_or_else(|_| {
                    Err(
                        tr("The desktop did not answer. Log out and in again, then try once more.")
                            .to_string(),
                    )
                });
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
                .and_then(|()| reset_person(&files, WIDGETS))
                .and_then(|()| reset_person(&files, WALLPAPER));
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
            command_for(WIDGETS)
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
            "Give this look another name. It keeps its widgets and its wallpaper, and shows in the cards above.",
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
/// the keyboard to the look in use; asked for by `lock_screen.wallpaper` or
/// `.widgets` it scrolls the look in use to the top and gives the keyboard to
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
        Some(WALLPAPER) | Some(WIDGETS) => {
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
            let set = edel::settings::set("format = 1\n", WIDGETS, &text).unwrap();
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
                dim: None,
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
                dim: None,
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
        let names: Vec<String> = card_list(&lock, &Inputs::default())
            .into_iter()
            .map(|(n, _, _)| n)
            .collect();
        assert_eq!(names, ["classic", "magazine", "minimal"]);
        let own = OwnLook {
            parts: lock_screen::classic(false),
            background: None,
            dim: None,
        };
        let lock = LockScreen {
            looks: Some([("home".to_string(), own)].into_iter().collect()),
            ..LockScreen::default()
        };
        let names: Vec<String> = card_list(&lock, &Inputs::default())
            .into_iter()
            .map(|(n, _, _)| n)
            .collect();
        assert_eq!(names, ["classic", "magazine", "minimal", "home"]);
        assert_eq!(lock_screen::look_title("magazine"), "Magazine");
        assert_eq!(lock_screen::look_title("home"), "Home");
    }

    #[test]
    fn a_card_draws_its_own_look_and_its_background() {
        let own = OwnLook {
            parts: lock_screen::classic(false),
            background: Some("backdrop".to_string()),
            dim: None,
        };
        let lock = LockScreen {
            looks: Some([("home".to_string(), own)].into_iter().collect()),
            ..LockScreen::default()
        };
        let cards = card_list(&lock, &Inputs::default());
        let (_, (_, backdrop), _) = cards.iter().find(|(n, _, _)| n == "home").unwrap();
        assert!(*backdrop, "the look's own navy background");
        let (_, (_, magazine), _) = cards.iter().find(|(n, _, _)| n == "magazine").unwrap();
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
        let classic = lock_screen::classic(false);
        let line = look_line("classic", false, &classic, &Background::Blurred);
        assert!(line.ends_with(" over your wallpaper, blurred."), "{line}");
        assert!(line.starts_with("Clock, date"), "{line}");
        assert!(line.contains(" and "), "{line}");
        let alone: Vec<Placed> = classic
            .iter()
            .filter(|p| p.part == Part::Person)
            .cloned()
            .collect();
        assert_eq!(
            look_line("minimal", false, &alone, &Background::Backdrop),
            "Only the sign in over the navy backdrop."
        );
        assert_eq!(
            look_line("home", true, &classic, &Background::Blurred),
            "Changed over Home on the command line."
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
                    dim: None,
                },
            ),
            (
                "work".to_string(),
                OwnLook {
                    parts: lock_screen::classic(true),
                    background: None,
                    dim: None,
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
                dim: None,
            },
        )]
        .into_iter()
        .collect();
        assert_eq!(lock_screen::looks_value(Some(&own), "home", None), None);
        assert_eq!(renamed(None, "home", "den"), None);
    }

    /// Draws the three built-in looks side by side, each as its card draws
    /// it, into the folder `EDEL_LOOKS_PNG` names. Cairo has no PNG writer
    /// here (as shape.rs notes), so the pictures are written as `looks.pam`,
    /// and ImageMagick's `convert` makes `looks.png` beside it when it is
    /// installed. Gated by the variable, so a normal run skips it.
    #[test]
    fn the_looks_are_written_to_a_file_when_asked() {
        let Some(dir) = std::env::var_os("EDEL_LOOKS_PNG") else {
            return;
        };
        let (cell_w, cell_h) = (320, 200);
        let (w, h) = (cell_w * 3, cell_h);
        let t = Tokens::built_in_scheme(edel::tokens::Scheme::Light);
        let mut surface =
            gtk::cairo::ImageSurface::create(gtk::cairo::Format::ARgb32, w, h).expect("a surface");
        {
            let cr = Context::new(&surface).expect("a context");
            let looks = card_list(&LockScreen::default(), &Inputs::default());
            for (i, (_, (parts, backdrop), _)) in looks.iter().take(3).enumerate() {
                cr.save().expect("save");
                cr.translate(f64::from(cell_w * i as i32), 0.0);
                draw_look(
                    &cr,
                    f64::from(cell_w),
                    f64::from(cell_h),
                    parts,
                    *backdrop,
                    PICTURE_CORNER,
                    &t,
                );
                cr.restore().expect("restore");
            }
        }
        surface.flush();
        let stride = surface.stride() as usize;
        let data = surface.data().expect("the pixels");
        let mut out =
            format!("P7\nWIDTH {w}\nHEIGHT {h}\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR\n")
                .into_bytes();
        for y in 0..h as usize {
            for x in 0..w as usize {
                // ARGB32 in memory is B, G, R, A per pixel, premultiplied.
                let at = y * stride + x * 4;
                let (b, g, r, a) = (data[at], data[at + 1], data[at + 2], data[at + 3]);
                let unpremultiply = |c: u8| -> u8 {
                    if a == 0 {
                        0
                    } else {
                        (u32::from(c) * 255 / u32::from(a)).min(255) as u8
                    }
                };
                out.extend_from_slice(&[unpremultiply(r), unpremultiply(g), unpremultiply(b), a]);
            }
        }
        let pam = std::path::Path::new(&dir).join("looks.pam");
        std::fs::write(&pam, out).expect("the picture");
        let _ = std::process::Command::new("convert")
            .arg(&pam)
            .arg(pam.with_extension("png"))
            .status();
    }

    #[test]
    fn the_pictures_keep_the_shape_of_sixteen_to_ten() {
        assert_eq!((CARD_W, CARD_H), (184, 115));
        assert_eq!(CARD_W * 10, CARD_H * 16);
    }
}
