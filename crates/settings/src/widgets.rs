//! Settings' building blocks (M5.6a): a page with its title and what it
//! is for, a section's heading with where its value comes from, a group
//! of rows on a card, and a row with its title, its line under it and its
//! control. Every page is built of these, so a page holds no look of its
//! own, the look is `style.rs`'s from the tokens, and another toolkit
//! would be these few functions again (ADR-004).

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gdk, gio, glib, graphene};

use edel::i18n::{tr, trf};
use edel::tokens::Colour;

use crate::icon;

/// How wide a page's text and cards grow before they stop and centre,
/// margins included, so lines stay easy to read on a wide screen.
pub const PAGE_WIDTH: i32 = 704;

/// A page: its title, a paragraph saying what it is for and a line that
/// shows a problem when there is one, scrolling, never wider than
/// [`PAGE_WIDTH`]. Sections are appended to the box it gives back.
pub fn page(title: &str, intro: &str) -> (gtk::Widget, gtk::Box, gtk::Label) {
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .margin_top(24)
        .margin_bottom(32)
        .margin_start(32)
        .margin_end(32)
        .build();
    let heading = gtk::Label::builder()
        .label(title)
        .xalign(0.0)
        .css_classes(["edel-page-title"])
        .build();
    let intro = gtk::Label::builder()
        .label(intro)
        .xalign(0.0)
        .wrap(true)
        .visible(!intro.is_empty())
        .css_classes(["edel-intro"])
        .build();
    let problem = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .selectable(true)
        .visible(false)
        .css_classes(["edel-error"])
        .build();
    content.append(&heading);
    content.append(&intro);
    content.append(&problem);
    let clamp = adw::Clamp::builder()
        .maximum_size(PAGE_WIDTH)
        .tightening_threshold(PAGE_WIDTH)
        .child(&content)
        .build();
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&clamp)
        .build();
    // A column, so a page may dock its change bar under what scrolls.
    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    page.append(&scroll);
    (page.upcast(), content, problem)
}

/// The class main puts on a page that was asked for by name
/// (`edel-settings --page NAME`): such a page gives the keyboard to its
/// main control once that shows, so the keys act on it at once, as when the
/// panel's volume opens Sound. Opened from the sidebar, a page leaves the
/// keyboard where the person had it.
pub const ASKED: &str = "edel-asked";

thread_local! {
    /// The row `--page` named by its key or title (`--page panels.list`),
    /// if it named one: its control takes the keyboard, as [`ASKED`] says.
    static ASKED_ROW: std::cell::Cell<Option<&'static str>> = const { std::cell::Cell::new(None) };
}

/// Sets the row `--page` named, or none, when the app starts.
pub fn set_asked_row(key: Option<&'static str>) {
    ASKED_ROW.with(|row| row.set(key));
}

/// The row `--page` named, if it named one (M5.31d).
pub fn asked_row() -> Option<&'static str> {
    ASKED_ROW.with(std::cell::Cell::get)
}

/// Gives the keyboard to `control` if `page` was asked for by name, once.
pub fn take_asked(page: &gtk::Widget, control: &impl IsA<gtk::Widget>) {
    if page.has_css_class(ASKED) {
        page.remove_css_class(ASKED);
        control.grab_focus();
    }
}

/// The bar a page shows once something on it changed (M5.6a): changes
/// apply at once, so the desktop itself is the preview, and the bar
/// offers to put back everything changed since the page opened, or to
/// keep it. It is docked along the page's foot, in the sidebar's colour
/// behind a hairline, so it never lies over a setting and reads as the
/// window's own, not the page's. Save as preset joins it with own presets
/// (M5.17).
pub struct ChangeBar {
    revealer: gtk::Revealer,
    pub undo: gtk::Button,
    pub keep: gtk::Button,
}

impl ChangeBar {
    /// Docks a bar, hidden, along the foot of `page`, made by [`page`].
    pub fn new(page: &gtk::Widget) -> ChangeBar {
        let text = gtk::Label::builder()
            .label(tr("Your changes are live"))
            .xalign(0.0)
            .css_classes(["edel-change-text"])
            .build();
        let detail = gtk::Label::builder()
            .label(tr(
                "Undo puts this page back as it was when Settings opened.",
            ))
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .hexpand(true)
            .css_classes(["edel-change-detail"])
            .build();
        let words = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .valign(gtk::Align::Center)
            .hexpand(true)
            .build();
        words.append(&text);
        words.append(&detail);
        let undo = gtk::Button::builder()
            .label(tr("Undo"))
            .tooltip_text(tr("Put this page back as it was when Settings opened"))
            .css_classes(["edel-change-undo"])
            .focus_on_click(false)
            .build();
        let keep = gtk::Button::builder()
            .label(tr("Keep"))
            .css_classes(["edel-change-keep"])
            .focus_on_click(false)
            .build();
        let bar = gtk::Box::builder()
            .spacing(10)
            .css_classes(["edel-change-bar"])
            .build();
        let mark = icon::image("check", 14);
        mark.add_css_class("edel-change-mark");
        mark.set_valign(gtk::Align::Center);
        bar.append(&mark);
        bar.append(&words);
        bar.append(&undo);
        bar.append(&keep);
        bar.update_property(&[gtk::accessible::Property::Label(tr(
            "Your changes are live",
        ))]);
        let revealer = gtk::Revealer::builder()
            .child(&bar)
            .transition_type(gtk::RevealerTransitionType::SlideUp)
            .transition_duration(160)
            .build();
        if let Some(column) = page.downcast_ref::<gtk::Box>() {
            column.append(&revealer);
        }
        ChangeBar {
            revealer,
            undo,
            keep,
        }
    }

    pub fn show(&self, shown: bool) {
        self.revealer.set_reveal_child(shown);
    }
}

