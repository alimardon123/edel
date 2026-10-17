//! What each app may use (M6.14a, M6.14b, M6.14c): the `apps.camera`,
//! `apps.microphone`, `apps.screenshots`, `apps.notifications`,
//! `apps.files` and `apps.location` keys, each a table of Flatpak app ids
//! and a choice, such as `{ "org.gnome.Snapshot" = "never" }`, laid onto
//! Flatpak's own state, so there is no store of ours (ADR-008, level 1 of
//! ADR-007). The first five are entries of the portals' permission store,
//! which shell-ui sets over the session bus (the portals read three of
//! them, WirePlumber the microphone's); the files are a line of the app's
//! override file, which shell-ui writes. Both are per person, as
//! the keys are. A key that is absent leaves Flatpak's own behaviour: the
//! portal asks the first time and keeps the answer. Pure functions, tested
//! on text; shell-ui does the bus calls and the writing.

use std::collections::BTreeMap;

use crate::i18n::n_;

/// The choices: `allow`, `ask` (every time) and `never`.
pub const ALLOW: &str = "allow";
pub const ASK: &str = "ask";
pub const NEVER: &str = "never";

/// The camera portal asks every time when its entry is `ask`. The location
/// portal takes the same three (M6.14c): `ask` leaves no entry, so it asks.
pub const CAMERA_CHOICES: &[&str] = &[ALLOW, ASK, NEVER];
/// The screenshot and notification portals keep an answer once given, so
/// they have no `ask`.
pub const ALLOW_OR_NEVER: &[&str] = &[ALLOW, NEVER];
/// Files: `allow` keeps what the app asked for when it was installed, `ask`
/// takes away its own reach into the home folder, so it sees only the
/// files a person picks in the file chooser, every time.
pub const FILES_CHOICES: &[&str] = &[ALLOW, ASK];

/// One thing an app may use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Permission {
    /// Its settings key, `apps.camera`.
    pub key: &'static str,
    /// Its name as Settings shows it.
    pub title: &'static str,
    /// The choices the key takes.
    pub choices: &'static [&'static str],
    /// Where it lives: the permission store's table and id, or the files.
    pub place: Place,
    /// Its icon, one of the shell's own (`edel::icons`), which Settings
    /// shows beside its name and the panel's sign will show.
    pub icon: &'static str,
    /// Whether an app with no entry is asked the first time it wants this
    /// (the camera's and the screenshot portal's way), rather than allowed.
    pub asks_first: bool,
}

/// Where a permission lives in Flatpak's state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// An entry of the permission store: its table and its id.
    Store(&'static str, &'static str),
    /// The `filesystems` line of the app's override file.
    Files,
}

/// Every permission, in the order Settings shows them.
pub const PERMISSIONS: &[Permission] = &[
    Permission {
        key: "apps.camera",
        icon: "camera",
        title: n_("Camera"),
        choices: CAMERA_CHOICES,
        place: Place::Store("devices", "camera"),
        asks_first: true,
    },
    // The microphone (M6.14b): no portal asks for it, and Flatpak apps with
    // the sound socket reach PipeWire straight, so the choice is kept under
    // the Device portal's old id and WirePlumber hides the microphones from
    // an app that may not use them (`features/sound`'s
    // `edel/microphone-access.lua`).
    Permission {
        key: "apps.microphone",
        icon: "microphone",
        title: n_("Microphone"),
        choices: ALLOW_OR_NEVER,
        place: Place::Store("devices", "microphone"),
        asks_first: false,
    },
    Permission {
        key: "apps.screenshots",
        icon: "screenshot",
        title: n_("Screenshots"),
        choices: ALLOW_OR_NEVER,
        place: Place::Store("screenshot", "screenshot"),
        asks_first: true,
    },
    Permission {
        key: "apps.notifications",
        icon: "bell",
        title: n_("Notifications"),
        choices: ALLOW_OR_NEVER,
        place: Place::Store("notifications", "notification"),
        asks_first: false,
    },
    Permission {
        key: "apps.files",
        icon: "folder",
        title: n_("Files"),
        choices: FILES_CHOICES,
        place: Place::Files,
        asks_first: false,
    },
    // Location (M6.14c): the location portal keeps an accuracy and a time,
    // EXACT or NONE here, and asks the person when an app has no entry, then
    // keeps the answer; shell-ui takes the entry out again for an app set to
    // ask, so it is asked every time.
    Permission {
        key: "apps.location",
        icon: "location",
        title: n_("Location"),
        choices: CAMERA_CHOICES,
        place: Place::Store("location", "location"),
        asks_first: true,
    },
];

