//! The Default apps page (roadmap M6.10): the app each role opens, one row
//! per `[default_apps]` key (`edel::apps::DEFAULT_APP_KEYS`): the web
//! browser, Files, the text editor, the terminal and Mail. A row's choice
//! offers Automatic, which takes the key out of the person's file, so the
//! first installed app of the role's kind opens, and the installed apps of
//! that kind; choosing one writes its desktop file id, through the writer
//! the Notifications page uses, so the file equals what `edel settings set`
//! writes. The page follows the files while it is open; the apps are read
//! once, when it opens.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

use edel::apps::{self, App};
use edel::i18n::{tr, trf};
use edel::settings::{PAGES, Source};

use crate::files::Files;
use crate::{actions, cmd, rows, widgets};

/// The section of the settings file this page is.
const SECTION: &str = "default_apps";

/// One choice: the desktop file id it writes (None is Automatic) and the
/// words it shows.
type Entry = (Option<String>, String);

/// The row of one role: its key, the slot its choice sits in, and the
/// choice with what it offers now.
struct RoleRow {
    role: &'static str,
    key: &'static str,
    slot: gtk::Box,
    row: widgets::Row,
    shown: RefCell<Option<Shown>>,
}

struct Shown {
    entries: Vec<Entry>,
    choice: Rc<widgets::Choice>,
}

struct Ui {
    /// The installed apps, read once when the page opens.
    apps: Vec<App>,
    rows: Vec<RoleRow>,
    problem: gtk::Label,
    /// Kept so the page follows the files while it is open.
    monitors: Vec<gio::FileMonitor>,
}

/// What a role's choice offers (M6.10): Automatic first, then the installed
/// apps of the role's kind in the order `apps` gives them, then the key's
/// current value when no listed app is it: an installed app of another
/// kind, or one that is not installed at all.
fn entries(apps: &[App], role: &str, current: Option<&str>) -> Vec<Entry> {
    let automatic = match apps::default_app(apps, role, None) {
        Some(app) => trf("Automatic ({current})", &[("current", &app.name)]),
        None => tr("Automatic").to_string(),
    };
    let mut out: Vec<Entry> = vec![(None, automatic)];
    out.extend(
        apps::apps_of_role(apps, role)
            .iter()
            .map(|app| (Some(app.id.clone()), app.name.clone())),
    );
    if let Some(id) = current.filter(|id| !id.is_empty()) {
        if !out.iter().any(|(chosen, _)| chosen.as_deref() == Some(id)) {
            let label = match apps.iter().find(|app| app.id == id) {
                Some(app) => app.name.clone(),
                None => trf("{id} (not installed)", &[("id", id)]),
            };
            out.push((Some(id.to_string()), label));
        }
    }
    out
}

/// The choice that shows `current`: its place in `entries`, Automatic when
/// the key is not set.
fn position(entries: &[Entry], current: Option<&str>) -> usize {
    entries
        .iter()
        .position(|(id, _)| id.as_deref() == current)
        .unwrap_or(0)
}

/// Shows what the files say for row `i`: its choice at the value that
/// applies, and where that value comes from. A choice whose offers changed
/// is built anew after this call returns, since a choice must not be
/// replaced while it is still answering a click.
fn show(ui: &Rc<Ui>, i: usize) {
    let row = &ui.rows[i];
    let files = Files::here();
    let current = files.chosen(row.key).filter(|v| !v.is_empty());
    let entries = entries(&ui.apps, row.role, current.as_deref());
    let at = position(&entries, current.as_deref());
    let same = row.shown.borrow().as_ref().map(|s| s.entries == entries);
    match same {
        Some(true) => {
            if let Some(shown) = row.shown.borrow().as_ref() {
                shown.choice.set_selected(at);
            }
        }
        Some(false) => {
            let weak = Rc::downgrade(ui);
            glib::idle_add_local_once(move || {
                if let Some(ui) = weak.upgrade() {
                    build(&ui, i, entries, at);
                }
            });
        }
        None => build(ui, i, entries, at),
    }
    let source = files.source(row.key);
    row.row
        .subtitle
        .set_label(rows::note(&source, false).unwrap_or(""));
    row.row.reset.set_visible(rows::resettable(&source));
}

