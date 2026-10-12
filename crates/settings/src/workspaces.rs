//! The Workspaces group's names and apps rows (M5.2n), under the count,
//! the switch and the switcher's choices in the Layout page: one entry per
//! workspace for its name, which the panel's switcher shows instead of the
//! number, and the apps that always open on a workspace, each with its
//! number, a button to remove it and a row to add one. Every write is one
//! key of the person's file through `Files::choose`, so the file is what
//! `edel settings set` writes; the rows follow the files and never replace
//! the text a person is typing.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;

use edel::i18n::{tr, trf};
use edel::presets::MOST_WORKSPACES;
use edel::{places, wallpapers};

use crate::files::{self, Files, apps_text, list_text};
use crate::{icon, rows, wallpaper, widgets};

/// The key of the names row, the wallpapers row and the apps row.
const NAMES: &str = "workspaces.names";
const WALLPAPERS: &str = "workspaces.wallpapers";
const APPS: &str = "workspaces.apps";
/// Appearance's wallpaper key, which an empty choice means (M5.12b5).
const APPEARANCE: &str = "appearance.wallpaper";
/// A workspace's wallpaper thumbnail in logical pixels, small beside its
/// name's row (M5.12b5).
const THUMB_WIDTH: i32 = 64;
const THUMB_HEIGHT: i32 = 40;

/// The Workspaces group's names and apps, as the page shows them.
pub struct Card {
    problem: gtk::Label,
    /// Set while the card itself moves a control, which is not a choice.
    quiet: Cell<bool>,
    names_row: widgets::Row,
    names_box: gtk::Box,
    /// One entry per workspace row shown, in order.
    entries: RefCell<Vec<gtk::Entry>>,
    /// The names as the files say them now.
    names: RefCell<Vec<String>>,
    wallpapers_row: widgets::Row,
    wallpapers_box: gtk::Box,
    /// The entries and Appearance's wallpaper the rows were built for: the
    /// rows are built again only when one of them changes.
    wallpapers_built: RefCell<Option<(Vec<String>, String)>>,
    /// Each workspace's choice and the values it offers, in order.
    wallpaper_rows: RefCell<Vec<(Rc<widgets::Choice>, Vec<String>)>>,
    apps_row: widgets::Row,
    apps_box: gtk::Box,
    /// The rules shown, each app with its number's spin button.
    rules: RefCell<Vec<(String, gtk::SpinButton)>>,
    add_app: gtk::Entry,
    add_number: gtk::SpinButton,
    add_button: gtk::Button,
}

/// Appends the names and apps rows to `group`, which holds the Workspaces
/// rows of the Layout page. Call [`Card::show`] once the page is built and
/// whenever a file changes.
pub fn card(group: &gtk::Box, problem: &gtk::Label) -> Rc<Card> {
    let empty = || gtk::Box::builder().build().upcast::<gtk::Widget>();
    let names_row = widgets::row(group, rows::title(NAMES), &empty());
    let names_box = vertical();
    group.append(&names_box);
    let wallpapers_row = widgets::row(group, rows::title(WALLPAPERS), &empty());
    let wallpapers_box = vertical();
    group.append(&wallpapers_box);
    let apps_row = widgets::row(group, rows::title(APPS), &empty());
    let apps_box = vertical();
    group.append(&apps_box);

    let add_app = gtk::Entry::builder()
        .placeholder_text(tr("App id, such as org.mozilla.firefox"))
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build();
    let add_number = number_spin(1);
    let add_button = widgets::action(tr("Add"), false);
    let add = gtk::Box::builder().spacing(8).build();
    add.append(&add_app);
    add.append(&add_number);
    add.append(&add_button);
    let add_row = widgets::row(group, tr("Add an app"), &add.upcast::<gtk::Widget>());
    add_row.subtitle.set_visible(false);
    add_row.reset.set_visible(false);
    add_row.copy.set_visible(false);

    let card = Rc::new(Card {
        problem: problem.clone(),
        quiet: Cell::new(false),
        names_row,
        names_box,
        entries: RefCell::new(Vec::new()),
        names: RefCell::new(Vec::new()),
        wallpapers_row,
        wallpapers_box,
        wallpapers_built: RefCell::new(None),
        wallpaper_rows: RefCell::new(Vec::new()),
        apps_row,
        apps_box,
        rules: RefCell::new(Vec::new()),
        add_app,
        add_number,
        add_button,
    });
    card.wire();
    card
}

/// A vertical box for rows to be built in.
fn vertical() -> gtk::Box {
    gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build()
}

