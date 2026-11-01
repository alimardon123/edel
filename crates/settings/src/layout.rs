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
//! shows here too. Never (M5.18b) is a row here. A person's own presets
//! are cards after the built-in ones (M5.17): a right click deletes one,
//! and Save as preset, under the cards while the person's file sets layout
//! keys of its own, keeps the layout in use as one.

use std::cell::Cell;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gio, glib};

use edel::i18n::{n_, tr, trf};

use crate::files::{self, Files};
use crate::style::Theme;
use crate::widgets;
use crate::{
    colours, cursor, depth, desktop, motion, panels, preview, rows, shape, text, themes, tray,
    workspaces,
};

const PANELS_INTRO: &str = n_("The bars and docks along the screen's edges, and what they hold.");

const WORKSPACES_INTRO: &str = n_("Desktops of their own, to keep windows apart.");

const APPEARANCE_INTRO: &str = n_("Your theme in light and dark, and how everything is drawn.");

const INTRO: &str = n_("One preset sets up the whole desktop: its panels, windows and tiling.");

/// A row's control: a switch for a flag, a list for one of a few values, or
/// buttons in a row for a few values all in view (the workspaces' numbers).
enum Control {
    Switch(gtk::Switch),
    Choice(Rc<widgets::Choice>, &'static [&'static str]),
    /// The values as written, such as `"1"` to `"9"` or `"dark"`.
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
        let segments = widgets::Segments::new(&values);
        segments.tight();
        Control::Segments(segments, values)
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

/// The Layout page's own presets (M5.17): the cards' flow, the line of the
/// layout's changes under them with Save as preset, and the note under the
/// cards for a change that was refused.
struct Own {
    cards: gtk::FlowBox,
    /// The names the cards show, in order, so they are rebuilt only when
    /// the presets change.
    names: std::cell::RefCell<Vec<String>>,
    changes: gtk::Box,
    changed: gtk::Label,
    save: gtk::Button,
    note: gtk::Label,
}

/// The page's widgets, so every change shows everywhere on it.
struct Ui {
    /// The settings file's section the page shows (M5.2q).
    section: &'static str,
    /// The preset cards, built-in then own, by name (M5.17); rebuilt when
    /// the presets change, through `sync_cards`.
    presets: std::cell::RefCell<Vec<(String, gtk::ToggleButton, gtk::Box)>>,
    /// The Layout page's own presets (M5.17): the cards' box and the line of
    /// changes under them.
    own: Option<Own>,
    theme: Rc<Theme>,
    preset_source: Option<widgets::Source>,
    settings: Vec<Setting>,
    /// A title bar as the compositor draws it with these choices.
    bar: Option<widgets::BarPreview>,
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
    /// The Panels page's cards, which follow the files too: the panels
    /// that apply (M5.31d) and the tray's apps (M5.9h).
    panels: Option<Rc<panels::Card>>,
    tray: Option<Rc<tray::Card>>,
    /// The Workspaces page's names and apps (M5.2n).
    names: Option<Rc<workspaces::Card>>,
    /// The Appearance page's theme card (M5.12b6b): the theme line, the
    /// styles, the themes row and the pictures sheet, which follows the files.
    theme_card: Option<Rc<themes::Card>>,
    /// The Appearance page's accent and colours, which follow the files too
    /// (M5.12b1c).
    colours: Option<Rc<colours::Group>>,
    /// The Appearance page's Shape group, which follows the files too
    /// (M5.12b2c).
    shape: Option<Rc<shape::Group>>,
    /// The Appearance page's Text group, which follows the files too
    /// (M5.12b5).
    text: Option<Rc<text::Group>>,
    /// The Appearance page's Depth group, which follows the files too
    /// (M5.12b3).
    depth: Option<Rc<depth::Group>>,
    /// The Appearance page's Cursor and icons group, which follows the files
    /// too (M5.12b5 part A).
    cursor: Option<Rc<cursor::Group>>,
    /// The Appearance page's Motion group, which follows the files too
    /// (M5.12b4).
    motion: Option<Rc<motion::Group>>,
    /// The Appearance page's Desktop group, which follows the files too
    /// (M8.21 part 21j1).
    desktop: Option<Rc<desktop::Group>>,
}