/// Builds row `i`'s choice over `entries`, showing `at`, in its slot.
fn build(ui: &Rc<Ui>, i: usize, entries: Vec<Entry>, at: usize) {
    let row = &ui.rows[i];
    let labels: Vec<String> = entries.iter().map(|(_, label)| label.clone()).collect();
    let choice = widgets::Choice::new(labels);
    choice.set_selected(at);
    let weak = Rc::downgrade(ui);
    choice.connect_changed(move || {
        if let Some(ui) = weak.upgrade() {
            choose(&ui, i);
        }
    });
    while let Some(child) = row.slot.first_child() {
        row.slot.remove(&child);
    }
    row.slot.append(&choice.widget());
    *row.shown.borrow_mut() = Some(Shown { entries, choice });
}

/// Writes the choice a person made for row `i`: Automatic takes the key
/// out, an app writes its id unless the machine already names it, then the
/// page shows what the files say.
fn choose(ui: &Rc<Ui>, i: usize) {
    let row = &ui.rows[i];
    let id = {
        let shown = row.shown.borrow();
        let Some(shown) = shown.as_ref() else { return };
        shown.entries[shown.choice.selected()].0.clone()
    };
    let result = match &id {
        Some(id) => Files::here().choose_over(row.key, id, None),
        None => Files::here().set(row.key, None),
    };
    if result.is_ok() {
        // CI reads these lines: a written id is in the person's file, and
        // one the machine already gives is taken out, never written.
        let written = id
            .as_deref()
            .filter(|_| matches!(Files::here().source(row.key), Source::Person(_)));
        match written {
            Some(id) => eprintln!("edel-settings: default app {} {id} written", row.role),
            None => eprintln!("edel-settings: default app {} taken out", row.role),
        }
    }
    ui.report(result);
    update_all(ui);
}

/// Takes row `i`'s key out of the person's file (Reset).
fn reset(ui: &Rc<Ui>, i: usize) {
    let row = &ui.rows[i];
    let result = Files::here().set(row.key, None);
    if result.is_ok() {
        eprintln!("edel-settings: default app {} taken out", row.role);
    }
    ui.report(result);
    update_all(ui);
}

/// The line Copy as command gives row `key`: the value that applies, or
/// the reset when nothing sets it.
fn copy_line(key: &str) -> String {
    match Files::here().chosen(key).filter(|v| !v.is_empty()) {
        Some(value) => rows::command(&[(key, value)]),
        None => format!("{} {key}", cmd::line(actions::RESET.args)),
    }
}

fn update_all(ui: &Rc<Ui>) {
    for i in 0..ui.rows.len() {
        show(ui, i);
    }
}

impl Ui {
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
    let words = PAGES.iter().find(|p| p.section == SECTION);
    let (title, about) = words.map_or(("", ""), |p| (p.title, p.about));
    let (page, content, problem) = widgets::page(tr(title), tr(about));
    let group = widgets::group(&content);
    let mut rows = Vec::new();
    for (role, key) in apps::DEFAULT_APP_KEYS.iter().copied() {
        let slot = gtk::Box::builder().valign(gtk::Align::Center).build();
        let row = widgets::row(&group, rows::title(key), &slot.clone().upcast());
        rows.push(RoleRow {
            role,
            key,
            slot,
            row,
            shown: RefCell::new(None),
        });
    }
    let ui = Rc::new(Ui {
        apps: apps::read_all(&apps::dirs()),
        rows,
        problem,
        monitors: monitors(),
    });
    update_all(&ui);
    log_places(
        &page,
        ui.rows.iter().map(|r| (r.role, r.slot.clone())).collect(),
    );
    for monitor in &ui.monitors {
        let weak = Rc::downgrade(&ui);
        monitor.connect_changed(move |_, _, _, _| {
            if let Some(ui) = weak.upgrade() {
                update_all(&ui);
            }
        });
    }
    for i in 0..ui.rows.len() {
        let weak = Rc::downgrade(&ui);
        ui.rows[i].row.reset.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                reset(&ui, i);
            }
        });
        let key = ui.rows[i].key;
        ui.rows[i].row.copy.connect_clicked(move |button| {
            button.clipboard().set_text(&copy_line(key));
            widgets::copied(button);
        });
    }
    // The page's state lives as long as the page does: its callbacks hold
    // only weak references.
    let keep = ui.clone();
    page.connect_destroy(move |_| {
        let _ = &keep;
    });
    page
}

