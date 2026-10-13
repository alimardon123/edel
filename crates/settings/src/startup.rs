//! The Startup page (roadmap M6.10): the apps that start with the desktop,
//! `startup.apps`, each a desktop file id the desktop starts once it is on
//! screen, in the order listed. Each listed app has a Remove button, and
//! Add an app offers the installed apps not in the list yet. The whole list
//! is written each time, through `Files::choose_startup`, so the file equals
//! what `edel settings set` writes. The page follows the files while it is
//! open, as the Default apps page does.

use std::rc::Rc;

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

use edel::apps::{self, App};
use edel::i18n::{tr, trf};
use edel::settings::{PAGES, Source};

use crate::files::{self, Files};
use crate::{rows, widgets};

/// The key this page changes.
const KEY: &str = "startup.apps";

struct Ui {
    /// The installed apps, read once when the page opens.
    apps: Vec<App>,
    source: widgets::Source,
    /// The box the app rows and the Add an app row are rebuilt in.
    group: gtk::Box,
    problem: gtk::Label,
    /// Kept so the page follows the files while it is open.
    monitors: Vec<gio::FileMonitor>,
}

/// The installed apps that the list does not start yet, by name.
fn offered<'a>(apps: &'a [App], list: &[String]) -> Vec<&'a App> {
    let mut out: Vec<&App> = apps.iter().filter(|app| !list.contains(&app.id)).collect();
    out.sort_by_key(|app| app.name.to_lowercase());
    out
}

/// Shows the list that applies: a row for each app, Add an app, and where
/// the list comes from. Rebuilt from the files each time.
fn refresh(ui: &Rc<Ui>) {
    let files = Files::here();
    let list = files.startup();
    let source = files.source(KEY);
    ui.source
        .label
        .set_label(rows::note(&source, false).unwrap_or(""));
    ui.source.reset.set_visible(rows::resettable(&source));
    while let Some(child) = ui.group.first_child() {
        ui.group.remove(&child);
    }
    for id in &list {
        let row = app_row(ui, &list, id);
        ui.group.append(&row);
    }
    let add = add_row(ui, &list);
    ui.group.append(&add);
}

/// The row of one listed app: its name, or its id with a note when no
/// installed app has it, and Remove, which writes the list without it.
fn app_row(ui: &Rc<Ui>, list: &[String], id: &str) -> gtk::Box {
    let row = gtk::Box::builder()
        .spacing(10)
        .css_classes(["edel-row"])
        .build();
    let name = match ui.apps.iter().find(|app| app.id == id) {
        Some(app) => app.name.clone(),
        None => trf("{id} (not installed)", &[("id", id)]),
    };
    let title = gtk::Label::builder()
        .label(&name)
        .xalign(0.0)
        .hexpand(true)
        .wrap(true)
        .css_classes(["edel-row-title"])
        .build();
    let remove = widgets::action(tr("Remove"), false);
    remove.update_property(&[gtk::accessible::Property::Label(&trf(
        "Remove {app}",
        &[("app", &name)],
    ))]);
    let rest: Vec<String> = list.iter().filter(|x| x.as_str() != id).cloned().collect();
    let weak = Rc::downgrade(ui);
    remove.connect_clicked(move |_| {
        if let Some(ui) = weak.upgrade() {
            write(&ui, rest.clone());
        }
    });
    row.append(&title);
    row.append(&remove);
    row
}

/// The Add an app row: a choice of the installed apps not listed, and Add,
/// which writes the list with the chosen one at its end.
fn add_row(ui: &Rc<Ui>, list: &[String]) -> gtk::Box {
    let row = gtk::Box::builder()
        .spacing(10)
        .css_classes(["edel-row"])
        .build();
    let title = gtk::Label::builder()
        .label(tr("Add an app"))
        .xalign(0.0)
        .hexpand(true)
        .wrap(true)
        .css_classes(["edel-row-title"])
        .build();
    let candidates = offered(&ui.apps, list);
    let labels: Vec<String> = candidates.iter().map(|app| app.name.clone()).collect();
    let ids: Vec<String> = candidates.iter().map(|app| app.id.clone()).collect();
    let choice = widgets::Choice::new(labels);
    let choice_widget = choice.widget();
    let add = widgets::action(tr("Add"), true);
    add.set_sensitive(!ids.is_empty());
    let weak = Rc::downgrade(ui);
    let list = list.to_vec();
    add.connect_clicked(move |_| {
        let Some(ui) = weak.upgrade() else { return };
        let Some(id) = ids.get(choice.selected()) else {
            return;
        };
        let mut next = list.clone();
        next.push(id.clone());
        write(&ui, next);
    });
    row.append(&title);
    row.append(&choice_widget);
    row.append(&add);
    row
}