impl Ui {
    /// Shows what the files say now: the chosen preset, each row's value,
    /// and where each comes from.
    fn update(&self) {
        let files = Files::here();
        let now = files.layout();
        self.quiet.set(true);
        for (name, card, badge) in self.presets.borrow().iter() {
            card.set_active(*name == now.named);
            badge.set_visible(*name == now.named);
        }
        for setting in &self.settings {
            setting.control.show(setting.key, &now);
        }
        self.quiet.set(false);
        self.changes
            .show(files.own_keys(self.section) != *self.before.borrow());
        if let Some(tray) = &self.tray {
            tray.show();
        }
        if let Some(panels) = &self.panels {
            panels.show(&files, &now.preset);
        }
        if let Some(bar) = &self.bar {
            bar.show(
                now.window_buttons == "left",
                [now.minimize_button, now.maximize_button, now.close_button],
            );
        }
        if let Some(own) = &self.own {
            let changed = files.changed_layout();
            own.changes.set_visible(!changed.is_empty());
            own.changed.set_label(&trf(
                "Changed: {what}",
                &[("what", &themes::changed_words(&changed))],
            ));
        }
        if let Some(source) = &self.preset_source {
            let preset = files.source("layout.preset");
            let title = edel::presets::title(&now.named);
            source.label.set_label(&rows::describe(&preset, &title));
            source.reset.set_visible(rows::resettable(&preset));
        }
        if let Some(names) = &self.names {
            names.show(&files, &now);
        }
        if let Some(theme_card) = &self.theme_card {
            theme_card.show(&files);
        }
        if let Some(colours) = &self.colours {
            colours.show(&files);
        }
        if let Some(shape) = &self.shape {
            shape.show();
        }
        if let Some(text) = &self.text {
            text.show();
        }
        if let Some(cursor) = &self.cursor {
            cursor.show();
        }
        if let Some(depth) = &self.depth {
            depth.show();
        }
        if let Some(motion) = &self.motion {
            motion.show();
        }
        if let Some(desktop) = &self.desktop {
            desktop.show();
        }
        let dynamic = now.dynamic_workspaces;
        for setting in &self.settings {
            // The count is no choice while workspaces come and go by
            // themselves, and the switcher's numbers and ends only show
            // with the numbers look (M5.2n).
            let inert = match setting.key {
                "workspaces.count" => dynamic,
                "appearance.switcher_shown" | "appearance.switcher_ends" => {
                    now.workspaces_look != edel::settings::SWITCHER_LOOK_DEFAULT
                }
                _ => false,
            };
            setting.widget.set_sensitive(!inert);
            let what = if setting.key == "workspaces.count" && dynamic {
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

    /// Shows why a preset change was not made, under the cards, or takes an
    /// old message away (M5.17).
    fn report_preset(&self, result: Result<(), String>) {
        let Some(own) = &self.own else { return };
        match result {
            Ok(()) => own.note.set_visible(false),
            Err(e) => {
                own.note.set_label(&e);
                own.note.set_visible(true);
            }
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

/// The Layout page (M5.6a): the presets, tiling and title bars.
pub fn page(theme: &Rc<Theme>) -> gtk::Widget {
    build("layout", theme)
}

/// The Panels page (M5.2q): the panels that apply with their editor, and
/// the tray's apps.
pub fn panels_page(theme: &Rc<Theme>) -> gtk::Widget {
    build("panels", theme)
}

/// The Workspaces page (M5.2q): how many, whether they come and go, each
/// screen's own, their names and the apps that open on their own.
pub fn workspaces_page(theme: &Rc<Theme>) -> gtk::Widget {
    build("workspaces", theme)
}

/// The Appearance page's first rows (M5.2q): light or dark, and the
/// workspace switcher's look.
pub fn appearance_page(theme: &Rc<Theme>) -> gtk::Widget {
    build("appearance", theme)
}

/// The page for `section` of the settings file: its rows, with the
/// machinery every one of these pages shares (each row's line, Reset, Copy
/// as command, Undo and Keep, and following the files).
fn build(section: &'static str, theme: &Rc<Theme>) -> gtk::Widget {
    let files = Files::here();
    let (title, intro) = match section {
        "panels" => (tr("Panels"), tr(PANELS_INTRO)),
        "workspaces" => (tr("Workspaces"), tr(WORKSPACES_INTRO)),
        "appearance" => (tr("Appearance"), tr(APPEARANCE_INTRO)),
        _ => (tr("Layout"), tr(INTRO)),
    };
    let (page, content, problem) = widgets::page(title, intro);
    let mut own = None;
    let mut preset_source = None;
    let mut settings = Vec::new();
    let mut bar = None;
    let (mut panels, mut tray, mut names) = (None, None, None);
    let mut theme_card = None;
    let mut colour_group = None;
    let mut shape_group = None;
    let mut depth_group = None;
    let mut cursor_group = None;
    let mut text_group = None;
    let mut motion_group = None;
    let mut desktop_group = None;
    match section {
        "layout" => {
            let source = widgets::section(&content, rows::title("layout.preset"));
            // The cards are the built-in presets, then the person's own
            // (M5.17), filled in by `sync_cards` once the page exists.
            let cards = gtk::FlowBox::builder()
                .selection_mode(gtk::SelectionMode::None)
                .homogeneous(true)
                .min_children_per_line(1)
                .max_children_per_line(3)
                .column_spacing(12)
                .row_spacing(12)
                .build();
            content.append(&cards);
            // Under them, while the person's file sets layout keys of its
            // own: what changed, and Save as preset (M5.17).
            let changed = gtk::Label::builder()
                .xalign(0.0)
                .hexpand(true)
                .css_classes(["edel-theme-changed"])
                .build();
            let save = widgets::action(tr("Save as preset"), false);
            let changes = gtk::Box::builder()
                .spacing(14)
                .margin_top(10)
                .visible(false)
                .build();
            changes.append(&changed);
            changes.append(&save);
            content.append(&changes);
            let note = gtk::Label::builder()
                .xalign(0.0)
                .wrap(true)
                .visible(false)
                .css_classes(["edel-error"])
                .build();
            content.append(&note);
            own = Some(Own {
                cards,
                names: std::cell::RefCell::new(Vec::new()),
                changes,
                changed,
                save,
                note,
            });

            let group = widgets::group(&content);
            group.set_margin_top(20);
            settings.push(setting(
                &group,
                "layout.tiling",
                (tr("Or the panel's toggle"), Some("toggle_tiling"), true),
                Control::Switch(gtk::Switch::new()),
            ));
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

            // Everything about title bars in one group, each row named as its key
            // is (ADR-008's same names decision): a bar drawn as the compositor
            // draws it, so every choice shows at once, then where bars show, the
            // side their buttons sit on, and a switch per button, each row saying
            // how to do the same without the button (M5.18a).
            widgets::heading(&content, tr("Title bars"));
            let bars = widgets::group(&content);
            bar = Some(widgets::BarPreview::new(&bars, tr("Settings")));
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

            preset_source = Some(source);
        }
        "panels" => {
            // The panels that apply, and the editor that changes them
            // (M5.31d), then the tray's apps, kept in the panel or behind
            // its arrow as the panel's drag keeps them (M5.9h).
            panels = Some(panels::card(&content, theme, &problem));
            tray = Some(tray::card(&content, &problem));
        }
        "workspaces" => {
            // How many, whether they come and go, each screen's own, then
            // the names and the apps that open on their own (M5.2n).
            let spaces = widgets::group(&content);
            let count = setting(
                &spaces,
                "workspaces.count",
                (
                    tr("How many there are; Super+1 to Super+9 show them"),
                    None,
                    true,
                ),
                Control::numbers(),
            );
            log_places(&page, &spaces, &count, ("workspaces", "count"));
            settings.push(count);
            settings.push(setting(
                &spaces,
                "workspaces.dynamic",
                (
                    tr("An empty one always waits at the end, and other empty ones close"),
                    None,
                    false,
                ),
                Control::Switch(gtk::Switch::new()),
            ));
            settings.push(setting(
                &spaces,
                "workspaces.per_screen",
                (tr("Each screen shows its own workspace"), None, false),
                Control::Switch(gtk::Switch::new()),
            ));
            settings.push(setting(
                &spaces,
                "workspaces.overview_strip",
                (
                    tr("Which side of the screen the workspace overview shows its strip of workspaces on"),
                    None,
                    false,
                ),
                Control::choice("workspaces.overview_strip"),
            ));
            names = Some(workspaces::card(&spaces, &problem));
        }
        _ => {
            // The Appearance page's group headings are the board's (M5.12b6c),
            // styled by this class in style.rs.
            content.add_css_class("edel-appearance");
            // The theme first (M5.12b6b): its line with the pictures sheet
            // behind its button, the three styles, and the themes row. Then
            // the accent and the colours (M5.12b1c), the text, the shape and
            // the motion, then the workspace switcher's look, shown count and
            // ends (M5.2m).
            theme_card = Some(themes::card(&content, &problem, theme));
            colour_group = Some(colours::group(&content, theme, &problem));
            // The text after the colours (M5.12b5): the fonts and the size,
            // and the windows' shape after them (M5.12b2c): corners, gaps,
            // border, title bars and panels' opacity, with a picture.
            text_group = Some(text::group(&content, &problem));
            // The pointer and the icons after the text (M5.12b5 part A).
            cursor_group = Some(cursor::group(&content, &problem));
            shape_group = Some(shape::group(&content, theme, &problem));
            // The depth after the shape (M5.12b3): how see-through the
            // windows are and the shadow under them.
            depth_group = Some(depth::group(&content, &problem));
            // The motion after the depth (M5.12b4): how much, how fast and
            // with what feel things move.
            motion_group = Some(motion::group(&content, &problem));
            // The desktop's widgets after the motion (M8.21 part 21j1): the
            // clock and the date over the wallpaper, one switch.
            desktop_group = Some(desktop::group(&content, &problem));
            // The glass (M8.21 part 21m): how the shell's surfaces are drawn
            // and their colour, one group.
            widgets::heading(&content, tr("Glass"));
            let glass = widgets::group(&content);
            settings.push(setting(
                &glass,
                "appearance.glass",
                (tr("How the panel and its cards are drawn"), None, false),
                Control::choice("appearance.glass"),
            ));
            settings.push(setting(
                &glass,
                "appearance.glass_colour",
                (tr("The glass's colour"), None, false),
                Control::choice("appearance.glass_colour"),
            ));
            widgets::heading(&content, tr("Workspace switcher"));
            let switcher = widgets::group(&content);
            settings.push(setting(
                &switcher,
                "appearance.switcher_look",
                (tr("How the panel's switcher shows them"), None, false),
                Control::choice("appearance.switcher_look"),
            ));
            settings.push(setting(
                &switcher,
                "appearance.switcher_shown",
                (
                    tr("How many numbers the switcher shows at once"),
                    None,
                    false,
                ),
                Control::numbers(),
            ));
            settings.push(setting(
                &switcher,
                "appearance.switcher_ends",
                (tr("What shows where more workspaces lie"), None, false),
                Control::choice("appearance.switcher_ends"),
            ));
        }
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
        section,
        presets: std::cell::RefCell::new(Vec::new()),
        own,
        theme: theme.clone(),
        preset_source,
        settings,
        bar,
        problem,
        changes: widgets::ChangeBar::new(&page),
        before: std::cell::RefCell::new(files.own_keys(section)),
        quiet: Cell::new(false),
        monitors,
        tray: tray.clone(),
        panels: panels.clone(),
        names,
        theme_card: theme_card.clone(),
        colours: colour_group.clone(),
        shape: shape_group.clone(),
        text: text_group.clone(),
        depth: depth_group.clone(),
        cursor: cursor_group.clone(),
        motion: motion_group.clone(),
        desktop: desktop_group.clone(),
    });
    sync_cards(&ui);
    ui.update();
    if let Some(tray) = &tray {
        tray.refresh();
    }
    for monitor in &ui.monitors {
        let weak = Rc::downgrade(&ui);
        monitor.connect_changed(move |_, _, _, _| {
            if let Some(ui) = weak.upgrade() {
                sync_cards(&ui);
                ui.update();
            }
        });
    }
    if let Some(own) = &ui.own {
        log_preset_places(&own.cards, Rc::downgrade(&ui));
        let weak = Rc::downgrade(&ui);
        own.save.connect_clicked(move |button| {
            if let Some(ui) = weak.upgrade() {
                ask_save_as(&ui, button.upcast_ref());
            }
        });
    }
    if let Some(source) = &ui.preset_source {
        wire(&ui, &source.reset, &source.copy, vec!["layout.preset"]);
    }
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
    if let Some(panels) = &panels {
        let edit = panels.edit();
        page.connect_map(move |page| {
            let (page, edit) = (page.clone(), edit.clone());
            gtk::glib::idle_add_local_once(move || {
                if widgets::asked_row() == Some(edel::panel_edit::PANELS) {
                    widgets::take_asked(&page, &edit);
                }
            });
        });
    }
    let weak = Rc::downgrade(&ui);
    ui.changes.undo.connect_clicked(move |_| {
        if let Some(ui) = weak.upgrade() {
            let before = ui.before.borrow().clone();
            ui.report(Files::here().restore(&before));
            sync_cards(&ui);
            ui.update();
        }
    });
    let weak = Rc::downgrade(&ui);
    ui.changes.keep.connect_clicked(move |_| {
        if let Some(ui) = weak.upgrade() {
            *ui.before.borrow_mut() = Files::here().own_keys(ui.section);
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

/// Rebuilds the preset cards when the presets the files hold changed (M5.17):
/// one per row of `edel::presets::rows`, the built-in ones first, each card
/// a click that chooses it, and the person's own ones a right click that
/// deletes them. Nothing is rebuilt while the names stay the same.
fn sync_cards(ui: &Rc<Ui>) {
    let Some(own) = &ui.own else { return };
    let file = edel::settings::SettingsFile {
        presets: Files::here().own_presets(),
        ..Default::default()
    };
    let rows = edel::presets::rows(&file);
    // A card's picture is its base's, so the base is part of what it shows.
    let names: Vec<String> = rows
        .iter()
        .map(|row| format!("{} {}", row.name, row.base))
        .collect();
    if *own.names.borrow() == names {
        return;
    }
    *own.names.borrow_mut() = names;
    while let Some(child) = own.cards.first_child() {
        own.cards.remove(&child);
    }
    let mut cards = Vec::new();
    let mut first: Option<gtk::ToggleButton> = None;
    for row in rows {
        // An own preset's picture is its base's, under its own name.
        let picture = preview::area(&row.base, &ui.theme).upcast::<gtk::Widget>();
        let (card, badge) = widgets::picture_card(&row.title, &picture);
        match &first {
            Some(first) => card.set_group(Some(first)),
            None => first = Some(card.clone()),
        }
        own.cards.append(&card);
        // The card takes the focus, not the box the flow wraps it in.
        if let Some(child) = card.parent() {
            child.set_focusable(false);
        }
        if !row.built_in {
            attach_delete(ui, &card, &row.name);
        }
        let (name, weak) = (row.name.clone(), Rc::downgrade(ui));
        card.connect_toggled(move |card| {
            let Some(ui) = weak.upgrade() else { return };
            if card.is_active() && !ui.quiet.get() {
                ui.report(Files::here().choose("layout.preset", &name));
                // A preset brings its own policy and buttons unless set.
                ui.update();
            }
        });
        cards.push((row.name, card, badge));
    }
    *ui.presets.borrow_mut() = cards;
}

/// Gives an own preset's card its right click menu with Delete (M5.17). The
/// preset in use is refused first, with the library's words under the cards,
/// and any other is asked about in a dialog, as the theme's Delete is.
fn attach_delete(ui: &Rc<Ui>, card: &gtk::ToggleButton, name: &str) {
    let menu = gtk::Popover::builder()
        .css_classes(["edel-choice-list"])
        .build();
    let items = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    let delete = widgets::menu_item(tr("Delete…"));
    delete.add_css_class("edel-danger");
    items.append(&delete);
    menu.set_child(Some(&items));
    menu.set_parent(card);
    let click = gtk::GestureClick::builder().button(3).build();
    let open = menu.clone();
    click.connect_pressed(move |_, _, x, y| {
        open.set_pointing_to(Some(&gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        open.popup();
    });
    card.add_controller(click);
    let (name, weak, close) = (name.to_string(), Rc::downgrade(ui), menu.clone());
    delete.connect_clicked(move |button| {
        close.popdown();
        let Some(ui) = weak.upgrade() else { return };
        if let Err(why) = edel::presets::delete_text(&Files::here().person_text(), &name)
            .map(|_| ())
            .map_err(|e| format!("{e:#}"))
        {
            ui.report_preset(Err(why));
            return;
        }
        let (weak, asked) = (Rc::downgrade(&ui), name.clone());
        widgets::ask_delete(
            button.upcast_ref(),
            &trf("Delete the preset {preset}?", &[("preset", &asked)]),
            tr("It cannot be brought back."),
            tr("Delete"),
            move || {
                let Some(ui) = weak.upgrade() else { return };
                let result = Files::here().delete_preset(&asked);
                if result.is_ok() {
                    eprintln!("edel-settings: preset {asked} deleted");
                }
                ui.report_preset(result);
                sync_cards(&ui);
                ui.update();
            },
        );
    });
}

/// Asks for the name of the layout in use kept as a preset of the person's
/// own (Save as preset, M5.17): the name is checked by the library, and a
/// name it refuses is said in the dialog. The layout keeps its values, as
/// `edel settings` says the screen stays the same.
fn ask_save_as(ui: &Rc<Ui>, near: &gtk::Widget) {
    let files = Files::here();
    let mut taken: Vec<String> = files.own_presets().into_keys().collect();
    taken.extend(edel::presets::NAMES.iter().map(|name| name.to_string()));
    let suggestion = edel::themes::free_name("my preset", |name| taken.iter().any(|t| t == name));
    let weak = Rc::downgrade(ui);
    widgets::ask_name(
        near,
        tr("Save as preset"),
        tr(
            "Give the preset a name. It keeps the layout you changed, and the preset in use becomes it.",
        ),
        &suggestion,
        tr("Name, such as my-hive"),
        tr("Save as preset"),
        move |name| {
            let Some(ui) = weak.upgrade() else {
                return Ok(());
            };
            Files::here().save_preset(name)?;
            eprintln!("edel-settings: preset {name} saved");
            sync_cards(&ui);
            ui.update();
            Ok(())
        },
    );
}

/// Logs where each preset card lies in the window, once the cards are laid
/// out and two frames agree, as `edel-settings: layout presets places NAME
/// X+Y+WxH, ...`, for CI to click the cards by (logical pixels).
fn log_preset_places(cards: &gtk::FlowBox, ui: std::rc::Weak<Ui>) {
    let last = std::cell::RefCell::new(String::new());
    cards.add_tick_callback(move |cards, _| {
        let Some(window) = cards.root().and_downcast::<gtk::Window>() else {
            return glib::ControlFlow::Continue;
        };
        if cards.width() == 0 || !cards.is_mapped() {
            return glib::ControlFlow::Continue;
        }
        let Some(ui) = ui.upgrade() else {
            return glib::ControlFlow::Break;
        };
        let places: Vec<String> = ui
            .presets
            .borrow()
            .iter()
            .map(|(name, card, _)| {
                format!("{name} {}", widgets::place(card.compute_bounds(&window)))
            })
            .collect();
        let line = format!("edel-settings: layout presets places {}", places.join(", "));
        if last.replace(line.clone()) != line {
            return glib::ControlFlow::Continue;
        }
        eprintln!("{line}");
        glib::ControlFlow::Break
    });
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
    // Segments rows answer their key themselves (`log_places`), with the
    // chosen button taking the keyboard.
    if !matches!(control, Control::Segments(..)) {
        widgets::show_when_asked(key, &widget);
    }
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

/// Logs where a row of buttons (the workspaces' count, light or dark) and
/// its Reset lie in the window, once `heading`'s group is laid out, for CI
/// to click (logical pixels), as `edel-settings: GROUP group places WHAT
/// P1 P2 ..., reset R`. Asked for by the row's key, the page first scrolls
/// the group to the top of its view and gives the keyboard to the chosen
/// button, as the Panels group does.
fn log_places(
    page: &gtk::Widget,
    heading: &gtk::Box,
    count: &Setting,
    (group, what): (&'static str, &'static str),
) {
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
    let last = std::cell::RefCell::new(String::new());
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
        let line = format!(
            "edel-settings: {group} group places {what} {}, reset {}",
            counts.join(" "),
            widgets::place(reset.compute_bounds(&window)),
        );
        // The view settles over a few frames, and a group near the page's
        // end stops short of the top, so the places are logged only once
        // two frames agree.
        if last.replace(line.clone()) != line {
            return glib::ControlFlow::Continue;
        }
        eprintln!("{line}");
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
