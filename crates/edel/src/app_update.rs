//! Undoing an app's update (roadmap M7.20): the copies `edel update` takes
//! before it updates the default apps, and `edel app undo-update` and
//! `edel app update`, which put an app back and let it update again. The
//! record and its rules are `edel::app_undo`'s.
//!
//! Root works on the default apps' installation and writes only their
//! records; everything inside a person's home (the copy, the swap) runs as
//! that person (`machine::as_user`), so a link they plant there leads root
//! nowhere. A person runs the same commands on their own apps, which have
//! no record: the version before the one installed comes back, the data
//! stays.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use edel::app_undo::{self, COPY_LIMIT_MIB, Data, FORMAT, Installed, LIST_COLUMNS, Point, Record};
use edel::apps::{APPS_REMOTE, INSTALLATION, is_flatpak_id};
use edel::exit::{Coded, OFFLINE, REFUSED};
use edel::i18n::{tr, trf};
use edel::places;

use crate::machine::{self, Account};
use crate::update::{Lock, run};

/// Copies `~/.var/app/ID`'s entries but the cache into the copies folder's
/// `data.new`, then renames it to `data`, so a copy cut short is never
/// taken for a whole one. Run as the person; `$1` is the data folder, `$2`
/// the copies folder. Sockets are left out, as an app makes them again.
const COPY: &str = r#"set -e
src=$1 dst=$2
mkdir -p "$dst"
rm -rf "$dst/data.new"
mkdir "$dst/data.new"
for e in "$src"/* "$src"/.[!.]* "$src"/..?*; do
	[ -e "$e" ] || [ -L "$e" ] || continue
	[ "${e##*/}" = cache ] && continue
	[ -S "$e" ] && continue
	cp -a "$e" "$dst/data.new/"
done
rm -rf "$dst/data"
mv "$dst/data.new" "$dst/data"
"#;

/// Swaps the data folder with the copy, by renames on one filesystem, so
/// nothing is deleted but the data an earlier undo set aside: the data now
/// goes to `newer`, the copy to the data folder. Run as the person; `$1` is
/// the data folder, `$2` the copies folder. `mv` across filesystems would
/// copy, so a copies folder elsewhere stops it.
const SWAP: &str = r#"set -e
src=$1 dst=$2
[ -d "$dst/data" ]
[ "$(stat -c %d "$src")" = "$(stat -c %d "$dst")" ] || { echo "the copy is on another filesystem" >&2; exit 1; }
rm -rf "$dst/newer"
mv "$src" "$dst/newer"
mv "$dst/data" "$src"
"#;

/// Which installation a command works on.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Of {
    /// The default apps', as root.
    Default,
    /// The person's own, as that person.
    Own,
}

fn flatpak(of: Of) -> Command {
    let mut command = Command::new("flatpak");
    command.arg(match of {
        Of::Default => format!("--installation={INSTALLATION}"),
        Of::Own => "--user".to_string(),
    });
    command
}

/// A command's standard output; empty when it fails or does not run.
fn output(command: &mut Command) -> String {
    match command.stderr(Stdio::null()).output() {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).into_owned(),
        _ => String::new(),
    }
}

fn installed(of: Of) -> Vec<Installed> {
    app_undo::installed(&output(flatpak(of).args(["list", "--app", LIST_COLUMNS])))
}

fn held(of: Of) -> std::collections::BTreeSet<String> {
    app_undo::held(&output(flatpak(of).arg("mask")))
}

/// The people (root aside) on this machine.
fn people() -> Result<Vec<Account>> {
    let passwd = fs::read_to_string("/etc/passwd").context("could not read /etc/passwd")?;
    Ok(machine::accounts(&passwd)
        .into_iter()
        .filter(|a| machine::is_person(a) && a.name != "root")
        .collect())
}

/// `account`'s data folder of `id`, when it is a real folder, never a link.
fn data_of(account: &Account, id: &str) -> Option<std::path::PathBuf> {
    let path = Path::new(&account.home).join(".var/app").join(id);
    path.symlink_metadata()
        .is_ok_and(|m| m.is_dir())
        .then_some(path)
}