/// A spin button for a workspace number, 1 to the most there may be.
fn number_spin(number: i64) -> gtk::SpinButton {
    let spin = gtk::SpinButton::with_range(1.0, MOST_WORKSPACES as f64, 1.0);
    spin.set_digits(0);
    spin.set_value(number as f64);
    spin.set_valign(gtk::Align::Center);
    spin.update_property(&[gtk::accessible::Property::Label(tr("Workspace"))]);
    spin
}

/// The names as they are kept: no empty names at the end.
fn tidy(mut names: Vec<String>) -> Vec<String> {
    while names.last().is_some_and(String::is_empty) {
        names.pop();
    }
    names
}

/// How many workspaces the rows show: the names' rows, the wallpapers' too.
fn workspace_count(now: &files::Layout) -> usize {
    if now.dynamic_workspaces {
        (now.workspace_names.len() + 1).min(MOST_WORKSPACES)
    } else {
        now.workspaces as usize
    }
}

/// The values a workspace's wallpaper choice offers, in order: an empty
/// one (Appearance's), each wallpaper in `names`, then the workspace's own
/// value when it is none of those (a colour or a picture's path), so the
/// choice always shows what the file says (M5.12b5).
pub fn wallpaper_values(names: &[String], entry: &str) -> Vec<String> {
    let mut values = vec![String::new()];
    values.extend(names.iter().cloned());
    if !entry.is_empty() && !values.iter().any(|v| v == entry) {
        values.push(entry.to_string());
    }
    values
}

impl Card {
    /// Shows what the files say now, without writing anything. Rows are
    /// built again only when their number changes; otherwise each shows
    /// its value, except an entry that has the keyboard.
    pub fn show(self: &Rc<Self>, files: &Files, now: &files::Layout) {
        self.quiet.set(true);
        self.show_names(files, now);
        self.show_wallpapers(files, now);
        self.show_apps(files, now);
        self.quiet.set(false);
    }

    /// Shows the wallpapers heading's line, and one row per workspace with
    /// its thumbnail and its choice (M5.12b5). The rows are built again
    /// only when an entry or Appearance's wallpaper changes.
    fn show_wallpapers(self: &Rc<Self>, files: &Files, now: &files::Layout) {
        let source = files.source(WALLPAPERS);
        let hint = tr("Each workspace can show its own wallpaper; an empty choice is Appearance's");
        let line = match rows::note(&source, false) {
            Some(note) => format!("{hint} · {note}"),
            None => hint.to_string(),
        };
        self.wallpapers_row.subtitle.set_label(&line);
        self.wallpapers_row
            .reset
            .set_visible(rows::resettable(&source));

        let dir = PathBuf::from(places::WALLPAPERS_DIR);
        let appearance = wallpaper::shown(files.chosen(APPEARANCE), &dir);
        let entries: Vec<String> = (0..workspace_count(now))
            .map(|i| now.workspace_wallpapers.get(i).cloned().unwrap_or_default())
            .collect();
        let built = Some((entries.clone(), appearance.clone()));
        if *self.wallpapers_built.borrow() == built {
            return;
        }
        *self.wallpapers_built.borrow_mut() = built;
        self.build_wallpapers(&dir, &entries, &appearance);
    }

    /// Builds one row per workspace, replacing the rows shown: its number,
    /// a thumbnail of its wallpaper (Appearance's for an empty choice) and
    /// the choice.
    fn build_wallpapers(self: &Rc<Self>, dir: &Path, entries: &[String], appearance: &str) {
        while let Some(child) = self.wallpapers_box.first_child() {
            self.wallpapers_box.remove(&child);
        }
        let names = wallpapers::names(dir);
        let mut built = Vec::new();
        let mut controls = Vec::new();
        for (i, entry) in entries.iter().enumerate() {
            let values = wallpaper_values(&names, entry);
            let labels: Vec<String> = values
                .iter()
                .map(|v| {
                    if v.is_empty() {
                        tr("Same as Appearance").to_string()
                    } else {
                        wallpaper::title_of(v)
                    }
                })
                .collect();
            let choice = widgets::Choice::new(labels);
            choice.set_selected(values.iter().position(|v| v == entry).unwrap_or(0));
            let shown: &str = if entry.is_empty() {
                appearance
            } else {
                entry.as_str()
            };
            let picture = wallpaper::reference_picture_at(dir, shown, THUMB_WIDTH, THUMB_HEIGHT);
            let place = gtk::Box::builder()
                .spacing(10)
                .valign(gtk::Align::Center)
                .build();
            place.append(&picture);
            place.append(&choice.widget());
            let title = trf("Workspace {n}", &[("n", &(i + 1).to_string())]);
            let control = place.clone().upcast::<gtk::Widget>();
            let row = widgets::row(&self.wallpapers_box, &title, &control);
            row.subtitle.set_visible(false);
            row.reset.set_visible(false);
            row.copy.set_visible(false);
            // The choice holds its callback weakly, so the two do not keep
            // each other alive.
            let weak = Rc::downgrade(self);
            let weak_choice = Rc::downgrade(&choice);
            let offered = values.clone();
            choice.connect_changed(move || {
                let (Some(card), Some(choice)) = (weak.upgrade(), weak_choice.upgrade()) else {
                    return;
                };
                let value = offered.get(choice.selected()).cloned().unwrap_or_default();
                card.choose_wallpaper(i, value);
            });
            controls.push(control);
            built.push((choice, values));
        }
        *self.wallpaper_rows.borrow_mut() = built;
        log_places(&self.wallpapers_box, controls);
    }

