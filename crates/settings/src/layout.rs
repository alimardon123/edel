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

use gtk::prelude::*;
use gtk::{gio, glib};

use edel::i18n::{n_, tr};

use crate::files::{self, Files};
use crate::style::Theme;
use crate::widgets;
use crate::{icon, panels, preview, rows, tray, workspaces};

const INTRO: &str = n_(
    "One preset sets up the whole desktop: its panels, where windows open \
                     and how they tile. Each choice here is also a line of your settings \
                     file and an edel settings set command.",
);

/// A row's control: a switch for a flag, a list for one of a few values, or
/// buttons in a row for a few values all in view (the workspaces' numbers).
enum Control {
    Switch(gtk::Switch),
    Choice(Rc<widgets::Choice>, &'static [&'static str]),
    /// The values as written, `"1"` to `"9"`.
    Segments(Rc<widgets::Segments>, Vec<String>),
}

impl Control {
    /// A list of `key`'s values, as the key table has them.
    fn choice(key: &str) -> Control {
        let values = rows::values(key);
        let labels: Vec<String> = values.iter().map(|v| rows::label(v)).collect();
        Control::Choice(widgets::Choice::new(labels), values)
    }

    /// The workspaces' numbers, 1 to the most there may be (M5.2n).
    fn numbers() -> Control {
        let values: Vec<String> = (1..=edel::presets::MOST_WORKSPACES)
            .map(|n| n.to_string())
            .collect();
        Control::Segments(widgets::Segments::new(&values), values)
    }