/// Whether `id` runs for `account` now, from `flatpak ps` run as that
/// person in their own session folder.
fn open_for(account: &Account, id: &str) -> bool {
    let listing = output(
        machine::as_user(account, "flatpak", &["ps", "--columns=application"])
            .env("XDG_RUNTIME_DIR", machine::runtime_dir(account)),
    );
    listing.lines().any(|line| line.trim() == id)
}

/// Whether `id` runs for the person running this command.
fn open_here(id: &str) -> bool {
    output(Command::new("flatpak").args(["ps", "--columns=application"]))
        .lines()
        .any(|line| line.trim() == id)
}

/// The data partition's free and total bytes.
fn space() -> (u64, u64) {
    let Ok(path) = std::ffi::CString::new(places::DATA_MOUNT) else {
        return (0, 0);
    };
    // SAFETY: statvfs writes into the zeroed struct it is given and reads
    // only the path, which lives across the call.
    let mut stats: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(path.as_ptr(), &mut stats) } != 0 {
        return (0, 0);
    }
    let unit = stats.f_frsize as u64;
    (stats.f_bavail as u64 * unit, stats.f_blocks as u64 * unit)
}

/// An update `edel update` is about to make, written down.
pub(crate) struct Pending {
    id: String,
    record: Record,
}

/// Before the default apps are updated: for each app with an update that
/// is not held, records the version it runs and copies each person's data
/// as that person, when it may (`app_undo::before_copy`). `only` limits it
/// to one app. A failure here never stops the update: the record says
/// what was not copied, and an app with no record can still be updated.
pub(crate) fn before_update(only: Option<&str>) -> Vec<Pending> {
    let with_updates = app_undo::updates(&output(flatpak(Of::Default).args([
        "remote-ls",
        "--updates",
        "--app",
        "--columns=application",
    ])));
    let held = held(Of::Default);
    let people = people().unwrap_or_default();
    let (mut free, total) = space();
    let mut pending = Vec::new();
    for app in installed(Of::Default) {
        if !with_updates.contains(&app.id)
            || held.contains(&app.id)
            || only.is_some_and(|o| o != app.id)
        {
            continue;
        }
        let mut record = Record {
            format: FORMAT,
            date: crate::release::today_text(),
            before: app.now.clone(),
            after: None,
            people: BTreeMap::new(),
        };
        for account in &people {
            let Some(data) = data_of(account, &app.id) else {
                continue;
            };
            let size = app_undo::size_up_to(&data, COPY_LIMIT_MIB << 20);
            let state = match app_undo::before_copy(size, free, total, open_for(account, &app.id)) {
                Some(state) => state,
                None => copy(account, &app.id, &data),
            };
            if state == Data::Copied {
                free = free.saturating_sub(size.unwrap_or(0));
            }
            record.people.insert(account.name.clone(), state);
        }
        if let Err(err) = write(&app.id, &record) {
            eprintln!("warning: {err:#}");
        }
        pending.push(Pending { id: app.id, record });
    }
    pending
}

/// Copies `account`'s `data` of `id` as that person, saying how big the
/// copy is and how long it took.
fn copy(account: &Account, id: &str, data: &Path) -> Data {
    let Some(copies) = app_undo::copies_in(Path::new(&account.home), id) else {
        return Data::Failed;
    };
    let started = std::time::Instant::now();
    let copied = run(&mut machine::as_user(
        account,
        "sh",
        &[
            "-c",
            COPY,
            "sh",
            &data.display().to_string(),
            &copies.display().to_string(),
        ],
    ));
    match copied {
        Ok(()) => {
            let size = app_undo::size_up_to(&copies.join("data"), u64::MAX).unwrap_or(0);
            println!(
                "edel update: {}",
                trf(
                    "copied {person}'s data of {app} before its update ({mib} MiB in {seconds} s), so the update can be undone",
                    &[
                        ("person", &account.name),
                        ("app", id),
                        ("mib", &(size >> 20).to_string()),
                        ("seconds", &started.elapsed().as_secs().to_string()),
                    ],
                )
            );
            Data::Copied
        }
        Err(err) => {
            eprintln!(
                "warning: {}: {err:#}",
                trf(
                    "could not copy {person}'s data of {app} before its update; it updates, and its undo brings back the version only",
                    &[("person", &account.name), ("app", id)],
                )
            );
            Data::Failed
        }
    }
}