/// Where a value comes from, and the two things a person may do with it.
pub struct Source {
    /// `Automatic (Classic)`, `Your choice` or `Set by this machine`.
    pub label: gtk::Label,
    /// Takes the person's own choice out (M5.6b).
    pub reset: gtk::Button,
    /// Copies the `edel settings set` line for the value now.
    pub copy: gtk::Button,
}

/// A section's heading in `content`: its title, then where its value
/// comes from with Reset and Copy as command at its end.
pub fn section(content: &gtk::Box, title: &str) -> Source {
    let heading = heading(content, title);
    let source = source();
    heading.append(&source.label);
    heading.append(&source.reset);
    heading.append(&source.copy);
    source
}

/// A section's heading with only its title, for a group of rows that each
/// say their own source.
pub fn heading(content: &gtk::Box, title: &str) -> gtk::Box {
    let heading = gtk::Box::builder()
        .spacing(6)
        .margin_top(26)
        .margin_bottom(10)
        .build();
    let label = gtk::Label::builder()
        .label(title)
        .xalign(0.0)
        .hexpand(true)
        .css_classes(["edel-section"])
        .build();
    heading.append(&label);
    content.append(&heading);
    heading
}

/// A step's heading: its number in a small accent disc, then its title,
/// for pages a person goes through in order (the installer).
pub fn step_heading(content: &gtk::Box, number: u32, title: &str) -> gtk::Box {
    let heading = gtk::Box::builder()
        .spacing(10)
        .margin_top(30)
        .margin_bottom(10)
        .build();
    let disc = gtk::Label::builder()
        .label(number.to_string())
        .width_request(22)
        .height_request(22)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .xalign(0.5)
        .css_classes(["edel-step-number"])
        .build();
    let label = gtk::Label::builder()
        .label(title)
        .xalign(0.0)
        .hexpand(true)
        .css_classes(["edel-section"])
        .build();
    heading.append(&disc);
    heading.append(&label);
    content.append(&heading);
    heading
}

pub fn source() -> Source {
    let label = gtk::Label::builder()
        .xalign(1.0)
        .css_classes(["edel-source"])
        .build();
    let reset = gtk::Button::builder()
        .label(tr("Reset"))
        .tooltip_text(tr(
            "Take your choice out, so this machine or the release decides",
        ))
        .valign(gtk::Align::Center)
        .css_classes(["edel-link"])
        .build();
    let copy = gtk::Button::builder()
        .child(&icon::image("copy", 12))
        .tooltip_text(tr("Copy as command"))
        .valign(gtk::Align::Center)
        .css_classes(["edel-copy"])
        .build();
    copy.update_property(&[gtk::accessible::Property::Label(tr("Copy as command"))]);
    Source { label, reset, copy }
}

/// A card of rows in `content`; rows are appended to the box it gives.
pub fn group(content: &gtk::Box) -> gtk::Box {
    let group = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .css_classes(["edel-group"])
        .build();
    content.append(&group);
    group
}

/// One row of a group: its title with the keys that do the same, the line
/// under it (what it does, then where its value comes from), Reset, Copy
/// as command and its control.
pub struct Row {
    pub subtitle: gtk::Label,
    pub keys: gtk::Box,
    pub reset: gtk::Button,
    pub copy: gtk::Button,
    /// Room for an icon before the title, empty and hidden until
    /// [`Row::set_icon`] fills it.
    lead: gtk::Box,
}

impl Row {
    /// A row's parts for a page that lays its row out itself, as the Shape
    /// group's sliders do; it has no room for an icon.
    pub fn of(subtitle: gtk::Label, keys: gtk::Box, reset: gtk::Button, copy: gtk::Button) -> Row {
        Row {
            subtitle,
            keys,
            reset,
            copy,
            lead: gtk::Box::builder().visible(false).build(),
        }
    }

    /// Shows one of our icons (`design/icons/`) before the row's title, in
    /// the title's colour, such as a permission's on an app's page (M6.14a).
    pub fn set_icon(&self, name: &str) {
        while let Some(child) = self.lead.first_child() {
            self.lead.remove(&child);
        }
        self.lead.append(&crate::icon::image(name, 20));
        self.lead.set_visible(true);
    }
}

/// Appends a row titled `title` that changes `control` to `group`.
pub fn row(group: &gtk::Box, title: &str, control: &gtk::Widget) -> Row {
    let row = gtk::Box::builder()
        .spacing(10)
        .css_classes(["edel-row"])
        .build();
    let words = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build();
    let title_label = gtk::Label::builder()
        .label(title)
        .xalign(0.0)
        .wrap(true)
        .css_classes(["edel-row-title"])
        .build();
    let keys = gtk::Box::builder()
        .spacing(3)
        .valign(gtk::Align::Center)
        .visible(false)
        .build();
    let heading = gtk::Box::builder().spacing(8).build();
    heading.append(&title_label);
    heading.append(&keys);
    // The line under the title is there only while it has words, so a row
    // without one keeps its title centred on its control (M5.12b6c).
    let subtitle = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .visible(false)
        .css_classes(["edel-row-subtitle"])
        .build();
    subtitle.connect_label_notify(|label| label.set_visible(!label.label().is_empty()));
    words.append(&heading);
    words.append(&subtitle);
    let lead = gtk::Box::builder()
        .valign(gtk::Align::Center)
        .visible(false)
        .build();
    row.append(&lead);
    row.append(&words);
    let Source { reset, copy, .. } = source();
    row.append(&reset);
    row.append(&copy);
    control.set_valign(gtk::Align::Center);
    row.append(control);
    control.update_property(&[gtk::accessible::Property::Label(title)]);
    group.append(&row);
    Row {
        subtitle,
        keys,
        reset,
        copy,
        lead,
    }
}

/// A row that shows a value people may select and copy, as About's.
pub fn value_row(group: &gtk::Box, title: &str, value: &str) {
    value_row_in(group, title, value, true);
}

