//! The Apps page (roadmap M6.14a): what each app may use. The page is
//! a list and a detail view in one `gtk::Stack`. The list holds every app on
//! this computer, Flatpak apps and others together by name, a search field
//! that filters it live by name or id, and under each name a line that says
//! what the app may use (`summary`). Opening an app shows its page, built anew
//! each time from `SECTIONS`: a table of parts, each a heading, when it shows
//! and what it builds, so the page grows by one entry per part. The first part
//! is what a Flatpak app may use: a choice per permission of
//! `edel::app_permissions` (Camera, Screenshots, Notifications and Files), each
//! one app's entry in that key's table of the person's settings file, such as
//! `camera = { "org.gnome.Snapshot" = "never" }`. The choice without an entry
//! is the one that applies without the key, so its entry is taken out rather
//! than written (ADR-008). An app that is not a Flatpak app is not sandboxed,
//! so its page says so and offers no choice. The page follows the files while
//! it is open: the summaries and the open app's choices are shown again
//! whenever a file changes. shell-ui lays the choices onto Flatpak's own
//! state. `--app ID` opens one app's page when the page is built.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use gtk::gio;
use gtk::prelude::*;
use gtk::{gdk, glib};

use edel::app_icons;
use edel::app_permissions::{self, ALLOW, ASK, NEVER, PERMISSIONS, Permission, Place};
use edel::apps::{self, App};
use edel::i18n::{n_, tr, trf};
use edel::settings::{self, PAGES};

use crate::files::Files;
use crate::{icon, widgets};

/// The side of an app's icon in the list, and on its page, in logical pixels.
const LIST_ICON: i32 = 32;
const PAGE_ICON: i32 = 64;

thread_local! {
    /// The app `--app ID` asked for, taken when the page is built (M6.14a).
    static ASKED_APP: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Asks the page to show the app `id` when it is built, as `--app ID` does.
pub fn open_app(id: &str) {
    ASKED_APP.with(|asked| *asked.borrow_mut() = Some(id.to_string()));
}

/// The names of the apps on this computer, read once for the sidebar's
/// search, which finds the Apps page by an app's name (M6.14a).
pub fn names() -> &'static [String] {
    static NAMES: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    NAMES.get_or_init(|| {
        apps::read_all(&apps::dirs())
            .into_iter()
            .map(|app| app.name)
            .collect()
    })
}

/// One choice on an app's page: one app's one permission.
struct Slot {
    /// The settings key, such as `apps.camera`.
    key: &'static str,
    /// The app's Flatpak id, the name of its entry in the key's table.
    app: String,
    /// What each item of the choice writes: `None` is the choice without
    /// an entry, the one that applies without the key.
    items: Vec<Option<&'static str>>,
    choice: Rc<widgets::Choice>,
}

/// One part of an app's page (M6.14a): what it may use today; later the
/// app's workspace, startup, pins and storage join as one entry each.
struct Section {
    /// Its heading.
    title: &'static str,
    /// Whether it shows for this app.
    shown: fn(&App) -> bool,
    /// Builds it under its heading.
    build: fn(&gtk::Box, &App, &Rc<Ui>),
}

/// The parts of an app's page, in the order they show. Two parts share a
/// heading: each shows for one kind of app.
const SECTIONS: &[Section] = &[
    Section {
        title: n_("What it may use"),
        shown: sandboxed,
        build: permissions,
    },
    Section {
        title: n_("What it may use"),
        shown: unsandboxed,
        build: not_sandboxed,
    },
];

/// One app's row in the list.
struct ListRow {
    button: gtk::Button,
    summary: gtk::Label,
}

struct Ui {
    /// The apps, sorted by name; `rows` has one row for each, in this order.
    apps: Vec<App>,
    rows: Vec<ListRow>,
    /// The list and the app's page, one view at a time.
    stack: gtk::Stack,
    /// Holds the open app's page, built again each time one is opened.
    detail: gtk::Box,
    /// The line the list shows when a search finds no app.
    nothing: gtk::Label,
    /// The choices on the open app's page, one per permission.
    slots: RefCell<Vec<Slot>>,
    /// The row of the app whose page is open, to which the focus goes back.
    opened: Cell<Option<usize>>,
    problem: gtk::Label,
    /// Kept so the page follows the files while it is open.
    monitors: Vec<gio::FileMonitor>,
}

