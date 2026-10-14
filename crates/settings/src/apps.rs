//! The Apps page (roadmap M6.14a): what each app may use. Each installed
//! Flatpak app has a row for each of the four permissions in
//! `edel::app_permissions` (Camera, Screenshots, Notifications and Files),
//! and each choice is one app's entry in that key's table of the person's
//! settings file, such as `camera = { "org.gnome.Snapshot" = "never" }`.
//! The choice without an entry is the one that applies without the key, so
//! its entry is taken out rather than written (ADR-008). Apps that are not
//! Flatpak apps are listed below as not sandboxed, with no choices. The
//! page follows the files while it is open, as Startup does; the rows are
//! built once, from the apps installed when the page opens, and their
//! choices are shown again whenever a file changes. shell-ui lays the
//! choices onto Flatpak's own state.

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use gtk::gio;
use gtk::prelude::*;

use edel::app_icons;
use edel::app_permissions::{self, ALLOW, ASK, NEVER, PERMISSIONS, Permission, Place};
use edel::apps::{self, App};
use edel::i18n::{tr, trf};
use edel::settings::{self, PAGES};

use crate::files::Files;
use crate::{icon, widgets};

/// The side of an app's icon, in logical pixels.
const ICON: i32 = 24;

/// One choice on the page: one app's one permission.
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

struct Ui {
    slots: Vec<Slot>,
    problem: gtk::Label,
    /// Kept so the page follows the files while it is open.
    monitors: Vec<gio::FileMonitor>,
}

impl Ui {
    /// Shows the choices the files give now. `Choice::set_selected` only
    /// shows a value and never calls back, so this writes nothing.
    fn update(&self) {
        let files = Files::here();
        let (machine, person) = files.texts();
        let all = app_permissions::choices_between(machine.as_deref(), Some(&person));
        for slot in &self.slots {
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
        let slot = &self.slots[index];
        let files = Files::here();
        let value = slot.items.get(slot.choice.selected()).copied().flatten();
        let result = table_text(&files.person_text(), slot.key, &slot.app, value)
            .and_then(|text| files.set(slot.key, text.as_deref()));
        // The CI line says what was written for an app, as the other pages do.
        if result.is_ok() {
            let done = if value.is_some() {
                "written"
            } else {
                "taken out"
            };
            eprintln!("edel-settings: apps {} {} {done}", slot.key, slot.app);
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

/// The icon of `app` at `ICON` pixels: its own, found in the icon themes
/// its package installs into, else our generic one.
fn app_icon(app: &App) -> gtk::Image {
    let found = app
        .icon
        .as_deref()
        .and_then(|name| app_icons::find(&apps::data_dirs(), name, ICON as u32));
    match found {
        Some(path) => {
            let image = gtk::Image::from_file(path);
            image.set_pixel_size(ICON);
            image
        }
        None => icon::image("app-generic", ICON),
    }
}

/// The row of an app that is not sandboxed: its icon and name, and the
/// words that say what that means.
fn plain_row(group: &gtk::Box, app: &App) {
    let row = gtk::Box::builder()
        .spacing(10)
        .css_classes(["edel-row"])
        .build();
    let picture = app_icon(app);
    picture.set_valign(gtk::Align::Center);
    let words = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build();
    let name = gtk::Label::builder()
        .label(&app.name)
        .xalign(0.0)
        .wrap(true)
        .css_classes(["edel-row-title"])
        .build();
    let note = gtk::Label::builder()
        .label(tr("Not sandboxed: it can use everything you can"))
        .xalign(0.0)
        .wrap(true)
        .css_classes(["edel-row-subtitle"])
        .build();
    words.append(&name);
    words.append(&note);
    row.append(&picture);
    row.append(&words);
    group.append(&row);
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

pub fn page() -> gtk::Widget {
    let title = PAGES
        .iter()
        .find(|p| p.section == "apps")
        .map_or("", |p| p.title);
    let (page, content, problem) = widgets::page(
        tr(title),
        tr(
            "What each app may use. An app not changed here asks the first time and keeps your answer.",
        ),
    );
    // Each app once, by name (`read_all` already keeps the first file of an id).
    let mut listed = apps::read_all(&apps::dirs());
    let mut seen = BTreeSet::new();
    listed.retain(|app| seen.insert(app.id.clone()));
    listed.sort_by_key(|app| app.name.to_lowercase());
    let (flatpak, other): (Vec<App>, Vec<App>) = listed
        .into_iter()
        .partition(|app| apps::is_flatpak_id(&app.id));
    let mut slots = Vec::new();
    for app in &flatpak {
        let heading = widgets::heading(&content, &app.name);
        heading.prepend(&app_icon(app));
        let group = widgets::group(&content);
        for permission in PERMISSIONS {
            let items = items(permission);
            let labels: Vec<String> = items
                .iter()
                .map(|item| word(permission, *item).to_string())
                .collect();
            let choice = widgets::Choice::new(labels);
            let row = widgets::row(&group, tr(permission.title), &choice.widget());
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
            slots.push(Slot {
                key: permission.key,
                app: app.id.clone(),
                items,
                choice,
            });
        }
    }
    if !other.is_empty() {
        widgets::heading(&content, tr("Other apps"));
        let group = widgets::group(&content);
        for app in &other {
            plain_row(&group, app);
        }
    }
    let ui = Rc::new(Ui {
        slots,
        problem,
        monitors: monitors(),
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
    for index in 0..ui.slots.len() {
        let weak = Rc::downgrade(&ui);
        ui.slots[index].choice.connect_changed(move || {
            if let Some(ui) = weak.upgrade() {
                ui.choose(index);
            }
        });
    }
    // The page's state lives as long as the page does.
    let keep = ui.clone();
    page.connect_destroy(move |_| {
        let _ = &keep;
    });
    page
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rows;

    fn permission(key: &str) -> &'static Permission {
        app_permissions::find(key).expect("a permission of the page")
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
}
