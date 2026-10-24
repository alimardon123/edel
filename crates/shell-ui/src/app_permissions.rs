//! What each app may use (M6.14a): the person's `apps.camera`,
//! `apps.screenshots`, `apps.notifications` and `apps.files` choices, laid
//! onto the portals' permission store over the session bus and onto the
//! person's Flatpak override files. shell-ui does it at start and on each
//! settings change. Only what changed since the last pass is written, in a
//! thread of its own, so the panel never waits on the bus. The rules, which
//! entries and lines a choice makes, are `edel::app_permissions`.

use std::io::ErrorKind;
use std::path::Path;

use edel::app_permissions::{
    Entry, STORE_BUS, STORE_INTERFACE, STORE_PATH, asked_every_time, dropped, entries, files_asked,
    override_text,
};
use edel::places;

use crate::messages;

/// What shell-ui last laid on, so a change sets only what differs and
/// deletes only entries it set.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Applied {
    entries: Vec<Entry>,
    files: Vec<String>,
}

/// What one pass of [`follow`] does to the store and the files.
#[derive(Debug, Default, PartialEq)]
struct Plan {
    /// Store entries to set, or to set again with a new value.
    set: Vec<Entry>,
    /// Store entries to delete, which brings the portal's own asking back.
    delete: Vec<Entry>,
    /// Override files to write: each app, and whether it asks (true) or
    /// is allowed again (false).
    files: Vec<(String, bool)>,
}

/// What must change to lay `now` on what `last` holds. At start `last` is
/// None, so every entry is set and every asked file written. Pure, so the
/// rules are tested without a bus or a home folder.
fn plan(last: Option<&Applied>, now: &Applied) -> Plan {
    let before_entries: &[Entry] = match last {
        Some(last) => &last.entries,
        None => &[],
    };
    let before_files: &[String] = match last {
        Some(last) => &last.files,
        None => &[],
    };
    let set = now
        .entries
        .iter()
        .filter(|e| !before_entries.contains(e))
        .cloned()
        .collect();
    let delete = dropped(before_entries, &now.entries);
    let mut files: Vec<(String, bool)> = now
        .files
        .iter()
        .filter(|app| !before_files.contains(app))
        .map(|app| (app.clone(), true))
        .collect();
    files.extend(
        before_files
            .iter()
            .filter(|app| !now.files.contains(app))
            .map(|app| (app.clone(), false)),
    );
    Plan { set, delete, files }
}

/// Lays the person's app permissions on the portals' store and the Flatpak
/// overrides, at start and whenever the settings files change (M6.14a).
/// `last` is what was laid on before, and nothing is done when the choices
/// are the same. With no `connection` the store is skipped and the files
/// are still written. Returns the apps set to ask for the place
/// (`apps.location`, M6.14c), which `location.rs` asks every time.
pub fn follow(
    connection: Option<&zbus::blocking::Connection>,
    last: &mut Option<Applied>,
    machine: Option<&str>,
    person: Option<&str>,
) -> Vec<String> {
    let choices = edel::app_permissions::choices_between(machine, person);
    let asked = asked_every_time(&choices, "apps.location");
    let now = Applied {
        entries: entries(&choices),
        files: files_asked(&choices),
    };
    if last.as_ref() == Some(&now) {
        return asked;
    }
    let plan = plan(last.as_ref(), &now);
    *last = Some(now);
    if plan.set.is_empty() && plan.delete.is_empty() && plan.files.is_empty() {
        return asked;
    }
    let connection = connection.cloned();
    // The bus answers when the portal does, so the work runs off the panel's
    // thread.
    std::thread::spawn(move || {
        if let Some(connection) = &connection {
            store(connection, &plan.set, &plan.delete);
        }
        write_files(&plan.files);
    });
    asked
}

/// Sets and deletes the store's entries over the bus. One failure never
/// stops the others.
fn store(connection: &zbus::blocking::Connection, set: &[Entry], delete: &[Entry]) {
    for entry in set {
        let told = connection.call_method(
            Some(STORE_BUS),
            STORE_PATH,
            Some(STORE_INTERFACE),
            "SetPermission",
            &(
                entry.table,
                true,
                entry.id,
                entry.app.as_str(),
                entry.values.to_vec(),
            ),
        );
        match told {
            Ok(_) => eprintln!(
                "edel-shell-ui: app permission {} {} for {}",
                entry.id,
                entry.values.join(" "),
                entry.app
            ),
            Err(e) => eprintln!(
                "edel-shell-ui: {}",
                messages::app_permission_not_set(&entry.app, entry.id, e)
            ),
        }
    }
    for entry in delete {
        let gone = connection.call_method(
            Some(STORE_BUS),
            STORE_PATH,
            Some(STORE_INTERFACE),
            "DeletePermission",
            &(entry.table, entry.id, entry.app.as_str()),
        );
        match gone {
            Ok(_) => eprintln!(
                "edel-shell-ui: app permission {} for {} back to asking",
                entry.id, entry.app
            ),
            // An entry the portal already lacks is the state we want.
            Err(e) if not_there(&e.to_string()) => {}
            Err(e) => eprintln!(
                "edel-shell-ui: {}",
                messages::app_permission_not_set(&entry.app, entry.id, e)
            ),
        }
    }
}

/// Whether a bus error says the entry is not in the store.
fn not_there(text: &str) -> bool {
    text.contains("NotFound") || text.contains("not found")
}