/// A row that shows a value as it is, not to be selected: a page whose
/// first row would otherwise take the keyboard with its text selected, as
/// a Compact window shows no sidebar to give it to (Power's batteries,
/// Users' people, M5.8b).
pub fn fact_row(group: &gtk::Box, title: &str, value: &str) {
    value_row_in(group, title, value, false);
}

fn value_row_in(group: &gtk::Box, title: &str, value: &str, selectable: bool) {
    let row = gtk::Box::builder()
        .spacing(10)
        .css_classes(["edel-row"])
        .build();
    let title_label = gtk::Label::builder()
        .label(title)
        .xalign(0.0)
        .css_classes(["edel-row-title"])
        .build();
    // The value takes the room the title leaves and only then wraps, so a
    // short one never breaks beside a long title.
    let value_label = gtk::Label::builder()
        .label(value)
        .xalign(1.0)
        .hexpand(true)
        .justify(gtk::Justification::Right)
        .selectable(selectable)
        .wrap(true)
        .css_classes(["edel-value"])
        .build();
    row.append(&title_label);
    row.append(&value_label);
    group.append(&row);
}

/// A row of plain words in `group`, such as what to do when there is
/// nothing to show.
pub fn text_row(group: &gtk::Box, text: &str) {
    let label = gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .wrap(true)
        .selectable(true)
        .css_classes(["edel-row", "edel-row-subtitle"])
        .build();
    group.append(&label);
}

/// A row of plain words that is only read, as [`text_row`] but not to be
/// selected, so it never takes the keyboard with its text highlighted when
/// a window too narrow for a sidebar opens on its page (M5.8b).
pub fn note_row(group: &gtk::Box, text: &str) {
    let label = gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .wrap(true)
        .css_classes(["edel-row", "edel-row-subtitle"])
        .build();
    group.append(&label);
}

/// A button for something to do now, not a setting to choose: Save as,
/// Choose file, Apply (M5.8c). `main` marks the one a page leads to.
pub fn action(label: &str, main: bool) -> gtk::Button {
    let button = gtk::Button::builder()
        .label(label)
        .valign(gtk::Align::Center)
        .css_classes(["edel-action"])
        .build();
    if main {
        button.add_css_class("edel-action-main");
    }
    button
}

/// A big button for the thing a page leads to, as an updates page has its
/// Check for updates: tall, `main` filled with the accent, the others
/// outlined (M5.8d).
pub fn big_button(label: &str, main: bool) -> gtk::Button {
    let button = gtk::Button::builder()
        .label(label)
        .css_classes(["edel-big"])
        .build();
    if main {
        button.add_css_class("edel-big-main");
    }
    button
}

/// The card at the head of a page that is about one state, as an updates
/// page tells whether the system is up to date (M5.8d): a big icon or a
/// spinner, a headline in plain words, a line under it, and big buttons
/// below, which wrap on a narrow window.
pub struct Hero {
    card: gtk::Box,
    badge: gtk::Box,
    icon: gtk::Image,
    spinner: gtk::Spinner,
    title: gtk::Label,
    sub: gtk::Label,
    buttons: gtk::Box,
}

/// The card a hero and About's name sit on, appended to `content`.
pub fn hero_card(content: &gtk::Box) -> gtk::Box {
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .margin_top(18)
        .css_classes(["edel-hero"])
        .build();
    content.append(&card);
    card
}

/// A row of big buttons that wraps when the window is narrow.
pub fn button_flow() -> gtk::FlowBox {
    gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .homogeneous(false)
        // The card's whole width, so the row and its height are measured
        // at the same width; a button wraps when the next does not fit.
        .min_children_per_line(1)
        .max_children_per_line(4)
        .column_spacing(12)
        .row_spacing(10)
        .margin_top(20)
        .css_classes(["edel-buttons"])
        .build()
}

/// The card at the head of About (M5.8d): the system's name large, its
/// version under it, and a row of big buttons, which [`add_big`] fills.
pub fn banner(content: &gtk::Box, name: &str, version: &str) -> gtk::FlowBox {
    let card = hero_card(content);
    card.append(
        &gtk::Label::builder()
            .label(name)
            .xalign(0.0)
            .wrap(true)
            .css_classes(["edel-name"])
            .build(),
    );
    card.append(
        &gtk::Label::builder()
            .label(version)
            .xalign(0.0)
            .wrap(true)
            .margin_top(2)
            .css_classes(["edel-name-version"])
            .build(),
    );
    let buttons = button_flow();
    card.append(&buttons);
    buttons
}

/// A big button added to the row of a [`banner`].
pub fn add_big(buttons: &gtk::FlowBox, label: &str, main: bool) -> gtk::Button {
    let button = big_button(label, main);
    add_to_flow(buttons, &button);
    button
}

/// Appends `button` to a [`button_flow`] at its own width, not stretched
/// to fill what the row has left.
fn add_to_flow(flow: &gtk::FlowBox, button: &gtk::Button) {
    flow.append(button);
}

/// The hero's badge, in logical pixels, and the icon in it.
const HERO_BADGE: i32 = 64;
const HERO_ICON: i32 = 28;