/// The permission store's bus name, path and interface (xdg-desktop-portal's
/// `org.freedesktop.impl.portal.PermissionStore`, version 2, which has
/// `GetPermission` and `DeletePermission`).
pub const STORE_BUS: &str = "org.freedesktop.impl.portal.PermissionStore";
pub const STORE_PATH: &str = "/org/freedesktop/impl/portal/PermissionStore";
pub const STORE_INTERFACE: &str = "org.freedesktop.impl.portal.PermissionStore";

/// The permission a settings key names.
pub fn find(key: &str) -> Option<&'static Permission> {
    PERMISSIONS.iter().find(|p| p.key == key)
}

/// What the permission store holds for a choice of `key`: the words of its
/// entry, `yes`, `ask` or `no` for most keys. A location is an accuracy and
/// a time, `EXACT` or `NONE` with `0`, and `ask` leaves no entry at all, so
/// the portal asks (M6.14c). None when there is nothing to set.
pub fn store_values(key: &str, choice: &str) -> Option<&'static [&'static str]> {
    if key == "apps.location" {
        return match choice {
            ALLOW => Some(&["EXACT", "0"]),
            NEVER => Some(&["NONE", "0"]),
            _ => None,
        };
    }
    match choice {
        ALLOW => Some(&["yes"]),
        ASK => Some(&["ask"]),
        NEVER => Some(&["no"]),
        _ => None,
    }
}

/// The choice a permission store entry stands for, the inverse of
/// [`store_values`]: the words `yes`, `ask` and `no`, and a location's
/// accuracy (`EXACT`, `STREET`, `NEIGHBORHOOD`, `CITY` or `COUNTRY`, which
/// is allow) or `NONE` (never). None for anything else.
pub fn choice_of(entry: &[String]) -> Option<&'static str> {
    match entry.first().map(String::as_str) {
        Some("yes" | "EXACT" | "STREET" | "NEIGHBORHOOD" | "CITY" | "COUNTRY") => Some(ALLOW),
        Some("ask") => Some(ASK),
        Some("no" | "NONE") => Some(NEVER),
        _ => None,
    }
}

/// The choices that apply, from the machine's and the person's settings
/// files' texts, read leniently: each permission's table is the machine's
/// with the person's laid over it, app by app, so a fleet's file sets them
/// and a person can still change one.
pub fn choices_between(machine: Option<&str>, person: Option<&str>) -> Choices {
    let read = |text: Option<&str>| {
        text.and_then(|t| crate::settings::read(t).ok())
            .map(|r| r.file.apps.choices())
            .unwrap_or_default()
    };
    let mut all = read(machine);
    for (key, apps) in read(person) {
        all.entry(key).or_default().extend(apps);
    }
    all
}

/// One entry the store should hold: table, id, app and its words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub table: &'static str,
    pub id: &'static str,
    pub app: String,
    pub values: &'static [&'static str],
}

/// The choices a settings file makes, each permission's key with its table
/// of apps; what [`crate::settings::Apps`] holds.
pub type Choices = BTreeMap<&'static str, BTreeMap<String, String>>;