fn write(id: &str, record: &Record) -> Result<()> {
    let path = app_undo::record_path(id).context("not an app id")?;
    fs::create_dir_all(places::APP_UNDO_DIR)
        .with_context(|| trf("could not write {path}", &[("path", places::APP_UNDO_DIR)]))?;
    let new = path.with_extension("toml.new");
    fs::write(&new, record.text())
        .and_then(|()| fs::rename(&new, &path))
        .with_context(|| {
            trf(
                "could not write {path}",
                &[("path", &path.display().to_string())],
            )
        })
}

/// After the update: each record learns the version that runs now. An app
/// the update left as it was keeps no `after`, so its undo is refused.
pub(crate) fn after_update(pending: Vec<Pending>) {
    if pending.is_empty() {
        return;
    }
    let now = installed(Of::Default);
    for mut p in pending {
        let Some(app) = now.iter().find(|a| a.id == p.id) else {
            continue;
        };
        if app.now.commit == p.record.before.commit {
            continue;
        }
        p.record.after = Some(app.now.clone());
        if let Err(err) = write(&p.id, &p.record) {
            eprintln!("warning: {err:#}");
        }
    }
}

/// The runtimes the records' versions before ran on, which tidying the
/// default apps keeps (M6.2a), so an undo finds them.
pub(crate) fn runtimes_to_keep() -> Vec<String> {
    let records: Vec<Record> = installed(Of::Default)
        .iter()
        .filter_map(|a| app_undo::record(&a.id))
        .collect();
    app_undo::runtimes_before(&records)
}

fn refused(words: String) -> anyhow::Error {
    anyhow::Error::new(Coded::new(REFUSED, format!("{}: {words}", tr("refused"))))
}

/// The installation this command works on: root works on the default
/// apps, a person on their own. A person naming a default app they do not
/// have themselves is told to ask an administrator.
fn which(id: &str, root: bool, verb: &str) -> Result<(Of, Installed)> {
    if !is_flatpak_id(id) {
        bail!(refused(trf(
            "{app} is not an app id; write one such as org.mozilla.firefox, as edel settings get apps.installed shows them",
            &[("app", id)]
        )));
    }
    let of = if root { Of::Default } else { Of::Own };
    if let Some(app) = installed(of).into_iter().find(|a| a.id == id) {
        return Ok((of, app));
    }
    if !root && installed(Of::Default).iter().any(|a| a.id == id) {
        bail!(refused(trf(
            "{app} is one of the default apps, which an administrator changes: doas edel app {verb} {app}",
            &[("app", id), ("verb", verb)]
        )));
    }
    bail!(refused(trf("{app} is not installed", &[("app", id)])))
}

/// `edel app undo-update --check ID`: what an undo of the app would bring
/// back, as `app_undo::Check`'s lines, for Settings' app page. Anyone may
/// ask; a person's own copy of the app comes first, as Flatpak runs it.
pub(crate) fn check(id: &str) -> Result<()> {
    if !is_flatpak_id(id) {
        bail!(refused(trf(
            "{app} is not an app id; write one such as org.mozilla.firefox, as edel settings get apps.installed shows them",
            &[("app", id)]
        )));
    }
    let uid = edel::users::current();
    let root = uid == Some(0);
    let person = edel::users::read(uid)
        .into_iter()
        .find(|p| Some(p.uid) == uid)
        .map(|p| p.name)
        .unwrap_or_default();
    let mine = if root {
        None
    } else {
        installed(Of::Own).into_iter().find(|a| a.id == id)
    };
    let (of, app) = match mine {
        Some(app) => (Of::Own, app),
        None => (
            Of::Default,
            installed(Of::Default)
                .into_iter()
                .find(|a| a.id == id)
                .ok_or_else(|| refused(trf("{app} is not installed", &[("app", id)])))?,
        ),
    };
    let record = (of == Of::Default).then(|| app_undo::record(id)).flatten();
    let check = app_undo::Check::of(
        of == Of::Default,
        &app.now,
        held(of).contains(id),
        record.as_ref(),
        &person,
    );
    print!("{}", check.text());
    Ok(())
}