    fn widget(&self) -> gtk::Widget {
        match self {
            Control::Switch(switch) => switch.clone().upcast(),
            Control::Choice(list, _) => list.widget(),
            Control::Segments(segments, _) => segments.widget(),
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
            Control::Segments(segments, values) => {
                let at = values.iter().position(|v| *v == value(key)).unwrap_or(0);
                segments.set_selected(at);
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
            Control::Segments(segments, values) => {
                values.get(segments.selected()).cloned().unwrap_or_default()
            }
        }
    }

    fn on_change(&self, f: impl Fn() + 'static) {
        match self {
            Control::Switch(switch) => {
                switch.connect_active_notify(move |_| f());
            }
            Control::Choice(list, _) => list.connect_changed(f),
            Control::Segments(segments, _) => segments.connect_changed(f),
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
    /// The control's widget, which the page greys out while it does nothing.
    widget: gtk::Widget,
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
    /// The Tray card, which follows the files too (M5.9h).
    tray: Rc<tray::Card>,
    /// The Panels group, which follows them too (M5.31d).
    panels: Rc<panels::Card>,
    /// The Workspaces group's names and apps (M5.2n).
    names: Rc<workspaces::Card>,
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
        self.tray.show();
        self.panels.show(&files, &now.preset);
        self.bar.show(
            now.window_buttons == "left",
            [now.minimize_button, now.maximize_button, now.close_button],
        );
        let preset = files.source("layout.preset");
        let title = edel::presets::title(&now.preset);
        self.preset_source
            .label
            .set_label(&rows::describe(&preset, &title));
        self.preset_source
            .reset
            .set_visible(rows::resettable(&preset));
        self.names.show(&files, &now);
        let dynamic = now.dynamic_workspaces;
        for setting in &self.settings {
            // The count is no choice while workspaces come and go by
            // themselves, and the switcher's numbers and ends only show
            // with the numbers look (M5.2n).
            let inert = match setting.key {
                "layout.workspaces" => dynamic,
                "layout.workspaces_shown" | "layout.workspaces_ends" => {
                    now.workspaces_look != "numbers"
                }
                _ => false,
            };
            setting.widget.set_sensitive(!inert);
            let what = if setting.key == "layout.workspaces" && dynamic {
                tr("Dynamic workspaces decide how many")
            } else {
                setting.what.as_str()
            };
            let keys = setting.action.and_then(|a| files.shortcut(a));
            widgets::show_keys(&setting.row.keys, keys.as_deref());
            let source = files.source(setting.key);
            let note = rows::note(&source, setting.from_preset);
            let line = match (what, note) {
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
    let (page, content, problem) = widgets::page(tr("Layout"), tr(INTRO));

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
        (tr("Or the panel's toggle"), Some("toggle_tiling"), true),
        Control::Switch(gtk::Switch::new()),
    )];
    // How tiling lays windows out (M5.16a): the same key as the panel's
    // layout button's menu and `edel settings set`.
    settings.push(setting(
        &group,
        "layout.tiling_style",
        (
            tr("Stack keeps one main window, Split halves the focused one, Scroll lines them up"),
            None,
            false,
        ),
        Control::choice("layout.tiling_style"),
    ));

    // The panels that apply, and the editor that changes them (M5.31d).
    let panels = panels::card(&content, theme, &problem);

    // Everything about title bars in one group, each row named as its key
    // is (ADR-008's same names decision): a bar drawn as the compositor
    // draws it, so every choice shows at once, then where bars show, the
    // side their buttons sit on, and a switch per button, each row saying
    // how to do the same without the button (M5.18a).
    widgets::heading(&content, tr("Title bars"));
    let bars = widgets::group(&content);
    let bar = widgets::BarPreview::new(&bars, tr("Settings"));
    settings.push(setting(
        &bars,
        "layout.title_bars",
        (tr("On every window, or only on floating ones"), None, false),
        Control::choice("layout.title_bars"),
    ));
    settings.push(setting(
        &bars,
        "layout.window_buttons",
        (tr("The side they sit on"), None, true),
        Control::choice("layout.window_buttons"),
    ));
    for (key, what, action) in [
        (
            "layout.minimize_button",
            tr("The panel's window list brings a window back"),
            Some("minimize_window"),
        ),
        (
            "layout.maximize_button",
            tr("Or double-click a title bar"),
            Some("toggle_maximize"),
        ),
        (
            "layout.close_button",
            tr("Closes a window even with its button hidden"),
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

    // The tray's apps (M5.9h): kept in the panel or behind its arrow, as the
    // panel's drag keeps them, read from the bus when the page opens.
    let tray = tray::card(&content, &problem);

    // The workspaces (M5.2n): how many, whether they come and go, the
    // names, the apps that open on their own, then the switcher's look,
    // shown count and ends. The names and apps are a card of their own.
    let heading = widgets::heading(&content, tr("Workspaces"));
    let spaces = widgets::group(&content);
    let count = setting(
        &spaces,
        "layout.workspaces",
        (
            tr("How many there are; Super+1 to Super+9 show them"),
            None,
            true,
        ),
        Control::numbers(),
    );
    log_workspaces(&page, &heading, &count);
    settings.push(count);
    settings.push(setting(
        &spaces,
        "layout.dynamic_workspaces",
        (
            tr("An empty one always waits at the end, and other empty ones close"),
            None,
            false,
        ),
        Control::Switch(gtk::Switch::new()),
    ));
    settings.push(setting(
        &spaces,
        "layout.workspaces_per_screen",
        (tr("Each screen shows its own workspace"), None, false),
        Control::Switch(gtk::Switch::new()),
    ));
    settings.push(setting(
        &spaces,
        "layout.workspaces_look",
        (tr("How the panel's switcher shows them"), None, false),
        Control::choice("layout.workspaces_look"),
    ));
    settings.push(setting(
        &spaces,
        "layout.workspaces_shown",
        (
            tr("How many numbers the switcher shows at once"),
            None,
            false,
        ),
        Control::numbers(),
    ));
    settings.push(setting(
        &spaces,
        "layout.workspaces_ends",
        (tr("What shows where more workspaces lie"), None, false),
        Control::choice("layout.workspaces_ends"),
    ));
    let names = workspaces::card(&spaces, &problem);

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
        tray: tray.clone(),
        panels: panels.clone(),
        names: names.clone(),
    });
    ui.update();
    tray.refresh();
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
    // Asked for the Panels group by name, the keyboard goes to its Edit
    // panels button once the page shows (M5.31d); the page's own rule is
    // `widgets::take_asked`.
    let edit = panels.edit();
    page.connect_map(move |page| {
        let (page, edit) = (page.clone(), edit.clone());
        gtk::glib::idle_add_local_once(move || {
            if widgets::asked_row() == Some(edel::panel_edit::PANELS) {
                widgets::take_asked(&page, &edit);
            }
        });
    });
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
    let title = edel::presets::title(name);
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
    let widget = control.widget();
    let row = widgets::row(group, rows::title(key), &widget);
    Setting {
        key,
        what: what.to_string(),
        action,
        from_preset,
        control,
        widget,
        row,
    }
}

/// Logs where the count's buttons and its Reset lie in the window, once the
/// Workspaces group is laid out, for CI to click (logical pixels). Asked for
/// by name, the page first scrolls the heading to the top of its view and
/// gives the keyboard to the chosen number, as the Panels group does.
fn log_workspaces(page: &gtk::Widget, heading: &gtk::Box, count: &Setting) {
    let Control::Segments(segments, _) = &count.control else {
        return;
    };
    let (page, segments, reset, key) = (
        page.clone(),
        segments.clone(),
        count.row.reset.clone(),
        count.key,
    );
    let scrolled = std::cell::Cell::new(false);
    heading.add_tick_callback(move |heading, _| {
        let Some(window) = heading.root().and_downcast::<gtk::Window>() else {
            return glib::ControlFlow::Continue;
        };
        if !heading.is_mapped() || heading.width() == 0 {
            return glib::ControlFlow::Continue;
        }
        if !scrolled.replace(true) {
            if widgets::asked_row() != Some(key) {
                return glib::ControlFlow::Break;
            }
            widgets::scroll_to_top(heading.upcast_ref());
            let chosen = segments.buttons().get(segments.selected()).cloned();
            if let Some(chosen) = chosen {
                widgets::take_asked(&page, &chosen);
            }
            return glib::ControlFlow::Continue;
        }
        let counts: Vec<String> = segments
            .buttons()
            .iter()
            .map(|button| widgets::place(button.compute_bounds(&window)))
            .collect();
        eprintln!(
            "edel-settings: workspaces group places count {}, reset {}",
            counts.join(" "),
            widgets::place(reset.compute_bounds(&window)),
        );
        glib::ControlFlow::Break
    });
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