impl Hero {
    /// Appends an empty hero to `content`, as the board draws it: the icon
    /// in a round badge, the headline and the line under it, centred, and
    /// the buttons under those.
    pub fn new(content: &gtk::Box) -> Hero {
        let icon = icon::image("status-ok", HERO_ICON);
        icon.add_css_class("edel-hero-icon");
        icon.set_hexpand(true);
        icon.set_halign(gtk::Align::Center);
        icon.set_valign(gtk::Align::Center);
        let spinner = gtk::Spinner::builder()
            .width_request(HERO_ICON)
            .height_request(HERO_ICON)
            .visible(false)
            .hexpand(true)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .css_classes(["edel-hero-icon"])
            .build();
        let badge = gtk::Box::builder()
            .width_request(HERO_BADGE)
            .height_request(HERO_BADGE)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .css_classes(["edel-hero-badge", "edel-tone-accent"])
            .build();
        badge.append(&icon);
        badge.append(&spinner);
        let title = gtk::Label::builder()
            .xalign(0.5)
            .justify(gtk::Justification::Center)
            .wrap(true)
            .css_classes(["edel-hero-title"])
            .build();
        let sub = gtk::Label::builder()
            .xalign(0.5)
            .justify(gtk::Justification::Center)
            .wrap(true)
            .css_classes(["edel-hero-sub"])
            .build();
        // Shown with its first button, so a hero without one ends at its
        // line.
        let buttons = gtk::Box::builder()
            .halign(gtk::Align::Center)
            .margin_top(22)
            .visible(false)
            .build();
        let card = hero_card(content);
        card.set_spacing(10);
        card.append(&badge);
        card.append(&title);
        card.append(&sub);
        card.append(&buttons);
        Hero {
            card,
            badge,
            icon,
            spinner,
            title,
            sub,
            buttons,
        }
    }

    /// Drops the card's round background, border and padding: the hero sits
    /// on the page itself, as the installer's does.
    pub fn set_bare(&self) {
        self.card.add_css_class("edel-hero-bare");
    }

    /// Adds a button under the line, after the others.
    pub fn add(&self, button: &gtk::Button) {
        self.buttons.append(button);
        self.buttons.set_visible(true);
    }

    /// Shows `icon` (or a spinner when `busy`), the headline and the line
    /// under it. The badge takes the colour of what the icon says: green
    /// when it is ok, the accent for a new version or a sync, amber for a
    /// restart and red for a problem.
    pub fn show(&self, icon_name: &str, busy: bool, problem: bool, title: &str, sub: &str) {
        // Inside the round badge an ok is the bare check, as the board
        // draws it; the status icons' own circle would be a ring in a ring.
        let shown = if icon_name == "status-ok" {
            "check"
        } else {
            icon_name
        };
        self.icon
            .set_paintable(Some(&icon::Icon::new(shown, HERO_ICON)));
        self.icon.set_visible(!busy);
        self.spinner.set_visible(busy);
        self.spinner.set_spinning(busy);
        self.title.set_label(title);
        self.sub.set_label(sub);
        let tone = if problem {
            "edel-tone-problem"
        } else {
            match icon_name {
                "status-ok" => "edel-tone-ok",
                "status-restart" => "edel-tone-restart",
                _ => "edel-tone-accent",
            }
        };
        for class in [
            "edel-tone-ok",
            "edel-tone-accent",
            "edel-tone-restart",
            "edel-tone-problem",
        ] {
            self.badge.remove_css_class(class);
        }
        self.badge.add_css_class(tone);
        self.card
            .update_property(&[gtk::accessible::Property::Label(title)]);
    }
}

/// The primary button of a page's hero: an accent pill, as the board
/// draws Check for updates.
pub fn hero_button(label: &str) -> gtk::Button {
    gtk::Button::builder()
        .label(label)
        .css_classes(["edel-hero-button"])
        .build()
}

/// A fold-out in `content` that starts closed, for what only some people
/// want to read; `child` is shown when it opens. The title is its heading.
pub fn disclosure(content: &gtk::Box, title: &str, child: &impl IsA<gtk::Widget>) -> gtk::Expander {
    let expander = gtk::Expander::builder()
        .label(title)
        .child(child)
        .margin_top(26)
        .css_classes(["edel-details"])
        .build();
    content.append(&expander);
    expander
}

/// A row in `group` for what a command said, hidden until [`say`] gives it
/// words: the output as it is, in a fixed-width font so a diff lines up,
/// selectable so it can be copied, and in the problem colour when it is a
/// failure.
pub fn output_row(group: &gtk::Box) -> gtk::Label {
    let label = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .selectable(true)
        .visible(false)
        .css_classes(["edel-row", "edel-output", "monospace"])
        .build();
    group.append(&label);
    label
}

/// Shows `text` in a row made by [`output_row`] (none hides it), as an
/// error when `failed`.
pub fn say(row: &gtk::Label, text: &str, failed: bool) {
    row.set_label(text);
    row.set_visible(!text.is_empty());
    if failed {
        row.add_css_class("edel-error");
    } else {
        row.remove_css_class("edel-error");
    }
}

/// Asks which file, in the system's file chooser: one to save under `name`
/// when it is given, else one to open. None when the person cancels.
pub async fn pick_file(
    near: &gtk::Widget,
    title: &str,
    name: Option<&str>,
) -> Option<std::path::PathBuf> {
    let dialog = gtk::FileDialog::builder().title(title).modal(true).build();
    let window = near.root().and_downcast::<gtk::Window>();
    let file = match name {
        Some(name) => {
            dialog.set_initial_name(Some(name));
            dialog.save_future(window.as_ref()).await
        }
        None => dialog.open_future(window.as_ref()).await,
    };
    file.ok()?.path()
}

/// Asks for pictures, several at once, in the system's file chooser
/// (M5.10c4c, M5.10c5c): `done` gets their paths, or None when the person
/// cancels. Wallpaper's Your picture and the lock screen's Picture share it.
pub fn choose_pictures(near: &gtk::Widget, done: impl FnOnce(Option<Vec<PathBuf>>) + 'static) {
    let near = near.clone();
    glib::spawn_future_local(async move {
        let dialog = gtk::FileDialog::builder()
            .title(tr("Choose a picture"))
            .modal(true)
            .build();
        let window = near.root().and_downcast::<gtk::Window>();
        let picked = dialog
            .open_multiple_future(window.as_ref())
            .await
            .ok()
            .map(|files| {
                files
                    .iter::<gio::File>()
                    .filter_map(Result::ok)
                    .filter_map(|file| file.path())
                    .collect::<Vec<PathBuf>>()
            });
        done(picked);
    });
}