/// Writes the whole list (M6.10), and says so for CI: `edel-settings:
/// startup apps WRITTEN`, or `taken out` when the file no longer holds the
/// key, as the list it would give is the machine's or none.
fn write(ui: &Rc<Ui>, list: Vec<String>) {
    let result = Files::here().choose_startup(&list);
    if result.is_ok() {
        let written = matches!(Files::here().source(KEY), Source::Person(_));
        if written {
            eprintln!("edel-settings: startup apps WRITTEN");
        } else {
            eprintln!("edel-settings: startup apps taken out");
        }
    }
    report(ui, result);
    later(ui);
}

/// Takes the key out of the person's file (Reset).
fn reset(ui: &Rc<Ui>) {
    let result = Files::here().set(KEY, None);
    if result.is_ok() {
        eprintln!("edel-settings: startup apps taken out");
    }
    report(ui, result);
    later(ui);
}

/// Shows why the list was not written, or takes an old message away.
fn report(ui: &Ui, result: Result<(), String>) {
    match result {
        Ok(()) => ui.problem.set_visible(false),
        Err(e) => {
            ui.problem.set_label(&e);
            ui.problem.set_visible(true);
        }
    }
}

/// Shows the list again once the callback that asked for it has returned:
/// the rows it replaces are the ones answering the click.
fn later(ui: &Rc<Ui>) {
    let weak = Rc::downgrade(ui);
    glib::idle_add_local_once(move || {
        if let Some(ui) = weak.upgrade() {
            refresh(&ui);
        }
    });
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
    let words = PAGES.iter().find(|p| p.section == "startup");
    let (title, about) = words.map_or(("", ""), |p| (p.title, p.about));
    let (page, content, problem) = widgets::page(tr(title), tr(about));
    let source = widgets::section(&content, rows::title(KEY));
    let group = widgets::group(&content);
    let ui = Rc::new(Ui {
        apps: apps::read_all(&apps::dirs()),
        source,
        group,
        problem,
        monitors: monitors(),
    });
    refresh(&ui);
    for monitor in &ui.monitors {
        let weak = Rc::downgrade(&ui);
        monitor.connect_changed(move |_, _, _, _| {
            if let Some(ui) = weak.upgrade() {
                refresh(&ui);
            }
        });
    }
    let weak = Rc::downgrade(&ui);
    ui.source.reset.connect_clicked(move |_| {
        if let Some(ui) = weak.upgrade() {
            reset(&ui);
        }
    });
    ui.source.copy.connect_clicked(move |button| {
        // The line that gives the list as it applies now, or the reset when
        // none is set.
        let list = Files::here().startup();
        let line = if list.is_empty() {
            format!("edel settings reset {KEY}")
        } else {
            rows::command(&[(KEY, files::list_text(&list))])
        };
        button.clipboard().set_text(&line);
        widgets::copied(button);
    });
    // The page's state lives as long as the page does: its callbacks hold
    // only weak references.
    let keep = ui.clone();
    page.connect_destroy(move |_| {
        let _ = &keep;
    });
    page
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_add_choice_offers_the_installed_apps_not_listed_by_name() {
        let dir = std::env::temp_dir().join(format!("edel-startup-offers-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for id in ["org.example.Zed", "org.example.Ant", "org.example.Mid"] {
            std::fs::write(
                dir.join(format!("{id}.desktop")),
                format!("[Desktop Entry]\nType=Application\nName={id}\nExec={id}\n"),
            )
            .unwrap();
        }
        let apps = apps::read_all(std::slice::from_ref(&dir));
        let _ = std::fs::remove_dir_all(&dir);
        let list = ["org.example.Mid".to_string()];
        let ids: Vec<&str> = offered(&apps, &list)
            .iter()
            .map(|a| a.id.as_str())
            .collect();
        assert_eq!(
            ids,
            ["org.example.Ant", "org.example.Zed"],
            "by name, the listed one left out"
        );
        assert_eq!(offered(&apps, &[]).len(), 3);
    }
}