/// Writes the override files the plan names, in the person's Flatpak
/// overrides folder. Nothing is said when the folder cannot be found.
fn write_files(changes: &[(String, bool)]) {
    let Some(dir) = places::person_flatpak_overrides_dir() else {
        return;
    };
    for (app, ask) in changes {
        match write_override(&dir, app, *ask) {
            Ok(true) => eprintln!(
                "edel-shell-ui: app files {} for {app}",
                if *ask { "ask" } else { "allow" }
            ),
            Ok(false) => {}
            Err(e) => eprintln!("edel-shell-ui: {}", messages::app_files_not_written(app, e)),
        }
    }
}

/// Writes the app's override file in `dir` with the files choice laid on
/// it (`ask` takes the home away, else gives it back). Only what differs is
/// written, through `files::replace`. A file left with nothing in it is
/// removed. Says whether it wrote. An id that is no Flatpak id writes
/// nothing, so no path can leave the folder.
fn write_override(dir: &Path, app: &str, ask: bool) -> std::io::Result<bool> {
    if !edel::apps::is_flatpak_id(app) {
        return Ok(false);
    }
    write_changed(&dir.join(app), |old| override_text(old, ask))
}

/// Lays `change` on the Flatpak override file at `path` (an absent file is
/// empty text) and writes what it gives with `files::replace`; None from
/// `change` writes nothing, and empty text removes the file. Says whether
/// it wrote.
/// `apps_colours.rs` writes the person's global file with it (M6.18).
pub fn write_changed(
    path: &Path,
    change: impl FnOnce(&str) -> Option<String>,
) -> std::io::Result<bool> {
    let old = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e),
    };
    let Some(text) = change(&old) else {
        return Ok(false);
    };
    if text.is_empty() {
        return match std::fs::remove_file(path) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e),
        };
    }
    crate::files::replace(path, text.as_bytes())?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(app: &str, values: &'static [&'static str]) -> Entry {
        Entry {
            table: "devices",
            id: "camera",
            app: app.into(),
            values,
        }
    }

    fn applied(entries: Vec<Entry>, files: &[&str]) -> Applied {
        Applied {
            entries,
            files: files.iter().map(|f| f.to_string()).collect(),
        }
    }

    #[test]
    fn at_start_every_entry_is_set_and_every_asked_file_written() {
        let now = applied(
            vec![entry("org.a.A", &["no"]), entry("org.a.B", &["ask"])],
            &["org.a.C"],
        );
        let p = plan(None, &now);
        assert_eq!(p.set, now.entries);
        assert!(p.delete.is_empty());
        assert_eq!(p.files, [("org.a.C".to_string(), true)]);
    }

    #[test]
    fn a_changed_choice_sets_that_one_entry_and_deletes_none() {
        let before = applied(
            vec![entry("org.a.A", &["yes"]), entry("org.a.B", &["yes"])],
            &[],
        );
        let now = applied(
            vec![entry("org.a.A", &["yes"]), entry("org.a.B", &["no"])],
            &[],
        );
        let p = plan(Some(&before), &now);
        assert_eq!(p.set, [entry("org.a.B", &["no"])]);
        assert!(p.delete.is_empty());
        assert!(p.files.is_empty());
    }

    #[test]
    fn a_removed_app_deletes_its_entry_and_allows_its_files_again() {
        let before = applied(vec![entry("org.a.A", &["no"])], &["org.a.C"]);
        let now = applied(vec![], &[]);
        let p = plan(Some(&before), &now);
        assert!(p.set.is_empty());
        assert_eq!(p.delete, [entry("org.a.A", &["no"])]);
        assert_eq!(p.files, [("org.a.C".to_string(), false)]);
    }

    #[test]
    fn an_override_asks_for_files_then_allows_them_and_removes_the_file() {
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("edel-shell-ui-overrides-{pid}"));
        let _ = std::fs::remove_dir_all(&dir);
        let app = "org.example.App";
        let file = dir.join(app);

        // Asking writes the line that takes the home away, once.
        assert!(write_override(&dir, app, true).unwrap());
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains("filesystems=!host;"));
        assert!(!write_override(&dir, app, true).unwrap());

        // Allowing again takes the file away, as it holds nothing else.
        assert!(write_override(&dir, app, false).unwrap());
        assert!(!file.exists());
        assert!(!write_override(&dir, app, false).unwrap());

        // An id that is no Flatpak id writes nothing outside the folder.
        let outside = std::env::temp_dir().join(format!("edel-shell-ui-escape-{pid}"));
        let escape = format!("../edel-shell-ui-escape-{pid}");
        assert!(!write_override(&dir, &escape, true).unwrap());
        assert!(!outside.exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_global_override_is_written_once() {
        use edel::app_permissions::{GLOBAL_OVERRIDE, GTK_CSS_FILESYSTEM, gtk_css_readable};
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("edel-shell-ui-global-{pid}"));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join(GLOBAL_OVERRIDE);
        let temporary = dir.join(format!("{GLOBAL_OVERRIDE}.new"));

        // An absent file is made with the line, once, and no .new is left.
        assert!(write_changed(&file, gtk_css_readable).unwrap());
        assert!(
            std::fs::read_to_string(&file)
                .unwrap()
                .contains(GTK_CSS_FILESYSTEM)
        );
        assert!(!write_changed(&file, gtk_css_readable).unwrap());
        assert!(!temporary.exists());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
