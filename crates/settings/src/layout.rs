//! The Layout page (M5.6a, M5.6b), as the mockup draws it
//! (`docs/mockups/classic-menus.jpg`): a card per preset with its picture,
//! then the rows for tiling, title bars and the buttons' side. Each
//! choice is one line of the person's settings file, which the desktop
//! follows at once, and each says where its value comes from, with Reset
//! for the person's own choice and Copy as command (ADR-008). The page
//! follows the files, so a change from the panel or `edel settings set`
//! shows here too. Later keys join as their steps land: the tiling style
//! (M5.16) and Never with a switch per button (M5.18) as rows here, a
//! person's own presets as cards after the built-in ones, with Save as
//! (M5.17).

use std::cell::Cell;
use std::rc::Rc;

use gtk::gio;
use gtk::prelude::*;

use crate::files::{self, Files};
use crate::style::Theme;
use crate::widgets::{self, Source};
use crate::{icon, preview, rows};

const INTRO: &str = "One preset sets up the whole desktop: its panels, where windows open \
                     and how they tile. Each choice here is also a line of your settings \
                     file and an edel settings set command.";

/// A row's control: a switch for a flag, a list for one of a few values.
enum Control {
    Switch(gtk::Switch),
    Choice(gtk::DropDown, &'static [&'static str]),
}

impl Control {
    /// A list of `key`'s values, as the key table has them.
    fn choice(key: &str) -> Control {
        let values = rows::values(key);
        let labels: Vec<String> = values.iter().map(|v| rows::label(v)).collect();
        let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
        Control::Choice(gtk::DropDown::from_strings(&labels), values)
    }

    fn widget(&self) -> gtk::Widget {
        match self {
            Control::Switch(switch) => switch.clone().upcast(),
            Control::Choice(list, _) => list.clone().upcast(),
        }
    }

    fn show(&self, value: &str) {
        match self {
            Control::Switch(switch) => switch.set_active(value == "true"),
            Control::Choice(list, values) => {
                let at = values.iter().position(|v| *v == value).unwrap_or(0);
                list.set_selected(at as u32);
            }
        }
    }

    fn value(&self) -> String {
        match self {
            Control::Switch(switch) => switch.is_active().to_string(),
            Control::Choice(list, values) => values
                .get(list.selected() as usize)
                .map_or_else(String::new, |v| v.to_string()),
        }
    }

    fn on_change(&self, f: impl Fn() + 'static) {
        match self {
            Control::Switch(switch) => {
                switch.connect_active_notify(move |_| f());
            }
            Control::Choice(list, _) => {
                list.connect_selected_notify(move |_| f());
            }
        }
    }
}

/// One row: its key, what it does, its control and the row's parts.
struct Setting {
    key: &'static str,
    what: String,
    control: Control,
    row: widgets::Row,
}

/// The page's widgets, so every change shows everywhere on it.
struct Ui {
    presets: Vec<(&'static str, gtk::ToggleButton, gtk::Box)>,
    preset_source: Source,
    settings: Vec<Setting>,
    problem: gtk::Label,
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
            if let Some(value) = now.value(setting.key) {
                setting.control.show(&value);
            }
        }
        self.quiet.set(false);
        let preset = files.source("layout.preset");
        let title = files::title(&now.preset);
        self.preset_source
            .label
            .set_label(&rows::describe(&preset, &title));
        self.preset_source
            .reset
            .set_visible(rows::resettable(&preset));
        for setting in &self.settings {
            let source = files.source(setting.key);
            let value = match (setting.key, now.tiling) {
                ("layout.tiling", true) => "On".to_string(),
                ("layout.tiling", false) => "Off".to_string(),
                (key, _) => rows::label(&now.value(key).unwrap_or_default()),
            };
            let line = format!("{} · {}", setting.what, rows::describe(&source, &value));
            setting.row.subtitle.set_label(&line);
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

    widgets::heading(&content, "Windows");
    let group = widgets::group(&content);
    let toggle = files
        .shortcut("toggle_tiling")
        .map_or_else(String::new, |keys| format!(" {keys}, or"));
    let settings = vec![
        setting(
            &group,
            "layout.tiling",
            format!("Side by side;{toggle} the panel's button switches one workspace"),
            Control::Switch(gtk::Switch::new()),
        ),
        setting(
            &group,
            "layout.title_bars",
            "On every window, or only on floating ones".to_string(),
            Control::choice("layout.title_bars"),
        ),
        setting(
            &group,
            "layout.window_buttons",
            "Where close, minimize and maximize sit".to_string(),
            Control::choice("layout.window_buttons"),
        ),
    ];

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
        "layout.preset",
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
        wire(&ui, &setting.row.reset, &setting.row.copy, setting.key);
    }
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

/// Appends a row for `key` to `group`.
fn setting(group: &gtk::Box, key: &'static str, what: String, control: Control) -> Setting {
    let row = widgets::row(group, rows::title(key), &control.widget());
    Setting {
        key,
        what,
        control,
        row,
    }
}

/// `reset` takes `key` out of the person's file; `copy` copies the
/// command that sets its value now.
fn wire(ui: &Rc<Ui>, reset: &gtk::Button, copy: &gtk::Button, key: &'static str) {
    let weak = Rc::downgrade(ui);
    reset.connect_clicked(move |_| {
        if let Some(ui) = weak.upgrade() {
            ui.report(Files::here().set(key, None));
            ui.update();
        }
    });
    copy.connect_clicked(move |button| {
        let value = Files::here().layout().value(key).unwrap_or_default();
        button.clipboard().set_text(&rows::command(key, &value));
        widgets::copied(button);
    });
}