impl Ui {
    /// Shows what the files give now: each app's summary in the list and the
    /// choices on the open app's page. `Choice::set_selected` only shows a
    /// value and never calls back, so this writes nothing.
    fn update(&self) {
        let files = Files::here();
        let (machine, person) = files.texts();
        let all = app_permissions::choices_between(machine.as_deref(), Some(&person));
        for (app, row) in self.apps.iter().zip(&self.rows) {
            let text = summary(sandboxed(app), &entries_of(&all, &app.id));
            row.summary.set_label(&text);
            row.button
                .update_property(&[gtk::accessible::Property::Label(&trf(
                    "{name}, {summary}",
                    &[("name", &app.name), ("summary", &text)],
                ))]);
        }
        for slot in self.slots.borrow().iter() {
            let entry = all
                .get(slot.key)
                .and_then(|table| table.get(&slot.app))
                .map(String::as_str);
            // An app with no entry shows the first item, the choice that
            // applies without one.
            let at = slot
                .items
                .iter()
                .position(|item| *item == entry)
                .unwrap_or(0);
            slot.choice.set_selected(at);
        }
    }

    /// Writes the choice slot `index` now shows to the person's file, or
    /// takes the app's entry out for the choice without one.
    fn choose(&self, index: usize) {
        let (key, app, value) = {
            let slots = self.slots.borrow();
            let Some(slot) = slots.get(index) else {
                return;
            };
            (
                slot.key,
                slot.app.clone(),
                slot.items.get(slot.choice.selected()).copied().flatten(),
            )
        };
        let files = Files::here();
        let result = table_text(&files.person_text(), key, &app, value)
            .and_then(|text| files.set(key, text.as_deref()));
        // The CI line says what was written for an app, as the other pages do.
        if result.is_ok() {
            let done = if value.is_some() {
                "written"
            } else {
                "taken out"
            };
            eprintln!("edel-settings: apps {key} {app} {done}");
        }
        self.report(result);
        self.update();
    }

    /// Shows why a choice was not written, or takes an old message away.
    fn report(&self, result: Result<(), String>) {
        match result {
            Ok(()) => self.problem.set_visible(false),
            Err(e) => {
                self.problem.set_label(&e);
                self.problem.set_visible(true);
            }
        }
    }

    /// Shows the rows whose name or id holds `query`, and the line that says
    /// none does when none does (M6.14a).
    fn filter(&self, query: &str) {
        let mut found = false;
        for (app, row) in self.apps.iter().zip(&self.rows) {
            let shown = matches(&app.name, &app.id, query);
            row.button.set_visible(shown);
            found |= shown;
        }
        self.nothing.set_visible(!found && !query.trim().is_empty());
    }

    /// Shows the page of the app at `at` in `apps`: its header, then each part
    /// of `SECTIONS` that shows for it. Built anew each time, so the choices
    /// of the page before are dropped (M6.14a).
    fn open(self: &Rc<Self>, at: usize) {
        let Some(app) = self.apps.get(at) else {
            return;
        };
        *self.slots.borrow_mut() = Vec::new();
        while let Some(child) = self.detail.first_child() {
            self.detail.remove(&child);
        }
        let back = back_button(self);
        self.detail.append(&back);
        self.detail.append(&header(app));
        for section in SECTIONS.iter().filter(|section| (section.shown)(app)) {
            widgets::heading(&self.detail, tr(section.title));
            (section.build)(&self.detail, app, self);
        }
        // The choices start at their first item, so show what the files give.
        self.update();
        self.opened.set(Some(at));
        self.stack.set_visible_child_name("app");
        back.grab_focus();
        eprintln!("edel-settings: app {} shown", app.id);
    }

    /// Goes back to the list, with the focus on the row of the app whose page
    /// was open.
    fn show_list(&self) {
        self.stack.set_visible_child_name("list");
        if let Some(row) = self.opened.take().and_then(|at| self.rows.get(at)) {
            row.button.grab_focus();
        }
    }
}