/// The store entries `choices` ask for, in the order of [`PERMISSIONS`],
/// then by app. A choice a permission does not take is left out, as a
/// lenient reader leaves it.
pub fn entries(choices: &Choices) -> Vec<Entry> {
    let mut out = Vec::new();
    for permission in PERMISSIONS {
        let Place::Store(table, id) = permission.place else {
            continue;
        };
        for (app, choice) in choices.get(permission.key).into_iter().flatten() {
            if !permission.choices.contains(&choice.as_str()) {
                continue;
            }
            if let Some(values) = store_values(permission.key, choice) {
                out.push(Entry {
                    table,
                    id,
                    app: app.clone(),
                    values,
                });
            }
        }
    }
    out
}

/// The store entries to delete: those `before` asked for that `now` no
/// longer names, so a reset in a session brings the portal's own asking
/// back. Only entries we set are ever deleted; an answer a person gave the
/// portal stays.
pub fn dropped(before: &[Entry], now: &[Entry]) -> Vec<Entry> {
    before
        .iter()
        .filter(|b| {
            !now.iter()
                .any(|n| n.table == b.table && n.id == b.id && n.app == b.app)
        })
        .cloned()
        .collect()
}

/// The apps whose choice for `key` is `ask`, sorted. shell-ui uses it for
/// `apps.location`, whose `ask` leaves no entry for the portal to keep, so
/// it has to ask every time itself (M6.14c).
pub fn asked_every_time(choices: &Choices, key: &str) -> Vec<String> {
    let mut apps: Vec<String> = choices
        .get(key)
        .into_iter()
        .flatten()
        .filter(|(_, choice)| choice.as_str() == ASK)
        .map(|(app, _)| app.clone())
        .collect();
    apps.sort();
    apps
}

/// The apps whose files `choices` set to `ask`.
pub fn files_asked(choices: &Choices) -> Vec<String> {
    PERMISSIONS
        .iter()
        .filter(|p| p.place == Place::Files)
        .filter_map(|p| choices.get(p.key))
        .flatten()
        .filter(|(_, choice)| choice.as_str() == ASK)
        .map(|(app, _)| app.clone())
        .collect()
}

/// The places an app's own reach into a person's files is taken away
/// from, as `!NAME` in its override's `filesystems` line: the whole
/// system, the home folder and each of the home's own folders. What a
/// person picks in the file chooser still reaches it through the document
/// portal.
pub const NO_FILES: &[&str] = &[
    "!host",
    "!home",
    "!xdg-desktop",
    "!xdg-documents",
    "!xdg-download",
    "!xdg-music",
    "!xdg-pictures",
    "!xdg-public-share",
    "!xdg-templates",
    "!xdg-videos",
];

