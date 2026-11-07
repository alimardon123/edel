//! Undoing an app's update (roadmap M7.20): what `edel update` writes down
//! just before it updates the default apps, and what `edel app
//! undo-update` reads to put one back.
//!
//! An app's older version comes from Flatpak's own history: the commit it
//! ran before, downloaded again from its remote and switched to with
//! `flatpak update --commit`, then held with `flatpak mask` until the
//! person updates it again. Its data comes from a copy of each person's
//! `~/.var/app/ID` taken just before the update, kept for the last update
//! only, and swapped in by renames, so nothing is deleted. A default app's
//! record is root's, under [`places::APP_UNDO_DIR`]; the copies are each
//! person's own, under [`places::UNDO_IN_HOME`] in their home, made and
//! moved as that person. A person's own app, which Bazaar updates, has no
//! record: its undo puts back the version before the one installed, and
//! its data stays as it is.
//!
//! This file holds the record and the pure rules; the binary's
//! `app_update` runs Flatpak.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::apps::is_flatpak_id;
use crate::places;

/// The largest data folder copied before an update, in MiB. A copy costs
/// its size on the data partition until the app's next update, so a folder
/// over this is not copied and its undo brings back the version only: a
/// game launcher's folder, which holds its games, is the case this keeps
/// from filling the disk (Efficient over Functional, M7.20).
pub const COPY_LIMIT_MIB: u64 = 2048;

/// The share of the data partition, in percent, a copy must leave free.
pub const KEEP_FREE_PERCENT: u64 = 10;

/// The part of an app's data folder never copied: what the app can make
/// again (the XDG cache Flatpak gives it).
pub const LEFT_OUT: &str = "cache";

/// The record's format; a newer one is not read.
pub const FORMAT: u32 = 1;

/// One version of an app: Flatpak's commit, the version its metadata
/// names (empty when it names none) and the runtime it runs on.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Point {
    pub commit: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub version: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub runtime: String,
}

/// What happened to one person's data before the update.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Data {
    /// Copied, so the undo brings it back.
    Copied,
    /// Larger than [`COPY_LIMIT_MIB`].
    TooLarge,
    /// The copy would have left the data partition under
    /// [`KEEP_FREE_PERCENT`] free.
    NoSpace,
    /// The app was open, and a copy of files it writes can be torn.
    Open,
    /// The copy failed; the update went on.
    Failed,
}

/// What `edel update` wrote down about a default app's last update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub format: u32,
    /// The day of the update, `YYYY-MM-DD`.
    pub date: String,
    pub before: Point,
    /// Filled once the update is done; none when it failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<Point>,
    /// Each person who had data of the app, and what became of it.
    #[serde(default)]
    pub people: BTreeMap<String, Data>,
}

impl Record {
    /// The record as its file holds it.
    pub fn text(&self) -> String {
        toml::to_string(self).unwrap_or_default()
    }

    /// A record from its file; none when it is not one, or of a newer
    /// format.
    pub fn read(text: &str) -> Option<Record> {
        toml::from_str::<Record>(text)
            .ok()
            .filter(|r| r.format == FORMAT && !r.before.commit.is_empty())
    }

    /// Whether the update can be undone while `deployed` runs: the update
    /// finished and is what runs now.
    pub fn undoes(&self, deployed: &str) -> bool {
        self.after.as_ref().is_some_and(|a| a.commit == deployed)
    }
}

/// Where a default app's record is; none for what is not an app id, so a
/// name can never lead out of the folder.
pub fn record_path(id: &str) -> Option<PathBuf> {
    is_flatpak_id(id).then(|| Path::new(places::APP_UNDO_DIR).join(format!("{id}.toml")))
}

/// A default app's record, when there is one.
pub fn record(id: &str) -> Option<Record> {
    Record::read(&fs::read_to_string(record_path(id)?).ok()?)
}

/// The folder in `home` that holds the copies of `id`'s data: `data`, the
/// copy taken before the last update, and `newer`, the data an undo set
/// aside. None for what is not an app id.
pub fn copies_in(home: &Path, id: &str) -> Option<PathBuf> {
    is_flatpak_id(id).then(|| home.join(places::UNDO_IN_HOME).join(id))
}

/// What to do with a person's data before an update (pure): `size` is the
/// folder's size in bytes, none when it is over [`COPY_LIMIT_MIB`];
/// `free` and `total` the data partition's bytes; `open` whether the app
/// runs for that person. None means copy it.
pub fn before_copy(size: Option<u64>, free: u64, total: u64, open: bool) -> Option<Data> {
    if open {
        return Some(Data::Open);
    }
    let Some(size) = size else {
        return Some(Data::TooLarge);
    };
    let keep = total / 100 * KEEP_FREE_PERCENT;
    if free < size.saturating_add(keep) {
        return Some(Data::NoSpace);
    }
    None
}