    /// Writes workspace `index`'s wallpaper as `value` (`""` for
    /// Appearance's) into the list, trailing empties dropped, the key taken
    /// out when no entry is left (M5.12b5). Entries past the rows shown
    /// stay as they are.
    fn choose_wallpaper(self: &Rc<Self>, index: usize, value: String) {
        if self.quiet.get() {
            return;
        }
        let files = Files::here();
        let now = files.layout();
        let mut list = now.workspace_wallpapers.clone();
        let count = self.wallpaper_rows.borrow().len().max(index + 1);
        if list.len() < count {
            list.resize(count, String::new());
        }
        list[index] = value;
        let list = tidy(list);
        if list == tidy(now.workspace_wallpapers.clone()) {
            return;
        }
        let result = files.choose(WALLPAPERS, &list_text(&list));
        if result.is_ok() {
            let done = if list.is_empty() {
                "taken out"
            } else {
                "WRITTEN"
            };
            eprintln!("edel-settings: workspace wallpapers {done}");
        }
        self.report(result);
        self.refresh();
    }

    /// Shows the names heading's line, and one row per workspace.
    fn show_names(self: &Rc<Self>, files: &Files, now: &files::Layout) {
        let source = files.source(NAMES);
        let hint = tr("A name shows in the panel's switcher instead of the number");
        let line = match rows::note(&source, false) {
            Some(note) => format!("{hint} · {note}"),
            None => hint.to_string(),
        };
        self.names_row.subtitle.set_label(&line);
        self.names_row.reset.set_visible(rows::resettable(&source));

        let wanted = workspace_count(now);
        *self.names.borrow_mut() = now.workspace_names.clone();
        if self.entries.borrow().len() == wanted {
            for (i, entry) in self.entries.borrow().iter().enumerate() {
                let name = now.workspace_names.get(i).map_or("", String::as_str);
                if !entry.has_focus() && entry.text().as_str() != name {
                    entry.set_text(name);
                }
            }
        } else {
            self.build_names(wanted, &now.workspace_names);
        }
    }

    /// Builds one name row per workspace, replacing the rows shown.
    fn build_names(self: &Rc<Self>, count: usize, names: &[String]) {
        while let Some(child) = self.names_box.first_child() {
            self.names_box.remove(&child);
        }
        let mut entries = Vec::new();
        for i in 0..count {
            let entry = gtk::Entry::builder()
                .placeholder_text(tr("No name"))
                .max_width_chars(16)
                .valign(gtk::Align::Center)
                .build();
            entry.set_text(names.get(i).map_or("", String::as_str));
            let title = trf("Workspace {n}", &[("n", &(i + 1).to_string())]);
            let control = entry.clone().upcast::<gtk::Widget>();
            let row = widgets::row(&self.names_box, &title, &control);
            row.subtitle.set_visible(false);
            row.reset.set_visible(false);
            row.copy.set_visible(false);
            self.wire_entry(&entry);
            entries.push(entry);
        }
        *self.entries.borrow_mut() = entries;
    }