/// The items of `permission`'s choice in the order people see them: `None`
/// is the choice without an entry, then each value the permission takes.
/// For the notifications and the files the first value is that choice, so
/// it is `None`, and only the others are listed.
fn items(permission: &Permission) -> Vec<Option<&'static str>> {
    let values = permission.choices.iter().map(|choice| Some(*choice));
    if permission.asks_first {
        std::iter::once(None).chain(values).collect()
    } else {
        std::iter::once(None).chain(values.skip(1)).collect()
    }
}

/// The words of one item of `permission`'s choice, as the person reads
/// them; the files' choice is about which files, so its words say so.
fn word(permission: &Permission, item: Option<&'static str>) -> &'static str {
    let files = permission.place == Place::Files;
    match item {
        None if files => tr("All its files"),
        Some(ASK) if files => tr("Only files you pick"),
        None if permission.asks_first => tr("Ask the first time"),
        None | Some(ALLOW) => tr("Allow"),
        Some(ASK) => tr("Ask every time"),
        Some(NEVER) => tr("Never"),
        Some(other) => other,
    }
}

/// The choices an app has that differ from the one that applies without its
/// key, in `PERMISSIONS` order, each with its value as `items` names it
/// (M6.14a). An entry no item of the choice shows is the default, as the page
/// shows it, so it is left out.
fn entries_of(all: &app_permissions::Choices, app: &str) -> Vec<(Permission, &'static str)> {
    PERMISSIONS
        .iter()
        .filter_map(|permission| {
            let entry = all.get(permission.key)?.get(app)?;
            items(permission)
                .into_iter()
                .flatten()
                .find(|item| *item == entry.as_str())
                .map(|item| (*permission, item))
        })
        .collect()
}

/// What an app's list row says it may use (M6.14a): `Not sandboxed` for an
/// app that is not a Flatpak app, `Sandboxed` alone when no choice differs
/// from the default, else `Sandboxed` with up to two of its choices and
/// `and N more` for the rest.
fn summary(app_is_flatpak: bool, choices: &[(Permission, &'static str)]) -> String {
    if !app_is_flatpak {
        return tr("Not sandboxed").to_string();
    }
    if choices.is_empty() {
        return tr("Sandboxed").to_string();
    }
    let parts: Vec<String> = choices
        .iter()
        .take(2)
        .map(|(permission, value)| {
            trf(
                "{title}: {word}",
                &[
                    ("title", tr(permission.title)),
                    ("word", word(permission, Some(*value))),
                ],
            )
        })
        .collect();
    let mut shown = parts.join(", ");
    let more = choices.len().saturating_sub(2);
    if more > 0 {
        shown = trf(
            "{parts} and {n} more",
            &[("parts", &shown), ("n", &more.to_string())],
        );
    }
    trf(
        "{status} · {parts}",
        &[("status", tr("Sandboxed")), ("parts", &shown)],
    )
}

/// Whether an app is listed for search text `query`: its name or its id
/// holds it, whatever the case. An empty query lists every app (M6.14a).
fn matches(app_name: &str, app_id: &str, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    query.is_empty()
        || app_name.to_lowercase().contains(&query)
        || app_id.to_lowercase().contains(&query)
}

/// The text of `key`'s table in the person's file once `app`'s entry is
/// `value` (`None` takes the entry out). Only the person's file is read:
/// the machine's entries stay the machine's. The table is one inline
/// table, as `edel settings set` takes it, or `None` when it is left empty,
/// so the key goes out of the file. A file that is not settings is refused
/// with its message and nothing is written.
fn table_text(
    person: &str,
    key: &str,
    app: &str,
    value: Option<&str>,
) -> Result<Option<String>, String> {
    let mut table = if person.trim().is_empty() {
        BTreeMap::new()
    } else {
        settings::read(person)
            .map_err(|e| format!("{e:#}"))?
            .file
            .apps
            .choices()
            .remove(key)
            .unwrap_or_default()
    };
    if let Some(value) = value {
        table.insert(app.to_string(), value.to_string());
    } else {
        table.remove(app);
    }
    if table.is_empty() {
        return Ok(None);
    }
    let pairs: Vec<String> = table
        .iter()
        .map(|(app, value)| format!("{app:?} = {value:?}"))
        .collect();
    Ok(Some(format!("{{ {} }}", pairs.join(", "))))
}

/// Whether an app is sandboxed: a Flatpak app, by its id.
fn sandboxed(app: &App) -> bool {
    apps::is_flatpak_id(&app.id)
}

fn unsandboxed(app: &App) -> bool {
    !sandboxed(app)
}

/// The icon of `app` at `size` pixels: its own, found in the icon themes
/// its package installs into, else our generic one.
fn app_icon(app: &App, size: i32) -> gtk::Image {
    let found = app
        .icon
        .as_deref()
        .and_then(|name| app_icons::find(&apps::data_dirs(), name, size as u32));
    match found {
        Some(path) => {
            let image = gtk::Image::from_file(path);
            image.set_pixel_size(size);
            image
        }
        None => icon::image("app-generic", size),
    }
}

/// The settings files this page follows: the machine's and the person's.
fn monitors() -> Vec<gio::FileMonitor> {
    let files = Files::here();
    std::iter::once(&files.machine)
        .chain(files.person.as_ref())
        .filter_map(|path| {
            gio::File::for_path(path)
                .monitor_file(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
                .ok()
        })
        .collect()
}

/// Every app on this computer, once each by id, sorted by name whatever the
/// case (`read_all` already keeps the first file of an id).
fn listed() -> Vec<App> {
    let mut listed = apps::read_all(&apps::dirs());
    let mut seen = BTreeSet::new();
    listed.retain(|app| seen.insert(app.id.clone()));
    listed.sort_by_key(|app| app.name.to_lowercase());
    listed
}

/// One app's row in the list: its icon, its name and what it may use, and a
/// chevron that says it opens (M6.14a).
fn list_row(group: &gtk::Box, app: &App) -> ListRow {
    let button = gtk::Button::builder()
        .css_classes(["edel-row", "edel-app-row", "flat"])
        .build();
    let row = gtk::Box::builder().spacing(12).build();
    let picture = app_icon(app, LIST_ICON);
    picture.set_valign(gtk::Align::Center);
    let words = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build();
    words.append(
        &gtk::Label::builder()
            .label(&app.name)
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .css_classes(["edel-row-title"])
            .build(),
    );
    let summary = gtk::Label::builder()
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .css_classes(["edel-row-subtitle"])
        .build();
    words.append(&summary);
    row.append(&picture);
    row.append(&words);
    row.append(&icon::image("chevron-right", 16));
    button.set_child(Some(&row));
    group.append(&button);
    ListRow { button, summary }
}

/// The page of an app: its heading, the icon and name, and its id with
/// whether it is sandboxed.
fn header(app: &App) -> gtk::Box {
    let head = gtk::Box::builder().spacing(16).margin_top(12).build();
    let picture = app_icon(app, PAGE_ICON);
    picture.set_valign(gtk::Align::Center);
    let words = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build();
    words.append(
        &gtk::Label::builder()
            .label(&app.name)
            .xalign(0.0)
            .wrap(true)
            .css_classes(["edel-page-title"])
            .build(),
    );
    let status = if sandboxed(app) {
        tr("Sandboxed")
    } else {
        tr("Not sandboxed")
    };
    words.append(
        &gtk::Label::builder()
            .label(trf(
                "{id} · {status}",
                &[("id", &app.id), ("status", status)],
            ))
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .css_classes(["edel-row-subtitle"])
            .build(),
    );
    head.append(&picture);
    head.append(&words);
    head
}

/// The button at the top of an app's page that goes back to the list.
fn back_button(ui: &Rc<Ui>) -> gtk::Button {
    let back = gtk::Button::builder()
        .halign(gtk::Align::Start)
        .css_classes(["edel-back"])
        .build();
    let words = gtk::Box::builder().spacing(4).build();
    words.append(&icon::image("back", 14));
    words.append(&gtk::Label::new(Some(tr("Apps"))));
    back.set_child(Some(&words));
    back.update_property(&[gtk::accessible::Property::Label(tr("Back to the apps"))]);
    let weak = Rc::downgrade(ui);
    back.connect_clicked(move |_| {
        if let Some(ui) = weak.upgrade() {
            ui.show_list();
        }
    });
    back
}

/// What a Flatpak app may use: a choice per permission, each written to the
/// person's settings file (M6.14a), with the permission's own icon (M6.14a).
fn permissions(content: &gtk::Box, app: &App, ui: &Rc<Ui>) {
    let group = widgets::group(content);
    for permission in PERMISSIONS {
        let items = items(permission);
        let labels: Vec<String> = items
            .iter()
            .map(|item| word(permission, *item).to_string())
            .collect();
        let choice = widgets::Choice::new(labels);
        let row = widgets::row(&group, tr(permission.title), &choice.widget());
        row.set_icon(permission.icon);
        // A screen reader hears the app too, as every app has a Camera row.
        choice
            .widget()
            .update_property(&[gtk::accessible::Property::Label(&trf(
                "{what} for {app}",
                &[("what", tr(permission.title)), ("app", &app.name)],
            ))]);
        // Reset and Copy as command work on a key's whole table, which
        // holds every app's entry, so each choice is changed here instead.
        row.reset.set_visible(false);
        row.copy.set_visible(false);
        let index = ui.slots.borrow().len();
        ui.slots.borrow_mut().push(Slot {
            key: permission.key,
            app: app.id.clone(),
            items,
            choice: Rc::clone(&choice),
        });
        let weak = Rc::downgrade(ui);
        choice.connect_changed(move || {
            if let Some(ui) = weak.upgrade() {
                ui.choose(index);
            }
        });
    }
}

/// What an app that is not a Flatpak app may use: everything, so there is
/// nothing to allow or refuse here (M6.14a).
fn not_sandboxed(content: &gtk::Box, _app: &App, _ui: &Rc<Ui>) {
    let group = widgets::group(content);
    widgets::text_row(
        &group,
        tr(
            "This app is not sandboxed: it can use everything you can, so there is nothing to allow or refuse here.",
        ),
    );
}

pub fn page() -> gtk::Widget {
    let title = PAGES
        .iter()
        .find(|p| p.section == "apps")
        .map_or("", |p| p.title);
    // The intro belongs to the list, so an app's page does not repeat it.
    let (page, content, problem) = widgets::page(tr(title), "");
    let apps = listed();
    let list = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    list.append(
        &gtk::Label::builder()
            .label(tr(
                "Every app on this computer and what each may use. Open an app to change it.",
            ))
            .xalign(0.0)
            .wrap(true)
            .css_classes(["edel-intro"])
            .build(),
    );
    let search = gtk::SearchEntry::builder()
        .placeholder_text(tr("Find an app"))
        .margin_top(8)
        .css_classes(["edel-entry"])
        .build();
    search.update_property(&[gtk::accessible::Property::Label(tr("Find an app"))]);
    list.append(&search);
    let count = match apps.len() {
        1 => tr("1 app").to_string(),
        n => trf("{count} apps", &[("count", &n.to_string())]),
    };
    widgets::heading(&list, &count);
    let group = widgets::group(&list);
    let rows: Vec<ListRow> = apps.iter().map(|app| list_row(&group, app)).collect();
    let nothing = gtk::Label::builder()
        .label(tr("No app matches"))
        .xalign(0.0)
        .wrap(true)
        .visible(false)
        .css_classes(["edel-row", "edel-row-subtitle"])
        .build();
    group.append(&nothing);
    let detail = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    let stack = gtk::Stack::builder()
        .transition_type(gtk::StackTransitionType::SlideLeftRight)
        .transition_duration(150)
        .hhomogeneous(false)
        .vhomogeneous(false)
        .build();
    stack.add_named(&list, Some("list"));
    stack.add_named(&detail, Some("app"));
    content.append(&stack);
    let listed_count = apps.len();
    let ui = Rc::new(Ui {
        apps,
        rows,
        stack,
        detail,
        nothing,
        slots: RefCell::new(Vec::new()),
        opened: Cell::new(None),
        problem,
        monitors: monitors(),
    });
    ui.update();
    eprintln!("edel-settings: apps list shows {listed_count} apps");
    for monitor in &ui.monitors {
        let weak = Rc::downgrade(&ui);
        monitor.connect_changed(move |_, _, _, _| {
            if let Some(ui) = weak.upgrade() {
                ui.update();
            }
        });
    }
    for (at, row) in ui.rows.iter().enumerate() {
        let weak = Rc::downgrade(&ui);
        row.button.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.open(at);
            }
        });
    }
    {
        let weak = Rc::downgrade(&ui);
        search.connect_search_changed(move |search| {
            if let Some(ui) = weak.upgrade() {
                ui.filter(&search.text());
            }
        });
    }
    {
        let weak = Rc::downgrade(&ui);
        search.connect_activate(move |_| {
            // Return opens the first app the search leaves showing.
            if let Some(ui) = weak.upgrade() {
                if let Some(at) = ui.rows.iter().position(|row| row.button.is_visible()) {
                    ui.open(at);
                }
            }
        });
    }
    {
        // Escape goes back from an app's page to the list, as the back button does.
        let weak = Rc::downgrade(&ui);
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed(move |_, key, _, _| {
            let app_shown = weak
                .upgrade()
                .filter(|ui| ui.stack.visible_child_name().as_deref() == Some("app"));
            match (key == gdk::Key::Escape, app_shown) {
                (true, Some(ui)) => {
                    ui.show_list();
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
        page.add_controller(keys);
    }
    // The page's state lives as long as the page does.
    let keep = ui.clone();
    page.connect_destroy(move |_| {
        let _ = &keep;
    });
    let asked = ASKED_APP.with(|asked| asked.borrow_mut().take());
    if let Some(id) = asked {
        match ui.apps.iter().position(|app| app.id == id) {
            Some(at) => ui.open(at),
            None => eprintln!("edel-settings: no app {id} on this computer"),
        }
    }
    page
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rows;

    fn permission(key: &str) -> &'static Permission {
        app_permissions::find(key).expect("a permission of the page")
    }

    /// An app as `apps::parse` reads its desktop file, with the id `read_all`
    /// would give it.
    fn desktop_app(id: &str) -> App {
        let mut app =
            apps::parse("[Desktop Entry]\nType=Application\nName=Example\nExec=example\n")
                .expect("a desktop entry");
        app.id = id.to_string();
        app
    }

    #[test]
    fn the_choice_without_an_entry_is_the_first_item() {
        assert_eq!(
            items(permission("apps.camera")),
            [None, Some(ALLOW), Some(ASK), Some(NEVER)]
        );
        assert_eq!(
            items(permission("apps.screenshots")),
            [None, Some(ALLOW), Some(NEVER)]
        );
        assert_eq!(items(permission("apps.notifications")), [None, Some(NEVER)]);
        assert_eq!(items(permission("apps.files")), [None, Some(ASK)]);
    }

    #[test]
    fn the_words_say_what_each_choice_does() {
        assert_eq!(word(permission("apps.camera"), None), "Ask the first time");
        assert_eq!(word(permission("apps.camera"), Some(ALLOW)), "Allow");
        assert_eq!(word(permission("apps.camera"), Some(ASK)), "Ask every time");
        assert_eq!(word(permission("apps.screenshots"), Some(NEVER)), "Never");
        assert_eq!(word(permission("apps.notifications"), None), "Allow");
        assert_eq!(word(permission("apps.files"), None), "All its files");
        assert_eq!(
            word(permission("apps.files"), Some(ASK)),
            "Only files you pick"
        );
    }

    #[test]
    fn an_entry_is_added_and_the_other_apps_are_kept() {
        let first = table_text("", "apps.camera", "org.a.A", Some(NEVER)).unwrap();
        assert_eq!(first.as_deref(), Some("{ \"org.a.A\" = \"never\" }"));
        let file = "format = 1\n[apps]\ncamera = { \"org.a.A\" = \"never\" }\n";
        let both = table_text(file, "apps.camera", "org.b.B", Some(ALLOW)).unwrap();
        assert_eq!(
            both.as_deref(),
            Some("{ \"org.a.A\" = \"never\", \"org.b.B\" = \"allow\" }")
        );
    }

    #[test]
    fn taking_the_last_entry_out_takes_the_key_out() {
        let file = "format = 1\n[apps]\ncamera = { \"org.a.A\" = \"never\" }\n";
        assert_eq!(table_text(file, "apps.camera", "org.a.A", None), Ok(None));
        let kept = table_text(
            "format = 1\n[apps]\ncamera = { \"org.a.A\" = \"never\", \"org.b.B\" = \"ask\" }\n",
            "apps.camera",
            "org.a.A",
            None,
        );
        assert_eq!(kept.unwrap().as_deref(), Some("{ \"org.b.B\" = \"ask\" }"));
    }

    #[test]
    fn a_table_is_one_the_settings_writer_accepts() {
        for (key, value) in [
            ("apps.camera", ALLOW),
            ("apps.screenshots", NEVER),
            ("apps.notifications", NEVER),
            ("apps.files", ASK),
        ] {
            let text = table_text("", key, "org.example.App", Some(value))
                .unwrap()
                .unwrap();
            assert!(
                settings::set("format = 1\n", key, &text).is_ok(),
                "{key} = {text}"
            );
        }
    }

    #[test]
    fn a_file_that_is_not_settings_is_refused_not_overwritten() {
        assert!(table_text("not toml [", "apps.camera", "org.a.A", Some(NEVER)).is_err());
    }

    #[test]
    fn each_permission_has_a_row_titled_as_the_page_says() {
        for permission in PERMISSIONS {
            assert_eq!(rows::title(permission.key), tr(permission.title));
        }
        assert_eq!(rows::title("apps.files"), "Files");
    }

    #[test]
    fn a_list_row_says_what_an_app_may_use() {
        assert_eq!(summary(false, &[]), "Not sandboxed");
        assert_eq!(summary(true, &[]), "Sandboxed");
        assert_eq!(
            summary(true, &[(*permission("apps.camera"), NEVER)]),
            "Sandboxed · Camera: Never"
        );
        let two = [
            (*permission("apps.camera"), NEVER),
            (*permission("apps.files"), ASK),
        ];
        assert_eq!(
            summary(true, &two),
            "Sandboxed · Camera: Never, Files: Only files you pick"
        );
        let three = [
            (*permission("apps.camera"), NEVER),
            (*permission("apps.screenshots"), NEVER),
            (*permission("apps.files"), ASK),
        ];
        assert_eq!(
            summary(true, &three),
            "Sandboxed · Camera: Never, Screenshots: Never and 1 more"
        );
    }

    #[test]
    fn the_summary_takes_the_choices_the_files_give_each_app() {
        let file = "format = 1\n[apps]\ncamera = { \"org.a.A\" = \"never\" }\nfiles = { \"org.a.A\" = \"allow\", \"org.b.B\" = \"ask\" }\n";
        let all = app_permissions::choices_between(None, Some(file));
        // Files' allow is the choice without an entry, so it is not a choice to name.
        assert_eq!(
            entries_of(&all, "org.a.A"),
            [(*permission("apps.camera"), NEVER)]
        );
        assert_eq!(
            entries_of(&all, "org.b.B"),
            [(*permission("apps.files"), ASK)]
        );
        assert_eq!(entries_of(&all, "org.c.C"), []);
    }

    #[test]
    fn search_finds_an_app_by_its_name_or_its_id() {
        assert!(matches("Firefox", "org.mozilla.firefox", ""));
        assert!(matches("Firefox", "org.mozilla.firefox", "   "));
        assert!(matches("Firefox", "org.mozilla.firefox", "FIRE"));
        assert!(matches("Snapshot", "org.gnome.Snapshot", "gnome"));
        assert!(!matches("Firefox", "org.mozilla.firefox", "chrome"));
    }

    #[test]
    fn an_app_page_shows_one_part_for_its_kind() {
        let shown = |app: &App| SECTIONS.iter().filter(|s| (s.shown)(app)).count();
        let flatpak = desktop_app("io.mpv.Mpv");
        let plain = desktop_app("example");
        assert!(sandboxed(&flatpak));
        assert!(!sandboxed(&plain));
        assert_eq!(shown(&flatpak), 1);
        assert_eq!(shown(&plain), 1);
    }
}