/// Logs where each role's choice lies in the window, once the page is laid
/// out and two frames agree, for CI to click (logical pixels), as
/// `edel-settings: default apps places browser X+Y+WxH, files ...`.
fn log_places(page: &gtk::Widget, slots: Vec<(&'static str, gtk::Box)>) {
    let last = RefCell::new(String::new());
    page.add_tick_callback(move |page, _| {
        let Some(window) = page.root().and_downcast::<gtk::Window>() else {
            return glib::ControlFlow::Continue;
        };
        if !page.is_mapped() || page.width() == 0 {
            return glib::ControlFlow::Continue;
        }
        let places: Vec<String> = slots
            .iter()
            .map(|(role, slot)| format!("{role} {}", widgets::place(slot.compute_bounds(&window))))
            .collect();
        let line = format!("edel-settings: default apps places {}", places.join(", "));
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

    /// Apps read from desktop files in a folder of the test's own, as the
    /// page reads them: each file's name is the app's id.
    fn installed(name: &str, files: &[(&str, &str)]) -> Vec<App> {
        let dir =
            std::env::temp_dir().join(format!("edel-default-apps-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (id, category) in files {
            std::fs::write(
                dir.join(format!("{id}.desktop")),
                format!("[Desktop Entry]\nType=Application\nName={id}\nExec={id}\nCategories={category};\n"),
            )
            .unwrap();
        }
        let apps = apps::read_all(std::slice::from_ref(&dir));
        let _ = std::fs::remove_dir_all(&dir);
        apps
    }

    fn labels(entries: &[Entry]) -> Vec<&str> {
        entries.iter().map(|(_, label)| label.as_str()).collect()
    }

    #[test]
    fn the_choice_offers_automatic_then_the_apps_of_the_role() {
        let apps = installed(
            "offers",
            &[
                ("org.example.Web", "WebBrowser"),
                ("org.example.Files", "FileManager"),
                ("org.example.Web2", "WebBrowser"),
            ],
        );
        let offered = entries(&apps, "browser", None);
        assert_eq!(
            labels(&offered),
            [
                "Automatic (org.example.Web)",
                "org.example.Web",
                "org.example.Web2"
            ],
            "Automatic names the app it opens, then the browsers in order"
        );
        assert_eq!(offered[0].0, None, "Automatic writes nothing");
        assert_eq!(position(&offered, None), 0);
        assert_eq!(position(&offered, Some("org.example.Web2")), 2);
    }

    #[test]
    fn a_chosen_app_that_is_missing_or_of_another_kind_is_still_shown() {
        let apps = installed("missing", &[("org.example.Files", "FileManager")]);
        let gone = entries(&apps, "browser", Some("org.example.Gone"));
        assert_eq!(
            labels(&gone),
            ["Automatic", "org.example.Gone (not installed)"],
            "no browser is installed, so Automatic says so"
        );
        assert_eq!(position(&gone, Some("org.example.Gone")), 1);
        let other = entries(&apps, "browser", Some("org.example.Files"));
        assert_eq!(
            labels(&other),
            ["Automatic", "org.example.Files"],
            "an installed app of another kind is listed once, by its name"
        );
        let files = entries(&apps, "files", Some("org.example.Files"));
        assert_eq!(
            labels(&files),
            ["Automatic (org.example.Files)", "org.example.Files"],
            "a listed app is not listed twice"
        );
    }
}