/// `edel app undo-update ID`: puts the app back on the version before its
/// last update, with each person's data from just before it when a copy
/// was taken, and holds it there until `edel app update`.
pub(crate) fn undo(id: &str, root: bool) -> Result<()> {
    let _lock = Lock::take("apps", "edel app undo-update")?;
    let (of, app) = which(id, root, "undo-update")?;
    if held(of).contains(id) {
        bail!(refused(trf(
            "{app} already stays on {version}; edel app update {app} updates it",
            &[
                ("app", id),
                ("version", &app_undo::version_words(&app.now, ""))
            ]
        )));
    }
    let (before, record, people) = match of {
        Of::Default => {
            let record = app_undo::record(id)
                .filter(|r| r.undoes(&app.now.commit))
                .ok_or_else(|| {
                    refused(trf(
                        "no update of {app} that edel made can be undone: it was not updated since, or updated another way",
                        &[("app", id)],
                    ))
                })?;
            let people: Vec<Account> = people()?
                .into_iter()
                .filter(|a| record.people.contains_key(&a.name))
                .collect();
            if let Some(a) = people.iter().find(|a| open_for(a, id)) {
                bail!(refused(trf(
                    "{app} is open for {person}; close it, then undo its update",
                    &[("app", id), ("person", &a.name)]
                )));
            }
            (record.before.clone(), Some(record), people)
        }
        Of::Own => {
            if open_here(id) {
                bail!(refused(trf(
                    "{app} is open; close it, then undo its update",
                    &[("app", id)]
                )));
            }
            let parent = output(flatpak(Of::Own).args([
                "remote-info",
                "--show-parent",
                &format!("--commit={}", app.now.commit),
                &origin(Of::Own, &app.reference),
                &app.reference,
            ]));
            let parent = parent.trim();
            if parent.is_empty() || parent == "-" {
                bail!(refused(trf(
                    "{app} has no older version on its remote to go back to, or the network is not reachable",
                    &[("app", id)]
                )));
            }
            (
                Point {
                    commit: parent.to_string(),
                    ..Point::default()
                },
                None,
                Vec::new(),
            )
        }
    };
    let words = record
        .as_ref()
        .map(|r| app_undo::version_words(&r.before, &r.date))
        .unwrap_or_else(|| tr("its version before").to_string());
    run(flatpak(of).args([
        "update",
        "--noninteractive",
        "-y",
        &format!("--commit={}", before.commit),
        &app.reference,
    ]))
    .map_err(|err| {
        anyhow::Error::new(Coded::new(
            OFFLINE,
            trf(
                "{app} {version} is downloaded again from its remote to undo its update, and that failed; connect to the network and try again ({why})",
                &[("app", id), ("version", &words), ("why", &format!("{err:#}"))],
            ),
        ))
    })?;
    run(flatpak(of).args(["mask", id]))?;
    let mut back = Vec::new();
    let mut not_back = Vec::new();
    for account in &people {
        let copied =
            record.as_ref().and_then(|r| r.people.get(&account.name)) == Some(&Data::Copied);
        match (copied, swap(account, id)) {
            (true, Ok(())) => back.push(account.name.clone()),
            (true, Err(err)) => {
                eprintln!("warning: {err:#}");
                not_back.push(account.name.clone());
            }
            (false, _) => not_back.push(account.name.clone()),
        }
    }
    println!(
        "edel app: {}",
        trf(
            "{app} is back on {version}; it stays there until edel app update {app}",
            &[("app", id), ("version", &words)]
        )
    );
    if !back.is_empty() {
        println!(
            "edel app: {}",
            trf(
                "the data of {people} is back from just before the update; the newer data is kept in {folder} until the app's next update",
                &[
                    ("people", &back.join(", ")),
                    ("folder", &format!("~/{}/{id}/newer", places::UNDO_IN_HOME)),
                ],
            )
        );
    }
    if !not_back.is_empty() || of == Of::Own {
        let who = if of == Of::Own {
            tr("your").to_string()
        } else {
            not_back.join(", ")
        };
        println!(
            "edel app: {}",
            trf(
                "the data of {people} stays as it is, as no copy was taken before the update",
                &[("people", &who)]
            )
        );
    }
    Ok(())
}