/// Asks for a colour in the system's dialog, starting from `start`, and
/// gives it as `#rrggbb` (M5.10c4c, M5.10c5c): `done` gets None when the
/// person cancels. The Wallpaper card and the lock screen's Colour share it.
pub fn choose_colour(
    near: &gtk::Widget,
    start: Colour,
    done: impl FnOnce(Option<String>) + 'static,
) {
    let initial = start.hex().parse::<gdk::RGBA>().ok();
    let window = near.root().and_downcast::<gtk::Window>();
    gtk::ColorDialog::new().choose_rgba(
        window.as_ref(),
        initial.as_ref(),
        None::<&gio::Cancellable>,
        move |picked| done(picked.ok().map(|rgba| hex_of(&rgba))),
    );
}

/// A colour as `#rrggbb`, lowercase, each channel rounded to 0 to 255.
fn hex_of(c: &gdk::RGBA) -> String {
    let channel = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "#{:02x}{:02x}{:02x}",
        channel(c.red()),
        channel(c.green()),
        channel(c.blue())
    )
}

/// A row in `group` that opens to show `text` in a read-only box, in a
/// fixed-width font and scrolled to its end, as About's session log is:
/// the newest line is the one people need.
pub fn log_row(group: &gtk::Box, title: &str, text: &str) {
    let view = gtk::TextView::builder()
        .editable(false)
        .cursor_visible(false)
        .monospace(true)
        .wrap_mode(gtk::WrapMode::WordChar)
        .build();
    view.buffer().set_text(text);
    view.update_property(&[gtk::accessible::Property::Label(title)]);
    view.connect_map(|view| {
        let buffer = view.buffer();
        let mark = buffer.create_mark(None, &buffer.end_iter(), false);
        view.scroll_to_mark(&mark, 0.0, false, 0.0, 1.0);
    });
    let scroll = gtk::ScrolledWindow::builder()
        .min_content_height(220)
        .max_content_height(360)
        .propagate_natural_height(true)
        .child(&view)
        .build();
    let expander = gtk::Expander::builder()
        .label(title)
        .child(&scroll)
        .css_classes(["edel-row"])
        .build();
    group.append(&expander);
}

/// A choice drawn as a picture (M5.10c4c): the picture above its name, and
/// a check when it is the one chosen. The layout's presets and the
/// wallpapers share it, so the cards look the same.
pub fn picture_card(title: &str, picture: &gtk::Widget) -> (gtk::ToggleButton, gtk::Box) {
    let label = gtk::Label::builder()
        .label(title)
        .xalign(0.0)
        .hexpand(true)
        .css_classes(["edel-preset-name"])
        .build();
    let check = icon::image("check", 12);
    check.set_hexpand(true);
    check.set_halign(gtk::Align::Center);
    let badge = gtk::Box::builder()
        .valign(gtk::Align::Center)
        .halign(gtk::Align::End)
        .hexpand(false)
        .css_classes(["edel-badge"])
        .build();
    badge.append(&check);
    let name_row = gtk::Box::builder().spacing(6).build();
    name_row.append(&label);
    name_row.append(&badge);
    let inner = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .build();
    inner.append(picture);
    inner.append(&name_row);
    let card = gtk::ToggleButton::builder()
        .child(&inner)
        .css_classes(["edel-preset"])
        .build();
    card.update_property(&[gtk::accessible::Property::Label(title)]);
    (card, badge)
}

/// A small pill of an icon and a word: In use (a check, in the accent) after
/// a look's or a theme's name, Built in (a lock, frosted) over a built-in
/// one's picture.
pub fn pill(icon_name: &str, words: &str, class: &str) -> gtk::Box {
    let pill = gtk::Box::builder()
        .spacing(4)
        .valign(gtk::Align::Center)
        .css_classes([class])
        .build();
    pill.append(&icon::image(icon_name, 10));
    pill.append(&gtk::Label::new(Some(words)));
    pill
}

/// A flat button of a popover menu (the More menus).
pub fn menu_item(label: &str) -> gtk::Button {
    gtk::Button::builder()
        .label(label)
        .halign(gtk::Align::Fill)
        .css_classes(["edel-menu-item"])
        .build()
}

/// Asks for a name in a small dialog over `near`'s window: `text` above an
/// entry holding `current`, Cancel and `action`. `check` gives the name's
/// problem, shown under the entry, or Ok, when the dialog closes.
pub fn ask_name(
    near: &gtk::Widget,
    title: &str,
    text: &str,
    current: &str,
    placeholder: &str,
    save_label: &str,
    check: impl Fn(&str) -> Result<(), String> + 'static,
) {
    let window = gtk::Window::builder()
        .title(title)
        .modal(true)
        .resizable(false)
        .default_width(360)
        .build();
    if let Some(parent) = near.root().and_downcast::<gtk::Window>() {
        window.set_transient_for(Some(&parent));
    }
    let entry = gtk::Entry::builder()
        .text(current)
        .placeholder_text(placeholder)
        .activates_default(true)
        .build();
    entry.update_property(&[gtk::accessible::Property::Label(title)]);
    let error = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .visible(false)
        .css_classes(["edel-error"])
        .build();
    let text = gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .wrap(true)
        .build();
    let cancel = action(tr("Cancel"), false);
    let save = action(save_label, true);
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
    let (typed, shown, dialog) = (entry.clone(), error.clone(), window.clone());
    save.connect_clicked(move |_| match check(&typed.text()) {
        Ok(()) => dialog.close(),
        Err(why) => {
            shown.set_label(&why);
            shown.set_visible(true);
        }
    });
    let save_again = save.clone();
    entry.connect_activate(move |_| save_again.emit_clicked());
    window.present();
    entry.grab_focus();
    entry.select_region(0, -1);
}

