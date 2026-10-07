//! The Layout page (M5.6a, M5.6b), as the mockup draws it
//! (`docs/mockups/classic-menus.jpg`): a card per preset with its picture,
//! then a card of rows for tiling and its style (M5.16a), title bars and
//! the buttons' side, and
//! the title bar's buttons: a bar drawn as every window's will be and a
//! switch for each button (M5.18a). Each
//! choice is one line of the person's settings file, which the desktop
//! follows at once, and each says where its value comes from, with Reset
//! for the person's own choice and Copy as command (ADR-008). The page
//! follows the files, so a change from the panel or `edel settings set`
//! shows here too. Later keys join as their steps land: Never (M5.18b)
//! as a row here, a person's own presets as cards after the built-in
//! ones, with Save as (M5.17).

use std::cell::Cell;
use std::rc::Rc;

use gtk::gio;
use gtk::prelude::*;

use crate::files::{self, Files};
use crate::style::Theme;
use crate::widgets;
use crate::{icon, preview, rows};

const INTRO: &str = "One preset sets up the whole desktop: its panels, where windows open \
                     and how they tile. Each choice here is also a line of your settings \
                     file and an edel settings set command.";

/// A row's control: a switch for a flag, a list for one of a few values.
enum Control {
    Switch(gtk::Switch),
    Choice(Rc<widgets::Choice>, &'static [&'static str]),
}

impl Control {
    /// A list of `key`'s values, as the key table has them.
    fn choice(key: &str) -> Control {
        let values = rows::values(key);
        let labels: Vec<String> = values.iter().map(|v| rows::label(v)).collect();
        Control::Choice(widgets::Choice::new(labels), values)
    }

    fn widget(&self) -> gtk::Widget {
        match self {
            Control::Switch(switch) => switch.clone().upcast(),
            Control::Choice(list, _) => list.widget(),
        }
    }

    /// Shows `now`'s value of `key`.
    fn show(&self, key: &str, now: &files::Layout) {
        let value = |key| now.value(key).unwrap_or_default();
        match self {
            Control::Switch(switch) => switch.set_active(value(key) == "true"),
            Control::Choice(list, values) => {
                let at = values.iter().position(|v| *v == value(key)).unwrap_or(0);
                list.set_selected(at);
            }
        }
    }

    /// The value it shows, as `edel settings set` writes it.
    fn value(&self) -> String {
        match self {
            Control::Switch(switch) => switch.is_active().to_string(),
            Control::Choice(list, values) => values
                .get(list.selected())
                .map_or_else(String::new, |v| v.to_string()),
        }
    }

    fn on_change(&self, f: impl Fn() + 'static) {
        match self {
            Control::Switch(switch) => {
                switch.connect_active_notify(move |_| f());
            }
            Control::Choice(list, _) => list.connect_changed(f),
        }
    }
}

/// One row: its key, what it does, the shortcut that does the same,
/// whether the preset gives its value when nobody chose one, its control
/// and the row's parts.
struct Setting {
    key: &'static str,
    what: String,
    action: Option<&'static str>,
    from_preset: bool,
    control: Control,
    row: widgets::Row,
}