/// The text of an app's override file with the files choice laid on it:
/// `ask` adds each of [`NO_FILES`] to `[Context]`'s `filesystems` line,
/// anything else takes them out again. Every other line, group and entry
/// stays as it was. None when the text is already right, so nothing is
/// written.
pub fn override_text(text: &str, ask: bool) -> Option<String> {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let mut group = String::new();
    let mut found = None;
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            group = trimmed.to_string();
            continue;
        }
        if group == "[Context]"
            && trimmed
                .split_once('=')
                .is_some_and(|(key, _)| key.trim() == "filesystems")
        {
            found = Some(i);
            break;
        }
    }
    let old: Vec<String> = found
        .and_then(|i| lines[i].split_once('=').map(|(_, v)| v.to_string()))
        .map(|v| {
            v.split(';')
                .map(str::trim)
                .filter(|e| !e.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let mut new: Vec<String> = old
        .iter()
        .filter(|e| !NO_FILES.contains(&e.as_str()))
        .cloned()
        .collect();
    if ask {
        new.extend(NO_FILES.iter().map(|e| e.to_string()));
    }
    if new == old {
        return None;
    }
    let line = format!("filesystems={};", new.join(";"));
    match found {
        Some(i) if new.is_empty() => {
            lines.remove(i);
        }
        Some(i) => lines[i] = line,
        None => match lines.iter().position(|l| l.trim() == "[Context]") {
            Some(i) => lines.insert(i + 1, line),
            None => {
                if lines.last().is_some_and(|l| !l.trim().is_empty()) {
                    lines.push(String::new());
                }
                lines.push("[Context]".to_string());
                lines.push(line);
            }
        },
    }
    // A `[Context]` group left with nothing in it goes too.
    if let Some(i) = lines.iter().position(|l| l.trim() == "[Context]") {
        let empty = lines[i + 1..]
            .iter()
            .take_while(|l| !l.trim().starts_with('['))
            .all(|l| l.trim().is_empty());
        if empty {
            let end = lines[i + 1..]
                .iter()
                .position(|l| l.trim().starts_with('['))
                .map_or(lines.len(), |n| i + 1 + n);
            lines.drain(i..end);
        }
    }
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    let mut out = lines.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choices(key: &'static str, pairs: &[(&str, &str)]) -> Choices {
        let mut all = Choices::new();
        all.insert(
            key,
            pairs
                .iter()
                .map(|(a, c)| (a.to_string(), c.to_string()))
                .collect(),
        );
        all
    }

    #[test]
    fn each_permission_has_a_key_of_the_apps_page() {
        for p in PERMISSIONS {
            assert!(p.key.starts_with("apps."), "{}", p.key);
            assert_eq!(find(p.key), Some(p));
            #[cfg(feature = "icons")]
            assert!(
                crate::icons::BUILT_IN
                    .iter()
                    .any(|(name, _)| *name == p.icon),
                "{} names no built-in icon {}",
                p.key,
                p.icon
            );
            // Only a location's `ask` has no entry: the portal asks itself.
            for choice in p.choices {
                let no_entry = p.key == "apps.location" && *choice == ASK;
                assert_eq!(
                    store_values(p.key, choice).is_none(),
                    no_entry,
                    "{} {choice}",
                    p.key
                );
            }
        }
    }

    #[test]
    fn a_choice_becomes_the_stores_word_and_back() {
        for choice in [ALLOW, ASK, NEVER] {
            let values = store_values("apps.camera", choice).unwrap();
            assert_eq!(choice_of(&[values[0].to_string()]), Some(choice));
        }
        assert_eq!(choice_of(&[]), None);
        assert_eq!(choice_of(&["maybe".into()]), None);
    }

    #[test]
    fn a_location_is_its_accuracy_and_a_time_and_back() {
        // Allowed, the portal keeps an accuracy and a time; never, none.
        assert_eq!(
            store_values("apps.location", ALLOW),
            Some(&["EXACT", "0"][..])
        );
        assert_eq!(
            store_values("apps.location", NEVER),
            Some(&["NONE", "0"][..])
        );
        assert_eq!(store_values("apps.location", ASK), None);
        assert_eq!(choice_of(&["EXACT".into(), "0".into()]), Some(ALLOW));
        assert_eq!(choice_of(&["CITY".into(), "5".into()]), Some(ALLOW));
        assert_eq!(choice_of(&["NONE".into(), "0".into()]), Some(NEVER));
    }

    #[test]
    fn an_allowed_location_is_set_and_an_asked_one_is_not() {
        let allowed = choices("apps.location", &[("org.example.A", "allow")]);
        let entry = entries(&allowed);
        assert_eq!(entry.len(), 1);
        assert_eq!(entry[0].values, ["EXACT", "0"]);
        let asked = choices("apps.location", &[("org.example.A", "ask")]);
        assert_eq!(entries(&asked), []);
        let never = choices("apps.location", &[("org.example.A", "never")]);
        assert_eq!(entries(&never)[0].values, ["NONE", "0"]);
        assert_eq!(asked_every_time(&asked, "apps.location"), ["org.example.A"]);
    }

    #[test]
    fn asked_every_time_lists_only_the_asking_apps_sorted() {
        let all = choices(
            "apps.location",
            &[
                ("org.example.B", "ask"),
                ("org.example.A", "allow"),
                ("org.example.C", "ask"),
            ],
        );
        assert_eq!(
            asked_every_time(&all, "apps.location"),
            ["org.example.B", "org.example.C"]
        );
        assert_eq!(asked_every_time(&all, "apps.camera"), Vec::<String>::new());
    }

    #[test]
    fn the_camera_set_to_never_is_the_devices_tables_no() {
        let all = choices(
            "apps.camera",
            &[("org.gnome.Snapshot", "never"), ("com.zoom.Zoom", "ask")],
        );
        assert_eq!(
            entries(&all),
            [
                Entry {
                    table: "devices",
                    id: "camera",
                    app: "com.zoom.Zoom".into(),
                    values: &["ask"]
                },
                Entry {
                    table: "devices",
                    id: "camera",
                    app: "org.gnome.Snapshot".into(),
                    values: &["no"]
                },
            ]
        );
    }

    #[test]
    fn the_persons_choice_wins_over_the_machines_app_by_app() {
        let machine = "format = 1\n[apps]\ncamera = { \"org.example.A\" = \"never\", \"org.example.B\" = \"never\" }\n";
        let person = "format = 1\n[apps]\ncamera = { \"org.example.A\" = \"allow\" }\nfiles = { \"org.example.C\" = \"ask\" }\n";
        let all = choices_between(Some(machine), Some(person));
        let camera = &all["apps.camera"];
        assert_eq!(camera["org.example.A"], "allow");
        assert_eq!(camera["org.example.B"], "never");
        assert_eq!(files_asked(&all), ["org.example.C"]);
        assert_eq!(choices_between(None, Some("not toml [")), Choices::new());
    }

    #[test]
    fn a_choice_a_permission_does_not_take_is_left_out() {
        let all = choices("apps.screenshots", &[("org.example.A", "ask")]);
        assert_eq!(entries(&all), []);
        // Files live in the override file, not the store.
        let files = choices("apps.files", &[("org.example.A", "ask")]);
        assert_eq!(entries(&files), []);
        assert_eq!(files_asked(&files), ["org.example.A"]);
    }

    #[test]
    fn only_entries_we_set_and_no_longer_want_are_dropped() {
        let before = entries(&choices(
            "apps.camera",
            &[("org.example.A", "never"), ("org.example.B", "allow")],
        ));
        let now = entries(&choices("apps.camera", &[("org.example.B", "never")]));
        let gone = dropped(&before, &now);
        assert_eq!(gone.len(), 1);
        assert_eq!(gone[0].app, "org.example.A");
        assert_eq!(dropped(&now, &now), []);
    }

    #[test]
    fn asking_for_files_takes_the_home_away_and_keeps_the_rest() {
        let text = "[Context]\nsockets=wayland;\nfilesystems=xdg-config/gtk-4.0:ro;\n\n[Environment]\nA=1\n";
        let asked = override_text(text, true).unwrap();
        assert!(asked.contains("sockets=wayland;"));
        assert!(asked.contains("[Environment]\nA=1"));
        assert!(asked.contains("filesystems=xdg-config/gtk-4.0:ro;!host;!home;"));
        assert_eq!(override_text(&asked, true), None);
        let allowed = override_text(&asked, false).unwrap();
        assert!(allowed.contains("filesystems=xdg-config/gtk-4.0:ro;\n"));
        assert!(!allowed.contains('!'));
    }

    #[test]
    fn a_new_override_file_holds_only_the_line() {
        let asked = override_text("", true).unwrap();
        assert!(asked.starts_with("[Context]\nfilesystems=!host;!home;"));
        assert!(asked.ends_with(";\n"));
        // Allowing again leaves nothing, and nothing to write the first time.
        assert_eq!(override_text(&asked, false).unwrap(), "");
        assert_eq!(override_text("", false), None);
    }

    #[test]
    fn a_context_group_elsewhere_gets_the_line_under_it() {
        let text = "[Session Bus Policy]\norg.example=talk\n\n[Context]\nshared=network;\n";
        let asked = override_text(text, true).unwrap();
        assert!(asked.contains("[Context]\nfilesystems=!host;"));
        assert!(asked.contains("shared=network;"));
        let allowed = override_text(&asked, false).unwrap();
        assert_eq!(allowed, text);
    }
}