    /// Shows the apps heading's line, and one row per rule.
    fn show_apps(self: &Rc<Self>, files: &Files, now: &files::Layout) {
        let source = files.source(APPS);
        let hint = tr("These apps always open on their workspace");
        let line = match rows::note(&source, false) {
            Some(note) => format!("{hint} · {note}"),
            None => hint.to_string(),
        };
        self.apps_row.subtitle.set_label(&line);
        self.apps_row.reset.set_visible(rows::resettable(&source));

        let same = {
            let rules = self.rules.borrow();
            rules.len() == now.app_workspaces.len()
                && rules
                    .iter()
                    .zip(&now.app_workspaces)
                    .all(|((shown, _), (app, _))| shown == app)
        };
        if same {
            for ((_, spin), number) in self.rules.borrow().iter().zip(now.app_workspaces.values()) {
                spin.set_value(*number as f64);
            }
        } else {
            self.build_rules(&now.app_workspaces);
        }
    }

    /// Builds one row per app in `apps`, replacing the rows shown.
    fn build_rules(self: &Rc<Self>, apps: &BTreeMap<String, i64>) {
        while let Some(child) = self.apps_box.first_child() {
            self.apps_box.remove(&child);
        }
        let mut rules = Vec::new();
        for (app, number) in apps {
            let spin = number_spin(*number);
            let remove = gtk::Button::builder()
                .child(&icon::image("close", 12))
                .tooltip_text(tr("Remove"))
                .valign(gtk::Align::Center)
                .css_classes(["edel-copy"])
                .build();
            remove.update_property(&[gtk::accessible::Property::Label(tr("Remove"))]);
            let place = gtk::Box::builder().spacing(8).build();
            place.append(&spin);
            place.append(&remove);
            let control = place.upcast::<gtk::Widget>();
            let row = widgets::row(&self.apps_box, app, &control);
            row.subtitle.set_visible(false);
            row.reset.set_visible(false);
            row.copy.set_visible(false);
            self.wire_rule(app, &spin, &remove);
            rules.push((app.clone(), spin));
        }
        *self.rules.borrow_mut() = rules;
    }

    /// Connects Reset and Copy as command of both headings, the add row and
    /// the names' and apps' handlers that build later.
    fn wire(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.names_row.reset.connect_clicked(move |_| {
            if let Some(card) = weak.upgrade() {
                card.report(Files::here().set(NAMES, None));
                card.refresh();
            }
        });
        self.names_row.copy.connect_clicked(|button| {
            let names = Files::here().layout().workspace_names;
            button
                .clipboard()
                .set_text(&rows::command(&[(NAMES, list_text(&names))]));
            widgets::copied(button);
        });
        let weak = Rc::downgrade(self);
        self.wallpapers_row.reset.connect_clicked(move |_| {
            if let Some(card) = weak.upgrade() {
                card.report(Files::here().set(WALLPAPERS, None));
                card.refresh();
            }
        });
        self.wallpapers_row.copy.connect_clicked(|button| {
            let list = Files::here().layout().workspace_wallpapers;
            button
                .clipboard()
                .set_text(&rows::command(&[(WALLPAPERS, list_text(&list))]));
            widgets::copied(button);
        });
        let weak = Rc::downgrade(self);
        self.apps_row.reset.connect_clicked(move |_| {
            if let Some(card) = weak.upgrade() {
                card.report(Files::here().set(APPS, None));
                card.refresh();
            }
        });
        self.apps_row.copy.connect_clicked(|button| {
            let apps = Files::here().layout().app_workspaces;
            button
                .clipboard()
                .set_text(&rows::command(&[(APPS, apps_text(&apps))]));
            widgets::copied(button);
        });
        let weak = Rc::downgrade(self);
        self.add_app.connect_activate(move |_| {
            if let Some(card) = weak.upgrade() {
                card.add_rule();
            }
        });
        let weak = Rc::downgrade(self);
        self.add_button.connect_clicked(move |_| {
            if let Some(card) = weak.upgrade() {
                card.add_rule();
            }
        });
    }

    /// Commits a name when its entry is confirmed or loses the keyboard.
    fn wire_entry(self: &Rc<Self>, entry: &gtk::Entry) {
        let weak = Rc::downgrade(self);
        entry.connect_activate(move |_| {
            if let Some(card) = weak.upgrade() {
                card.commit_names();
            }
        });
        let focus = gtk::EventControllerFocus::new();
        let weak = Rc::downgrade(self);
        focus.connect_leave(move |_| {
            if let Some(card) = weak.upgrade() {
                card.commit_names();
            }
        });
        entry.add_controller(focus);
    }

