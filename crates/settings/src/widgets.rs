//! Settings' building blocks (M5.6a): a page with its title and what it
//! is for, a section's heading with where its value comes from, a group
//! of rows on a card, and a row with its title, its line under it and its
//! control. Every page is built of these, so a page holds no look of its
//! own, the look is `style.rs`'s from the tokens, and another toolkit
//! would be these few functions again (ADR-004).

use gtk::prelude::*;

use edel::i18n::{tr, trf};

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

fn source() -> Source {
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
    let subtitle = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .css_classes(["edel-row-subtitle"])
        .build();
    words.append(&heading);
    words.append(&subtitle);
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
    }
}

/// A row that shows a value people may select and copy, as About's.
pub fn value_row(group: &gtk::Box, title: &str, value: &str) {
    let row = gtk::Box::builder()
        .spacing(10)
        .css_classes(["edel-row"])
        .build();
    let title_label = gtk::Label::builder()
        .label(title)
        .xalign(0.0)
        .hexpand(true)
        .css_classes(["edel-row-title"])
        .build();
    let value_label = gtk::Label::builder()
        .label(value)
        .xalign(1.0)
        .selectable(true)
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
            .child(&list)
            .has_arrow(false)
            .css_classes(["edel-choice-list"])
            .build();
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
        list.connect_row_activated(move |_, row| {
            let Some(choice) = weak.upgrade() else { return };
            choice.set_selected(row.index().max(0) as usize);
            popover.popdown();
            for f in choice.changed.borrow().iter() {
                f();
            }
        });
        choice
    }

    pub fn widget(&self) -> gtk::Widget {
        self.button.clone().upcast()
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