/// Asks whether to delete `message` (Delete), with Cancel as the default and
/// the escape key, and runs `delete` on Delete.
pub fn ask_delete(
    near: &gtk::Widget,
    message: &str,
    detail: &str,
    delete: &str,
    done: impl FnOnce() + 'static,
) {
    let parent = near.root().and_downcast::<gtk::Window>();
    let dialog = gtk::AlertDialog::builder()
        .modal(true)
        .message(message)
        .detail(detail)
        .buttons(vec![tr("Cancel"), delete])
        .cancel_button(0)
        .default_button(0)
        .build();
    dialog.choose(parent.as_ref(), gio::Cancellable::NONE, move |answer| {
        if matches!(answer, Ok(1)) {
            done();
        }
    });
}

/// Shows that `button` copied: its icon a check for a moment.
pub fn copied(button: &gtk::Button) {
    button.set_child(Some(&icon::image("check", 12)));
    let button = button.downgrade();
    gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(1200), move || {
        if let Some(button) = button.upgrade() {
            button.set_child(Some(&icon::image("copy", 12)));
        }
    });
}

/// A title bar drawn as the compositor draws every window's, in a small
/// window at the top of a group, for a page whose choices change it: the
/// title in its middle and the shown buttons on their side, close
/// outermost (M5.18a). Our own icons, so it is the bar people will see.
pub struct BarPreview {
    start: gtk::Box,
    end: gtk::Box,
    /// Minimize, maximize and close.
    buttons: [gtk::Box; 3],
}

impl BarPreview {
    /// Appends the preview, titled `title`, to `group`.
    pub fn new(group: &gtk::Box, title: &str) -> BarPreview {
        let button = |name: &str| {
            let b = gtk::Box::builder()
                .css_classes(["edel-bar-button"])
                .valign(gtk::Align::Center)
                .build();
            b.append(&icon::image(name, 12));
            b
        };
        let buttons = [button("minimize"), button("maximize"), button("close")];
        let start = gtk::Box::builder().spacing(2).build();
        let end = gtk::Box::builder().spacing(2).build();
        let bar = gtk::CenterBox::builder()
            .css_classes(["edel-bar"])
            .start_widget(&start)
            .center_widget(&gtk::Label::new(Some(title)))
            .end_widget(&end)
            .build();
        let body = gtk::Box::builder().css_classes(["edel-mini-body"]).build();
        let window = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .width_request(320)
            .halign(gtk::Align::Center)
            .css_classes(["edel-mini-window"])
            .build();
        window.append(&bar);
        window.append(&body);
        let row = gtk::Box::builder()
            .css_classes(["edel-row", "edel-preview-row"])
            .build();
        row.append(&window);
        window.set_hexpand(true);
        // A picture, not a control: screen readers hear the rows below.
        row.set_can_target(false);
        row.update_property(&[gtk::accessible::Property::Label(tr(
            "How every window's title bar will look",
        ))]);
        group.append(&row);
        BarPreview {
            start,
            end,
            buttons,
        }
    }

    /// Shows the buttons `shown` (minimize, maximize, close) on the left
    /// or the right, close outermost.
    pub fn show(&self, left: bool, shown: [bool; 3]) {
        for side in [&self.start, &self.end] {
            while let Some(child) = side.first_child() {
                side.remove(&child);
            }
        }
        let [minimize, maximize, close] = &self.buttons;
        let order = if left {
            [
                (close, shown[2]),
                (minimize, shown[0]),
                (maximize, shown[1]),
            ]
        } else {
            [
                (minimize, shown[0]),
                (maximize, shown[1]),
                (close, shown[2]),
            ]
        };
        let side = if left { &self.start } else { &self.end };
        for (button, on) in order {
            if on {
                side.append(button);
            }
        }
    }
}

/// Shows `keys`, such as `Super+Q`, as small keycaps in `place`, one per
/// key, or hides it for none; screen readers hear them as one shortcut.
pub fn show_keys(place: &gtk::Box, keys: Option<&str>) {
    while let Some(child) = place.first_child() {
        place.remove(&child);
    }
    let Some(keys) = keys.filter(|k| !k.is_empty()) else {
        place.set_visible(false);
        return;
    };
    for key in keys.split('+') {
        place.append(
            &gtk::Label::builder()
                .label(key)
                .css_classes(["edel-key"])
                .build(),
        );
    }
    place.update_property(&[gtk::accessible::Property::Label(&trf(
        "Shortcut {keys}",
        &[("keys", keys)],
    ))]);
    place.set_tooltip_text(Some(&trf("Shortcut: {keys}", &[("keys", keys)])));
    place.set_visible(true);
}

/// A value chosen from a few, as the mockups draw it: the value and our
/// up-down chevrons in a small box, opening a list with a check on the
/// chosen one. Ours rather than GTK's drop-down, whose arrow could only
/// be restyled with a picture.
pub struct Choice {
    button: gtk::MenuButton,
    label: gtk::Label,
    checks: Vec<gtk::Image>,
    labels: Vec<String>,
    selected: std::cell::Cell<usize>,
    changed: std::cell::RefCell<Vec<Box<dyn Fn()>>>,
}

impl Choice {
    /// A choice of `labels`, the first chosen.
    pub fn new(labels: Vec<String>) -> std::rc::Rc<Choice> {
        build(labels, false)
    }

    /// A choice of `labels` whose list a search field filters, for a long
    /// list such as the languages or the keyboard layouts: Return takes the
    /// first row left, and the search clears each time the list opens.
    pub fn searchable(labels: Vec<String>) -> std::rc::Rc<Choice> {
        build(labels, true)
    }