/// The size of the folder at `path` in bytes, without following links or
/// counting [`LEFT_OUT`] at its top; none as soon as it passes `limit`
/// bytes, so a huge folder is not walked to its end.
pub fn size_up_to(path: &Path, limit: u64) -> Option<u64> {
    fn walk(dir: &Path, total: &mut u64, limit: u64, top: bool) -> bool {
        let Ok(entries) = fs::read_dir(dir) else {
            return true;
        };
        for entry in entries.flatten() {
            if top && entry.file_name() == LEFT_OUT {
                continue;
            }
            let Ok(meta) = entry.path().symlink_metadata() else {
                continue;
            };
            if meta.is_dir() {
                if !walk(&entry.path(), total, limit, false) {
                    return false;
                }
            } else {
                *total = total.saturating_add(meta.len());
            }
            if *total > limit {
                return false;
            }
        }
        true
    }
    let mut total = 0;
    walk(path, &mut total, limit, true).then_some(total)
}

/// The app ids in a `flatpak remote-ls --updates --app
/// --columns=application` listing, keeping only real ids.
pub fn updates(listing: &str) -> Vec<String> {
    let ids: BTreeSet<&str> = listing
        .lines()
        .map(str::trim)
        .filter(|id| is_flatpak_id(id))
        .collect();
    ids.into_iter().map(String::from).collect()
}

/// The apps `flatpak mask` holds, from its listing: a pattern that is an
/// app id holds that app.
pub fn held(listing: &str) -> BTreeSet<String> {
    listing
        .lines()
        .map(str::trim)
        .filter(|p| is_flatpak_id(p))
        .map(String::from)
        .collect()
}

/// One installed app, as `flatpak list --app` gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub id: String,
    /// Its ref, `ID/ARCH/BRANCH`, which every command names it by, so two
    /// branches of one app are never mixed up.
    pub reference: String,
    pub now: Point,
}

/// The columns [`installed`] reads, in its order.
pub const LIST_COLUMNS: &str = "--columns=application,ref,active,version,runtime";

/// The apps of a `flatpak list --app` listing with [`LIST_COLUMNS`] (its
/// columns are tab-separated); lines that are not an app are skipped.
pub fn installed(listing: &str) -> Vec<Installed> {
    listing
        .lines()
        .filter_map(|line| {
            let mut f = line.split('\t').map(str::trim);
            let id = f.next()?;
            let reference = f.next()?;
            let commit = f.next()?;
            let version = f.next().unwrap_or("");
            let runtime = f.next().unwrap_or("");
            (is_flatpak_id(id) && reference.starts_with(id) && !commit.is_empty()).then(|| {
                Installed {
                    id: id.to_string(),
                    reference: reference.to_string(),
                    now: Point {
                        commit: commit.to_string(),
                        version: version.to_string(),
                        runtime: runtime.to_string(),
                    },
                }
            })
        })
        .collect()
}

/// How a version is named to a person: its version, else the day of the
/// update, never a commit.
pub fn version_words(point: &Point, date: &str) -> String {
    if point.version.is_empty() {
        crate::i18n::trf("the version of {date}", &[("date", date)])
    } else {
        point.version.clone()
    }
}

/// The runtimes the records of `ids`' last updates ran on before, which
/// tidying the installation keeps, so an undo finds them (M6.2a's
/// `Tidy`): a runtime as `flatpak list --runtime --columns=ref` names it.
pub fn runtimes_before(records: &[Record]) -> Vec<String> {
    let kept: BTreeSet<&str> = records
        .iter()
        .map(|r| r.before.runtime.as_str())
        .filter(|r| !r.is_empty())
        .collect();
    kept.into_iter().map(String::from).collect()
}

/// What `edel app undo-update --check ID` says of an app, for Settings'
/// app page (M7.20): lines a program reads, which stay English.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Check {
    /// Whether it is one of the default apps, which an administrator
    /// changes, rather than the person's own.
    pub default: bool,
    /// The version that runs, as [`version_words`] names it.
    pub version: String,
    /// Whether it stays on this version until `edel app update`.
    pub held: bool,
    /// The version an undo brings back, empty when the update cannot be
    /// undone; for a person's own app, `before` (the version before the
    /// one installed, which only its remote knows).
    pub undo: String,
    /// The day of the update, when edel made it.
    pub date: String,
    /// Whether the person's data comes back with it.
    pub data: bool,
}