    /// Writes the names the entries hold, when they changed. Names past the
    /// rows shown stay as they are, and trailing empty names are dropped.
    fn commit_names(self: &Rc<Self>) {
        if self.quiet.get() {
            return;
        }
        let shown = self.names.borrow().clone();
        let mut list: Vec<String> = self
            .entries
            .borrow()
            .iter()
            .map(|e| e.text().trim().to_string())
            .collect();
        let rest: Vec<String> = shown.iter().skip(list.len()).cloned().collect();
        list.extend(rest);
        let list = tidy(list);
        if list == tidy(shown) {
            return;
        }
        self.report(Files::here().choose(NAMES, &list_text(&list)));
        self.refresh();
    }

    /// Writes the table with `change` made to it, from the apps that apply
    /// now, when the card is not moving its own controls.
    fn write_apps(self: &Rc<Self>, change: impl FnOnce(&mut BTreeMap<String, i64>)) {
        if self.quiet.get() {
            return;
        }
        let files = Files::here();
        let mut table = files.layout().app_workspaces;
        change(&mut table);
        self.report(files.choose(APPS, &apps_text(&table)));
        self.refresh();
    }

    /// Adds the app typed in the add row with its number, then clears the
    /// row; nothing happens for an empty id.
    fn add_rule(self: &Rc<Self>) {
        let app = self.add_app.text().trim().to_string();
        if app.is_empty() {
            return;
        }
        let number = i64::from(self.add_number.value_as_int());
        let files = Files::here();
        let mut table = files.layout().app_workspaces;
        table.insert(app, number);
        match files.choose(APPS, &apps_text(&table)) {
            Ok(()) => {
                self.add_app.set_text("");
                self.report(Ok(()));
            }
            Err(e) => self.report(Err(e)),
        }
        self.refresh();
    }

    /// Connects an app's number and its remove button.
    fn wire_rule(self: &Rc<Self>, app: &str, spin: &gtk::SpinButton, remove: &gtk::Button) {
        let weak = Rc::downgrade(self);
        let name = app.to_string();
        spin.connect_value_changed(move |spin| {
            let Some(card) = weak.upgrade() else { return };
            let number = i64::from(spin.value_as_int());
            card.write_apps(|table| {
                table.insert(name.clone(), number);
            });
        });
        let weak = Rc::downgrade(self);
        let name = app.to_string();
        remove.connect_clicked(move |_| {
            if let Some(card) = weak.upgrade() {
                card.write_apps(|table| {
                    table.remove(&name);
                });
            }
        });
    }

    /// Shows the files' values again after a change made here.
    fn refresh(self: &Rc<Self>) {
        let files = Files::here();
        let now = files.layout();
        self.show(&files, &now);
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

/// Logs where each workspace's wallpaper control lies in the window, once
/// the rows are laid out, for CI and the screenshots (logical pixels), as
/// `edel-settings: workspace wallpapers places wallpaper1 X+Y+WxH, ...`;
/// logged once two frames agree, as the other groups' places are.
fn log_places(group: &gtk::Box, controls: Vec<gtk::Widget>) {
    let last = RefCell::new(String::new());
    group.add_tick_callback(move |group, _| {
        let Some(window) = group.root().and_downcast::<gtk::Window>() else {
            return glib::ControlFlow::Continue;
        };
        if !group.is_mapped() || group.width() == 0 {
            return glib::ControlFlow::Continue;
        }
        let places: Vec<String> = controls
            .iter()
            .enumerate()
            .map(|(i, control)| {
                format!(
                    "wallpaper{} {}",
                    i + 1,
                    widgets::place(control.compute_bounds(&window))
                )
            })
            .collect();
        let line = format!(
            "edel-settings: workspace wallpapers places {}",
            places.join(", ")
        );
        if last.replace(line.clone()) != line {
            return glib::ControlFlow::Continue;
        }
        eprintln!("{line}");
        glib::ControlFlow::Break
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trailing_empty_names_are_dropped_and_the_middle_kept() {
        let names = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<String>>();
        assert_eq!(
            tidy(names(&["Mail", "", "Chat", "", ""])),
            ["Mail", "", "Chat"]
        );
        assert!(tidy(names(&["", ""])).is_empty());
    }

    #[test]
    fn a_wallpaper_choice_offers_appearances_then_the_names_then_its_own_value() {
        let names = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<String>>();
        let wallpapers = names(&["aurora", "hills"]);
        assert_eq!(wallpaper_values(&wallpapers, ""), ["", "aurora", "hills"]);
        assert_eq!(
            wallpaper_values(&wallpapers, "#336699"),
            ["", "aurora", "hills", "#336699"],
            "a colour the names do not hold is offered as it is"
        );
        assert_eq!(
            wallpaper_values(&wallpapers, "hills"),
            ["", "aurora", "hills"]
        );
    }
}