/// Swaps `account`'s data of `id` with the copy, as that person.
fn swap(account: &Account, id: &str) -> Result<()> {
    let copies = app_undo::copies_in(Path::new(&account.home), id).context("not an app id")?;
    let data = Path::new(&account.home).join(".var/app").join(id);
    run(&mut machine::as_user(
        account,
        "sh",
        &[
            "-c",
            SWAP,
            "sh",
            &data.display().to_string(),
            &copies.display().to_string(),
        ],
    ))
    .with_context(|| {
        trf(
            "could not bring back {person}'s data of {app}",
            &[("person", &account.name), ("app", id)],
        )
    })
}

/// The remote `reference` came from.
fn origin(of: Of, reference: &str) -> String {
    let listed = output(flatpak(of).args(["info", "--show-origin", reference]));
    let name = listed.trim();
    if name.is_empty() {
        APPS_REMOTE.to_string()
    } else {
        name.to_string()
    }
}

/// `edel app update ID`: lets a held app update again and updates it now,
/// taking a new copy of each person's data first, so this too can be
/// undone.
pub(crate) fn update(id: &str, root: bool) -> Result<()> {
    let _lock = Lock::take("apps", "edel app update")?;
    let (of, app) = which(id, root, "update")?;
    if held(of).contains(id) {
        run(flatpak(of).args(["mask", "--remove", id]))?;
    }
    let pending = if of == Of::Default {
        before_update(Some(id))
    } else {
        Vec::new()
    };
    run(flatpak(of).args(["update", "--noninteractive", "-y", &app.reference])).map_err(|err| {
        anyhow::Error::new(Coded::new(
            OFFLINE,
            trf(
                "{app} could not be updated; connect to the network and try again ({why})",
                &[("app", id), ("why", &format!("{err:#}"))],
            ),
        ))
    })?;
    after_update(pending);
    let now = installed(of)
        .into_iter()
        .find(|a| a.id == id)
        .map(|a| app_undo::version_words(&a.now, &crate::release::today_text()))
        .unwrap_or_default();
    println!(
        "edel app: {}",
        trf(
            "{app} is on {version} and updates with the others again",
            &[("app", id), ("version", &now)]
        )
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(script: &str, a: &Path, b: &Path) -> bool {
        Command::new("sh")
            .args(["-c", script, "sh"])
            .arg(a)
            .arg(b)
            .status()
            .is_ok_and(|s| s.success())
    }

    #[test]
    fn the_copy_leaves_out_the_cache_and_the_swap_keeps_both_folders() {
        let root = std::env::temp_dir().join(format!("edel-app-update-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let data = root.join("var/app/org.edel.TestA");
        let copies = root.join("undo/org.edel.TestA");
        fs::create_dir_all(data.join("cache")).unwrap();
        fs::create_dir_all(data.join("config")).unwrap();
        fs::write(data.join("cache/big"), "cache").unwrap();
        fs::write(data.join("config/mark"), "version 1").unwrap();
        fs::write(data.join(".hidden"), "hidden").unwrap();
        assert!(sh(COPY, &data, &copies));
        assert!(!copies.join("data.new").exists());
        assert!(!copies.join("data/cache").exists());
        assert_eq!(
            fs::read_to_string(copies.join("data/config/mark")).unwrap(),
            "version 1"
        );
        assert_eq!(
            fs::read_to_string(copies.join("data/.hidden")).unwrap(),
            "hidden"
        );

        // The update changed the data; the undo swaps it with the copy.
        fs::write(data.join("config/mark"), "version 2").unwrap();
        assert!(sh(SWAP, &data, &copies));
        assert_eq!(
            fs::read_to_string(data.join("config/mark")).unwrap(),
            "version 1"
        );
        assert_eq!(
            fs::read_to_string(copies.join("newer/config/mark")).unwrap(),
            "version 2"
        );
        assert!(!copies.join("data").exists());
        // No copy left: a second swap changes nothing.
        assert!(!sh(SWAP, &data, &copies));
        assert_eq!(
            fs::read_to_string(data.join("config/mark")).unwrap(),
            "version 1"
        );
        fs::remove_dir_all(&root).unwrap();
    }
}