impl Check {
    /// The check of an app that `now` runs: `record` is its last update's
    /// when edel made it, `person` the one asking.
    pub fn of(
        default: bool,
        now: &Point,
        held: bool,
        record: Option<&Record>,
        person: &str,
    ) -> Check {
        let usable = record.filter(|r| r.undoes(&now.commit));
        let undo = match (default, usable) {
            (_, Some(r)) => version_words(&r.before, &r.date),
            (false, None) => "before".to_string(),
            (true, None) => String::new(),
        };
        Check {
            default,
            version: version_words(now, usable.map(|r| r.date.as_str()).unwrap_or("")),
            held,
            undo: if held { String::new() } else { undo },
            date: usable.map(|r| r.date.clone()).unwrap_or_default(),
            data: usable.is_some_and(|r| r.people.get(person) == Some(&Data::Copied)),
        }
    }

    /// The lines the command prints.
    pub fn text(&self) -> String {
        let yes = |b: bool| if b { "yes" } else { "no" };
        format!(
            "installation: {}\nversion: {}\nheld: {}\nundo: {}\ndate: {}\ndata: {}\n",
            if self.default { "default" } else { "own" },
            self.version,
            yes(self.held),
            if self.undo.is_empty() {
                "no"
            } else {
                &self.undo
            },
            self.date,
            yes(self.data),
        )
    }