    pub fn widget(&self) -> gtk::Widget {
        self.button.clone().upcast()
    }

    /// The words of the value chosen now.
    pub fn chosen_text(&self) -> String {
        self.label.label().to_string()
    }

    /// Gives the choice a width of `width` logical pixels, its value cut
    /// short with an ellipsis at the end when it is longer.
    pub fn set_width(&self, width: i32) {
        self.button.set_width_request(width);
        self.label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        self.label.set_hexpand(true);
        self.label.set_xalign(0.0);
    }

    pub fn selected(&self) -> usize {
        self.selected.get()
    }

    /// Shows choice `at` as chosen, without calling back.
    pub fn set_selected(&self, at: usize) {
        self.selected.set(at);
        self.show(at);
    }

    fn show(&self, at: usize) {
        if let Some(text) = self.labels.get(at) {
            self.label.set_label(text);
        }
        for (i, check) in self.checks.iter().enumerate() {
            check.set_opacity(if i == at { 1.0 } else { 0.0 });
        }
    }

    /// Calls `f` when a person picks a value.
    pub fn connect_changed(&self, f: impl Fn() + 'static) {
        self.changed.borrow_mut().push(Box::new(f));
    }
}

/// The one builder behind [`Choice::new`] and [`Choice::searchable`]: the
/// list of `labels`, with a search field above it when `search` is set.
fn build(labels: Vec<String>, search: bool) -> std::rc::Rc<Choice> {
    let label = gtk::Label::new(labels.first().map(String::as_str));
    let inside = gtk::Box::builder().spacing(6).build();
    inside.append(&label);
    inside.append(&icon::image("updown", 12));
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["edel-choices"])
        .build();
    let mut checks = Vec::new();
    for text in &labels {
        let row = gtk::Box::builder().spacing(8).build();
        let check = icon::image("check", 12);
        row.append(&check);
        row.append(&gtk::Label::builder().label(text).xalign(0.0).build());
        list.append(&row);
        checks.push(check);
    }
    let popover = gtk::Popover::builder()
        .has_arrow(false)
        .css_classes(["edel-choice-list"])
        .build();
    let mut search_entry = None;
    if search {
        let needle = Rc::new(std::cell::RefCell::new(String::new()));
        let entry = gtk::SearchEntry::builder()
            .placeholder_text(tr("Search"))
            .css_classes(["edel-entry"])
            .build();
        let shown = labels.clone();
        let filter = Rc::clone(&needle);
        list.set_filter_func(move |row| {
            let needle = filter.borrow();
            usize::try_from(row.index())
                .ok()
                .and_then(|at| shown.get(at))
                .is_some_and(|text| text.to_lowercase().contains(needle.as_str()))
        });
        let typed = Rc::clone(&needle);
        let filtered = list.clone();
        entry.connect_search_changed(move |entry| {
            *typed.borrow_mut() = entry.text().to_lowercase();
            filtered.invalidate_filter();
        });
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .max_content_height(320)
            .propagate_natural_height(true)
            .child(&list)
            .build();
        let column = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .build();
        column.append(&entry);
        column.append(&scroll);
        popover.set_child(Some(&column));
        search_entry = Some(entry);
    } else {
        popover.set_child(Some(&list));
    }
    let button = gtk::MenuButton::builder()
        .child(&inside)
        .popover(&popover)
        .css_classes(["edel-choice"])
        .build();
    let choice = std::rc::Rc::new(Choice {
        button,
        label,
        checks,
        labels,
        selected: std::cell::Cell::new(0),
        changed: std::cell::RefCell::new(Vec::new()),
    });
    choice.show(0);
    let weak = std::rc::Rc::downgrade(&choice);
    list.connect_row_activated({
        let (weak, popover) = (weak.clone(), popover.clone());
        move |_, row| choose(&weak, &popover, row.index().max(0) as usize)
    });
    if let Some(entry) = search_entry {
        // Return takes the first row the search leaves showing.
        entry.connect_activate({
            let (weak, popover, list) = (weak.clone(), popover.clone(), list.clone());
            move |_| {
                let mut at = 0;
                while let Some(row) = list.row_at_index(at) {
                    if row.is_child_visible() {
                        choose(&weak, &popover, at.max(0) as usize);
                        return;
                    }
                    at += 1;
                }
            }
        });
        // Each time the list opens it starts with no search, ready to type.
        popover.connect_show(move |_| {
            entry.set_text("");
            entry.grab_focus();
        });
    }
    choice
}

/// Chooses row `at` of a choice: shows it, shuts its list and tells the
/// callers, as a person's pick does.
fn choose(weak: &std::rc::Weak<Choice>, popover: &gtk::Popover, at: usize) {
    let Some(choice) = weak.upgrade() else { return };
    choice.set_selected(at);
    popover.popdown();
    for f in choice.changed.borrow().iter() {
        f();
    }
}

/// The space left above a group the page scrolls to, in logical pixels.
const SCROLL_MARGIN: f64 = 24.0;

/// `child` in a box that tells `fold` whether it is narrower than the
/// window's breakpoint (`crate::NARROW_SP`), once at the start and again
/// each time that changes, so a group can lay itself out for the width it
/// is given. Not an `adw::BreakpointBin`: that has no height of its own,
/// so on a page taller than the window GTK gave it none and the group did
/// not show (the Shape sliders and the Lock screen's card, 2026-10-11).
/// The width is read by a drawing area over the child that draws nothing
/// and takes no clicks; a change is laid out after the frame that saw it.
pub fn folding(child: &impl IsA<gtk::Widget>, fold: impl Fn(bool) + 'static) -> gtk::Overlay {
    let overlay = gtk::Overlay::builder().hexpand(true).child(child).build();
    let sensor = gtk::DrawingArea::builder()
        .can_target(false)
        .can_focus(false)
        .build();
    overlay.add_overlay(&sensor);
    overlay.set_measure_overlay(&sensor, false);
    let narrow: Rc<Cell<Option<bool>>> = Rc::new(Cell::new(None));
    let fold = Rc::new(fold);
    sensor.connect_resize(move |area, width, _| {
        // sp follows the text size the person chose, as the window's does.
        let dpi = area.settings().gtk_xft_dpi();
        let scale = if dpi > 0 {
            f64::from(dpi) / (96.0 * 1024.0)
        } else {
            1.0
        };
        let now = f64::from(width) < crate::NARROW_SP * scale;
        if narrow.replace(Some(now)) != Some(now) {
            let fold = fold.clone();
            glib::idle_add_local_once(move || fold(now));
        }
    });
    overlay
}

