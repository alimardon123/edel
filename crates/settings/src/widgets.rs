//! Settings' building blocks (M5.6a): a page with its title and what it
//! is for, a section's heading with where its value comes from, a group
//! of rows on a card, and a row with its title, its line under it and its
//! control. Every page is built of these, so a page holds no look of its
//! own, the look is `style.rs`'s from the tokens, and another toolkit
//! would be these few functions again (ADR-004).

use gtk::prelude::*;

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
    (scroll.upcast(), content, problem)
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
        .label("Reset")
        .tooltip_text("Take your choice out, so this machine or the release decides")
        .valign(gtk::Align::Center)
        .css_classes(["edel-link"])
        .build();
    let copy = gtk::Button::builder()
        .child(&icon::image("copy", 12))
        .tooltip_text("Copy as command")
        .valign(gtk::Align::Center)
        .css_classes(["edel-copy"])
        .build();
    copy.update_property(&[gtk::accessible::Property::Label("Copy as command")]);
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

/// One row of a group: its title, the line under it (what it does, then
/// where its value comes from), Reset, Copy as command and its control.
pub struct Row {
    pub subtitle: gtk::Label,
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
    let subtitle = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .css_classes(["edel-row-subtitle"])
        .build();
    words.append(&title_label);
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