    /// The check from the command's lines; none when they are not one.
    pub fn read(text: &str) -> Option<Check> {
        let mut check = Check::default();
        let mut seen = 0;
        for line in text.lines() {
            let Some((key, value)) = line.split_once(": ").or_else(|| line.split_once(':')) else {
                continue;
            };
            let value = value.trim();
            match key {
                "installation" => check.default = value == "default",
                "version" => check.version = value.to_string(),
                "held" => check.held = value == "yes",
                "undo" => {
                    check.undo = if value == "no" {
                        String::new()
                    } else {
                        value.to_string()
                    }
                }
                "date" => check.date = value.to_string(),
                "data" => check.data = value == "yes",
                _ => continue,
            }
            seen += 1;
        }
        (seen == 6).then_some(check)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(commit: &str, version: &str) -> Point {
        Point {
            commit: commit.into(),
            version: version.into(),
            runtime: "org.freedesktop.Platform/x86_64/24.08".into(),
        }
    }

    #[test]
    fn a_record_reads_back_and_a_newer_format_is_not_read() {
        let record = Record {
            format: FORMAT,
            date: "2026-11-07".into(),
            before: point("aaa", "127.0"),
            after: Some(point("bbb", "128.0")),
            people: BTreeMap::from([
                ("ali".to_string(), Data::Copied),
                ("sam".to_string(), Data::TooLarge),
            ]),
        };
        assert_eq!(Record::read(&record.text()), Some(record.clone()));
        assert!(record.text().contains("too-large"), "{}", record.text());
        let newer = record.text().replace("format = 1", "format = 2");
        assert_eq!(Record::read(&newer), None);
        assert_eq!(Record::read("not a record"), None);
    }

    #[test]
    fn an_update_is_undone_only_while_its_version_runs() {
        let mut record = Record {
            format: FORMAT,
            date: "2026-11-07".into(),
            before: point("aaa", "127.0"),
            after: None,
            people: BTreeMap::new(),
        };
        assert!(!record.undoes("aaa"), "an update that did not finish");
        record.after = Some(point("bbb", "128.0"));
        assert!(record.undoes("bbb"));
        assert!(!record.undoes("ccc"), "updated again by hand since");
    }

    #[test]
    fn paths_are_made_only_from_app_ids() {
        assert_eq!(
            record_path("org.mozilla.firefox"),
            Some(PathBuf::from(
                "/data/edel/app-undo/org.mozilla.firefox.toml"
            ))
        );
        assert_eq!(record_path("../etc/passwd"), None);
        assert_eq!(record_path("org.example/../x"), None);
        assert_eq!(
            copies_in(Path::new("/home/ali"), "org.mozilla.firefox"),
            Some(PathBuf::from(
                "/home/ali/.local/share/edel/undo/org.mozilla.firefox"
            ))
        );
        assert_eq!(copies_in(Path::new("/home/ali"), ".."), None);
    }

    #[test]
    fn data_is_copied_unless_open_too_large_or_short_of_space() {
        const GIB: u64 = 1 << 30;
        assert_eq!(before_copy(Some(GIB), 50 * GIB, 100 * GIB, false), None);
        assert_eq!(
            before_copy(Some(GIB), 50 * GIB, 100 * GIB, true),
            Some(Data::Open)
        );
        assert_eq!(
            before_copy(None, 50 * GIB, 100 * GIB, false),
            Some(Data::TooLarge)
        );
        // 10 GiB must stay free besides the copy itself.
        assert_eq!(
            before_copy(Some(GIB), 10 * GIB, 100 * GIB, false),
            Some(Data::NoSpace)
        );
        assert_eq!(before_copy(Some(GIB), 11 * GIB + 1, 100 * GIB, false), None);
    }

    #[test]
    fn the_size_walk_leaves_out_the_cache_and_stops_past_the_limit() {
        let dir = std::env::temp_dir().join(format!("edel-app-undo-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("cache")).unwrap();
        fs::create_dir_all(dir.join("data/deep")).unwrap();
        fs::write(dir.join("cache/big"), vec![0u8; 5000]).unwrap();
        fs::write(dir.join("data/deep/a"), vec![0u8; 300]).unwrap();
        fs::write(dir.join("config"), vec![0u8; 200]).unwrap();
        std::os::unix::fs::symlink("/", dir.join("link")).unwrap();
        let link_len = dir.join("link").symlink_metadata().unwrap().len();
        assert_eq!(size_up_to(&dir, 10_000), Some(500 + link_len));
        assert_eq!(size_up_to(&dir, 400), None);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn listings_give_only_app_ids() {
        assert_eq!(
            updates(
                "org.mozilla.firefox\n\norg.gnome.TextEditor\nnot an id\norg.mozilla.firefox\n"
            ),
            ["org.gnome.TextEditor", "org.mozilla.firefox"]
        );
        assert_eq!(
            held("org.freedesktop.Platform.GL.default\norg.mozilla.firefox\n*.Locale\n"),
            BTreeSet::from([
                "org.freedesktop.Platform.GL.default".to_string(),
                "org.mozilla.firefox".to_string()
            ])
        );
    }

    #[test]
    fn the_installed_listing_gives_each_app_by_its_ref() {
        let listing = "org.mozilla.firefox\torg.mozilla.firefox/x86_64/stable\tbbb\t128.0\torg.freedesktop.Platform/x86_64/24.08\n\
                       org.gnome.TextEditor\torg.gnome.TextEditor/x86_64/stable\tccc\t\torg.gnome.Platform/x86_64/48\n\
                       garbage line\n";
        let apps = installed(listing);
        assert_eq!(apps.len(), 2);
        assert_eq!(apps[0].reference, "org.mozilla.firefox/x86_64/stable");
        assert_eq!(apps[0].now, point("bbb", "128.0"));
        assert_eq!(apps[1].now.version, "");
    }

    #[test]
    fn a_version_is_named_by_its_number_or_its_day_never_its_commit() {
        assert_eq!(version_words(&point("aaa", "127.0"), "2026-11-07"), "127.0");
        assert_eq!(
            version_words(&point("aaa", ""), "2026-11-07"),
            "the version of 2026-11-07"
        );
    }

    #[test]
    fn tidying_keeps_each_record_s_runtime_before() {
        let record = |runtime: &str| Record {
            format: FORMAT,
            date: "2026-11-07".into(),
            before: Point {
                commit: "aaa".into(),
                version: String::new(),
                runtime: runtime.into(),
            },
            after: None,
            people: BTreeMap::new(),
        };
        assert_eq!(
            runtimes_before(&[
                record("org.gnome.Platform/x86_64/47"),
                record(""),
                record("org.gnome.Platform/x86_64/47")
            ]),
            ["org.gnome.Platform/x86_64/47"]
        );
    }

    #[test]
    fn the_check_says_what_an_undo_brings_back_and_reads_back() {
        let record = Record {
            format: FORMAT,
            date: "2026-11-07".into(),
            before: point("aaa", "127.0"),
            after: Some(point("bbb", "128.0")),
            people: BTreeMap::from([("ali".to_string(), Data::Copied)]),
        };
        let check = Check::of(true, &point("bbb", "128.0"), false, Some(&record), "ali");
        assert_eq!(check.undo, "127.0");
        assert!(check.data);
        assert_eq!(Check::read(&check.text()), Some(check.clone()));
        assert!(!Check::of(true, &point("bbb", "128.0"), false, Some(&record), "sam").data);
        // Held: nothing to undo until it updates again.
        assert_eq!(
            Check::of(true, &point("aaa", "127.0"), true, Some(&record), "ali").undo,
            ""
        );
        // Updated another way since: the record no longer undoes it.
        assert_eq!(
            Check::of(true, &point("ccc", "129.0"), false, Some(&record), "ali").undo,
            ""
        );
        // A person's own app: the version before, its data as it is.
        let own = Check::of(false, &point("ccc", "1.2"), false, None, "ali");
        assert_eq!((own.undo.as_str(), own.data), ("before", false));
        assert_eq!(Check::read("version: 1\n"), None);
    }
}