/// Scrolls the page so `widget` lies near the top of its view (M5.31d, M5.2n):
/// a group asked for by name is shown whole, its heading first, even on a
/// window too small for the page.
pub fn scroll_to_top(widget: &gtk::Widget) {
    let Some(scroll) = widget
        .ancestor(gtk::ScrolledWindow::static_type())
        .and_downcast::<gtk::ScrolledWindow>()
    else {
        return;
    };
    let Some(at) = widget.compute_bounds(&scroll) else {
        return;
    };
    let adjustment = scroll.vadjustment();
    adjustment.set_value(adjustment.value() + f64::from(at.y()) - SCROLL_MARGIN);
}

/// A widget's place in the window as CI reads it, `X+Y+WxH` in logical
/// pixels from the window's top left corner, or `none` when it has none.
pub fn place(bounds: Option<graphene::Rect>) -> String {
    match bounds {
        Some(r) => format!(
            "{:.0}+{:.0}+{:.0}x{:.0}",
            r.x(),
            r.y(),
            r.width(),
            r.height()
        ),
        None => "none".to_string(),
    }
}

/// A value chosen from a few that are all in view, as a row of buttons
/// joined into one, the chosen one in the accent: a click is the whole
/// change, where a [`Choice`] needs a click to open and another to pick.
/// The labels may be replaced (a screen's scale list gains the odd value
/// a file names).
pub struct Segments {
    bar: gtk::Box,
    buttons: std::cell::RefCell<Vec<gtk::ToggleButton>>,
    selected: std::cell::Cell<usize>,
    /// Set while the control itself moves a button, which is not a pick.
    quiet: std::cell::Cell<bool>,
    changed: std::cell::RefCell<Vec<Box<dyn Fn()>>>,
}

impl Segments {
    /// Segments of `labels`, the first chosen.
    pub fn new(labels: &[String]) -> std::rc::Rc<Segments> {
        let bar = gtk::Box::builder()
            .css_classes(["edel-segments"])
            .valign(gtk::Align::Center)
            .build();
        let segments = std::rc::Rc::new(Segments {
            bar,
            buttons: std::cell::RefCell::new(Vec::new()),
            selected: std::cell::Cell::new(0),
            quiet: std::cell::Cell::new(false),
            changed: std::cell::RefCell::new(Vec::new()),
        });
        segments.replace(labels, 0);
        segments
    }

    pub fn widget(&self) -> gtk::Widget {
        self.bar.clone().upcast()
    }

    pub fn selected(&self) -> usize {
        self.selected.get()
    }

    /// Narrower buttons, for a row of short labels such as the numbers 1
    /// to 9, so the row's title keeps its room (M5.2n).
    pub fn tight(&self) {
        self.bar.add_css_class("edel-segments-tight");
    }

    /// The buttons, in the order of the labels (CI's places and focus).
    pub fn buttons(&self) -> Vec<gtk::ToggleButton> {
        self.buttons.borrow().clone()
    }

    /// Shows choice `at` as chosen, without calling back.
    pub fn set_selected(&self, at: usize) {
        self.selected.set(at);
        self.quiet.set(true);
        if let Some(button) = self.buttons.borrow().get(at) {
            button.set_active(true);
        }
        self.quiet.set(false);
    }

    /// Calls `f` when a person picks a value.
    pub fn connect_changed(&self, f: impl Fn() + 'static) {
        self.changed.borrow_mut().push(Box::new(f));
    }

    /// Shows `labels` with choice `at` chosen, without calling back; does
    /// nothing when they are the labels already shown.
    pub fn replace(self: &std::rc::Rc<Self>, labels: &[String], at: usize) {
        let same = {
            let buttons = self.buttons.borrow();
            buttons.len() == labels.len()
                && buttons
                    .iter()
                    .zip(labels)
                    .all(|(b, l)| b.label().as_deref() == Some(l.as_str()))
        };
        if same {
            self.set_selected(at);
            return;
        }
        while let Some(child) = self.bar.first_child() {
            self.bar.remove(&child);
        }
        let mut buttons: Vec<gtk::ToggleButton> = Vec::new();
        for (i, text) in labels.iter().enumerate() {
            let button = gtk::ToggleButton::builder()
                .label(text)
                .css_classes(["edel-segment"])
                .build();
            if let Some(first) = buttons.first() {
                button.set_group(Some(first));
            }
            let weak = std::rc::Rc::downgrade(self);
            button.connect_toggled(move |button| {
                let Some(segments) = weak.upgrade() else {
                    return;
                };
                if button.is_active() && !segments.quiet.get() {
                    segments.selected.set(i);
                    for f in segments.changed.borrow().iter() {
                        f();
                    }
                }
            });
            self.bar.append(&button);
            buttons.push(button);
        }
        *self.buttons.borrow_mut() = buttons;
        self.set_selected(at);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_place_reads_as_ci_reads_it() {
        let at = graphene::Rect::new(12.4, 300.0, 88.0, 30.2);
        assert_eq!(place(Some(at)), "12+300+88x30");
        assert_eq!(place(None), "none");
    }
}