/// The page's widgets, so every change shows everywhere on it.
struct Ui {
    presets: Vec<(&'static str, gtk::ToggleButton, gtk::Box)>,
    preset_source: widgets::Source,
    settings: Vec<Setting>,
    /// A title bar as the compositor draws it with these choices.
    bar: widgets::BarPreview,
    problem: gtk::Label,
    /// Undo and Keep, shown while the person's own values of the page's
    /// keys differ from `before`, as they were when the page opened or
    /// Keep was pressed.
    changes: widgets::ChangeBar,
    before: std::cell::RefCell<Vec<(&'static str, Option<String>)>>,
    /// Set while the page itself moves a control, so that is not a choice.
    quiet: Cell<bool>,
    /// Kept so the page follows the files while it is open.
    monitors: Vec<gio::FileMonitor>,
}

impl Ui {
    /// Shows what the files say now: the chosen preset, each row's value,
    /// and where each comes from.
    fn update(&self) {
        let files = Files::here();
        let now = files.layout();
        self.quiet.set(true);
        for (name, card, badge) in &self.presets {
            card.set_active(*name == now.preset);
            badge.set_visible(*name == now.preset);
        }
        for setting in &self.settings {
            setting.control.show(setting.key, &now);
        }
        self.quiet.set(false);
        self.changes
            .show(files.own_layout() != *self.before.borrow());
        self.bar.show(
            now.window_buttons == "left",
            [now.minimize_button, now.maximize_button, now.close_button],
        );
        let preset = files.source("layout.preset");
        let title = files::title(&now.preset);
        self.preset_source
            .label
            .set_label(&rows::describe(&preset, &title));
        self.preset_source
            .reset
            .set_visible(rows::resettable(&preset));
        for setting in &self.settings {
            let keys = setting.action.and_then(|a| files.shortcut(a));
            widgets::show_keys(&setting.row.keys, keys.as_deref());
            let source = files.source(setting.key);
            let note = rows::note(&source, setting.from_preset);
            let line = match (setting.what.as_str(), note) {
                (what, Some(note)) if !what.is_empty() => format!("{what} · {note}"),
                ("", Some(note)) => note.to_string(),
                (what, _) => what.to_string(),
            };
            setting.row.subtitle.set_label(&line);
            setting.row.subtitle.set_visible(!line.is_empty());
            setting.row.reset.set_visible(rows::resettable(&source));
        }
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

pub fn page(theme: &Rc<Theme>) -> gtk::Widget {
    let files = Files::here();
    let (page, content, problem) = widgets::page("Layout", INTRO);

    let preset_source = widgets::section(&content, rows::title("layout.preset"));
    let cards = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .homogeneous(true)
        .min_children_per_line(1)
        .max_children_per_line(3)
        .column_spacing(12)
        .row_spacing(12)
        .build();
    let mut presets = Vec::new();
    let mut first: Option<gtk::ToggleButton> = None;
    for name in edel::presets::NAMES {
        let (card, badge) = card(name, theme);
        match &first {
            Some(first) => card.set_group(Some(first)),
            None => first = Some(card.clone()),
        }
        cards.append(&card);
        // The card takes the focus, not the box the flow wraps it in.
        if let Some(child) = card.parent() {
            child.set_focusable(false);
        }
        presets.push((*name, card, badge));
    }
    content.append(&cards);

    let group = widgets::group(&content);
    group.set_margin_top(20);
    let mut settings = vec![setting(
        &group,
        "layout.tiling",
        ("Or the panel's toggle", Some("toggle_tiling"), true),
        Control::Switch(gtk::Switch::new()),
    )];
    // How tiling lays windows out (M5.16a): the same key as the panel's
    // layout button's menu and `edel settings set`.
    settings.push(setting(
        &group,
        "layout.tiling_style",
        (
            "Stack keeps one main window, Split halves the focused one",
            None,
            false,
        ),
        Control::choice("layout.tiling_style"),
    ));

    // Everything about title bars in one group, each row named as its key
    // is (ADR-008's same names decision): a bar drawn as the compositor
    // draws it, so every choice shows at once, then where bars show, the
    // side their buttons sit on, and a switch per button, each row saying
    // how to do the same without the button (M5.18a).
    widgets::heading(&content, "Title bars");
    let bars = widgets::group(&content);
    let bar = widgets::BarPreview::new(&bars, "Settings");
    settings.push(setting(
        &bars,
        "layout.title_bars",
        ("On every window, or only on floating ones", None, false),
        Control::choice("layout.title_bars"),
    ));
    settings.push(setting(
        &bars,
        "layout.window_buttons",
        ("The side they sit on", None, true),
        Control::choice("layout.window_buttons"),
    ));
    for (key, what, action) in [
        (
            "layout.minimize_button",
            "The panel's window list brings a window back",
            Some("minimize_window"),
        ),
        (
            "layout.maximize_button",
            "Or double-click a title bar",
            Some("toggle_maximize"),
        ),
        (
            "layout.close_button",
            "Closes a window even with its button hidden",
            Some("close_window"),
        ),
    ] {
        settings.push(setting(
            &bars,
            key,
            (what, action, false),
            Control::Switch(gtk::Switch::new()),
        ));
    }

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
        presets,
        preset_source,
        settings,
        bar,
        problem,
        changes: widgets::ChangeBar::new(&page),
        before: std::cell::RefCell::new(files.own_layout()),
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

    for (name, card, _) in &ui.presets {
        let (name, weak) = (*name, Rc::downgrade(&ui));
        card.connect_toggled(move |card| {
            let Some(ui) = weak.upgrade() else { return };
            if card.is_active() && !ui.quiet.get() {
                ui.report(Files::here().choose("layout.preset", name));
                // A preset brings its own policy and buttons unless set.
                ui.update();
            }
        });
    }
    wire(
        &ui,
        &ui.preset_source.reset,
        &ui.preset_source.copy,
        vec!["layout.preset"],
    );
    for (i, setting) in ui.settings.iter().enumerate() {
        let weak = Rc::downgrade(&ui);
        setting.control.on_change(move || {
            let Some(ui) = weak.upgrade() else { return };
            if !ui.quiet.get() {
                let setting = &ui.settings[i];
                ui.report(Files::here().choose(setting.key, &setting.control.value()));
                ui.update();
            }
        });
        wire(
            &ui,
            &setting.row.reset,
            &setting.row.copy,
            vec![setting.key],
        );
    }
    let weak = Rc::downgrade(&ui);
    ui.changes.undo.connect_clicked(move |_| {
        if let Some(ui) = weak.upgrade() {
            let before = ui.before.borrow().clone();
            ui.report(Files::here().restore(&before));
            ui.update();
        }
    });
    let weak = Rc::downgrade(&ui);
    ui.changes.keep.connect_clicked(move |_| {
        if let Some(ui) = weak.upgrade() {
            *ui.before.borrow_mut() = Files::here().own_layout();
            ui.update();
        }
    });
    // The page's state lives as long as the page does.
    let keep = ui.clone();
    page.connect_destroy(move |_| {
        let _ = &keep;
    });
    page
}

/// A preset's card: its picture, its name and, when chosen, a check.
fn card(name: &str, theme: &Rc<Theme>) -> (gtk::ToggleButton, gtk::Box) {
    let title = files::title(name);
    let label = gtk::Label::builder()
        .label(&title)
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
    inner.append(&preview::area(name, theme));
    inner.append(&name_row);
    let card = gtk::ToggleButton::builder()
        .child(&inner)
        .css_classes(["edel-preset"])
        .build();
    card.update_property(&[gtk::accessible::Property::Label(&title)]);
    (card, badge)
}

/// Appends a row for `key` to `group`: what it does, the shortcut action
/// that does the same, and whether the preset gives its value when nobody
/// chose one.
fn setting(
    group: &gtk::Box,
    key: &'static str,
    (what, action, from_preset): (&str, Option<&'static str>, bool),
    control: Control,
) -> Setting {
    let row = widgets::row(group, rows::title(key), &control.widget());
    Setting {
        key,
        what: what.to_string(),
        action,
        from_preset,
        control,
        row,
    }
}

/// `reset` takes `keys` out of the person's file; `copy` copies the one
/// command that sets their values now.
fn wire(ui: &Rc<Ui>, reset: &gtk::Button, copy: &gtk::Button, keys: Vec<&'static str>) {
    let weak = Rc::downgrade(ui);
    let reset_keys = keys.clone();
    reset.connect_clicked(move |_| {
        if let Some(ui) = weak.upgrade() {
            let files = Files::here();
            let result = reset_keys.iter().try_for_each(|key| files.set(key, None));
            ui.report(result);
            ui.update();
        }
    });
    copy.connect_clicked(move |button| {
        let now = Files::here().layout();
        let pairs: Vec<(&str, String)> = keys
            .iter()
            .map(|key| (*key, now.value(key).unwrap_or_default()))
            .collect();
        button.clipboard().set_text(&rows::command(&pairs));
        widgets::copied(button);
    });
}
