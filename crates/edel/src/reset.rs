//! `edel factory-reset` (roadmap M8.38): the machine back to new. The
//! command only asks, writes [`places::FACTORY_RESET`] and restarts; the
//! erase itself is [`erase_if_asked`], which `edel boot mount-data` runs
//! on a fresh mount of the data partition, before anything else uses it.
//! It moves everything on the partition but [`KEEP`] into
//! [`places::FACTORY_RESET_TRASH`], a rename each, so it takes moments
//! however much there is and the guard's watchdog never cuts it short;
//! then it deletes the request, and a process of its own deletes the
//! trash while the machine starts. A power cut before the request is gone
//! moves the rest at the next start; one after it leaves only the trash,
//! which the next start deletes. The slots are left as they are: the
//! machine starts the version it runs, can still roll back, and its next
//! start is a first start, seeded by the slot's own settings file.

use std::fs;
use std::io::{self, BufRead, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

use edel::i18n::{tr, trf};
use edel::places;

use crate::system_power;

/// What a reset keeps on the data partition: the default apps, so the
/// machine needs no download; the add-ons' signed images, so adding one
/// back needs none either (none is mounted, as the new settings file names
/// none, and each version's list of failed add-ons goes); and the mark
/// that this disk was installed, so an installed USB disk never becomes a
/// stick that logs `live` in by itself (M6.17).
const KEEP: &[&str] = &[places::DEFAULT_APPS, places::ADDONS_DIR, places::INSTALLED];

/// The file system's own folder, which fsck uses: kept, but emptied, as
/// what fsck found is pieces of the person's files.
const LOST_FOUND: &str = "lost+found";

/// [`KEEP`], the request and the trash, relative to the mount point.
fn kept() -> Vec<PathBuf> {
    KEEP.iter()
        .chain([&places::FACTORY_RESET, &places::FACTORY_RESET_TRASH])
        .map(|path| relative(path))
        .collect()
}

fn relative(path: &str) -> PathBuf {
    Path::new(path)
        .strip_prefix(places::DATA_MOUNT)
        .unwrap_or(Path::new(path))
        .to_path_buf()
}

/// What one erase did: how many entries it moved aside, and those it could
/// not, each with why.
#[derive(Default)]
struct Erased {
    moved: usize,
    failed: Vec<String>,
}

/// Moves everything under `root` joined with `inside` into `trash` but
/// the `kept` paths, relative to `root`; it goes into the folders that
/// lead to them, and into `lost+found`. A link is moved, never followed.
/// An entry that cannot be moved is noted and the rest still go.
fn move_aside(root: &Path, inside: &Path, kept: &[PathBuf], trash: &Path, done: &mut Erased) {
    let dir = root.join(inside);
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(err) => {
            done.failed.push(format!("{}: {err}", dir.display()));
            return;
        }
    };
    for entry in entries.flatten() {
        let rel = inside.join(entry.file_name());
        if kept.contains(&rel) {
            continue;
        }
        let path = entry.path();
        let is_dir = fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_dir());
        if is_dir && (rel == Path::new(LOST_FOUND) || kept.iter().any(|k| k.starts_with(&rel))) {
            move_aside(root, &rel, kept, trash, done);
            continue;
        }
        match fs::rename(&path, trash.join(done.moved.to_string())) {
            Ok(()) => done.moved += 1,
            Err(err) => done.failed.push(format!("{}: {err}", path.display())),
        }
    }
}

/// The erase on the partition mounted at `root`: everything moved aside,
/// each version's list of failed add-ons deleted, then the request.
fn erase(root: &Path) -> Erased {
    let mut done = Erased::default();
    let trash = root.join(relative(places::FACTORY_RESET_TRASH));
    // A trash left by an earlier start gets a fresh folder beside it.
    let trash = (0..)
        .map(|n| trash.join(format!("{n}")))
        .find(|dir| !dir.exists())
        .unwrap_or(trash);
    if let Err(err) = fs::create_dir_all(&trash) {
        done.failed.push(format!("{}: {err}", trash.display()));
        return done;
    }
    move_aside(root, Path::new(""), &kept(), &trash, &mut done);
    if let Ok(versions) = fs::read_dir(root.join(relative(places::ADDONS_DIR))) {
        for version in versions.flatten() {
            let _ = fs::remove_file(version.path().join(places::ADDONS_FAILED));
        }
    }
    // Everything else is moved before the request goes, so a power cut
    // here moves the rest at the next start.
    sync();
    let request = root.join(relative(places::FACTORY_RESET));
    if let Err(err) = fs::remove_file(&request) {
        done.failed.push(format!("{}: {err}", request.display()));
    }
    sync();
    done
}

fn sync() {
    let _ = Command::new("sync").status();
}

/// Deletes the trash under `root` in a process of its own, which outlives
/// `edel boot mount-data`, so the start goes on meanwhile.
fn delete_trash(root: &Path) {
    let trash = root.join(relative(places::FACTORY_RESET_TRASH));
    if !trash.exists() {
        return;
    }
    let started = Command::new("rm")
        .arg("-rf")
        .arg(&trash)
        .stdin(Stdio::null())
        .process_group(0)
        .spawn();
    if let Err(err) = started {
        eprintln!(
            "warning: edel-data: could not start deleting {}: {err}",
            trash.display()
        );
    }
}

/// Erases the data partition mounted at `root` when a factory reset asked
/// for it, and deletes what an erase left, at every start. Whether it
/// erased. Nothing here stops the start: what could not be moved is said.
pub fn erase_if_asked(root: &Path) -> bool {
    let asked = root.join(relative(places::FACTORY_RESET)).exists();
    if asked {
        println!(
            "edel-data: {}",
            tr("erasing this machine's accounts, files and settings, as a factory reset asked")
        );
        let done = erase(root);
        for line in &done.failed {
            eprintln!(
                "warning: edel-data: {}",
                trf(
                    "the factory reset could not take out {what}",
                    &[("what", line)]
                )
            );
        }
        println!(
            "edel-data: {}",
            trf(
                "the factory reset is done: {count} entries taken out, the default apps kept; this start is a first start",
                &[("count", &done.moved.to_string())]
            )
        );
    }
    delete_trash(root);
    asked
}

/// `edel factory-reset`: asks for the machine's name unless `yes`, then
/// leaves the request and restarts, so the next start erases.
pub fn factory_reset(yes: bool) -> Result<()> {
    if edel::install::from_a_stick() {
        bail!(
            "{}",
            tr(
                "this system runs from a stick, which keeps nothing to reset; to erase a disk completely, install Edel OS on it with edel install"
            )
        );
    }
    println!(
        "{}",
        tr(
            "A factory reset erases every account, every person's files, this machine's settings, developer mode's changes and the add-ons in use, and restarts. The default apps and the version of Edel OS stay, so the machine starts as new, offline too, and can still roll back."
        )
    );
    if let Some(keeps) = edel::security::Data::now().after_reset() {
        println!("{keeps}");
    }
    if !yes {
        let name = edel::install::machine_name();
        if name.is_empty() {
            bail!(
                "{}",
                tr("this machine has no name in /etc/hostname to confirm with; nothing was erased")
            );
        }
        print!(
            "{}: ",
            trf(
                "To erase everything, type this machine's name, {name}",
                &[("name", &name)]
            )
        );
        io::stdout().flush()?;
        let mut typed = String::new();
        io::stdin().lock().read_line(&mut typed)?;
        if !edel::install::is_machine_name(&typed, &name) {
            bail!(
                "{}",
                trf(
                    "that is not this machine's name, {name}; nothing was erased",
                    &[("name", &name)]
                )
            );
        }
    }
    let request = Path::new(places::FACTORY_RESET);
    if let Some(dir) = request.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(request, "").with_context(|| format!("cannot write {}", request.display()))?;
    sync();
    println!(
        "{}",
        tr("The machine restarts now and erases everything as it starts.")
    );
    let restarted = system_power::run(system_power::Action::Restart);
    if restarted.is_err() {
        // No erase at some later start nobody expects.
        let _ = fs::remove_file(request);
    }
    restarted
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("edel-reset-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "x").unwrap();
    }

    #[test]
    fn erases_all_but_the_default_apps_the_add_ons_and_the_install_mark() {
        let root = &scratch("erase");
        let keep = [
            "edel/apps/repo/config",
            "addons/2026.11.0/fonts/feature.toml",
            "edel/installed",
        ];
        let gone = [
            "home/ali/notes.txt",
            "etc/upper/shadow",
            "var/log/messages",
            "developer/usr/bin/tool",
            "edel/device.key",
            "edel/last-check.toml",
            "edel/apps-elsewhere",
            "lost+found/#1234",
            "addons/2026.11.0/failed",
        ];
        for path in keep.iter().chain(&gone) {
            touch(&root.join(path));
        }
        // A link out of the partition is moved, never followed.
        let outside = scratch("outside");
        touch(&outside.join("precious"));
        std::os::unix::fs::symlink(&outside, root.join("home/link")).unwrap();
        let request = root.join(relative(places::FACTORY_RESET));
        touch(&request);

        let done = erase(root);
        assert!(done.failed.is_empty(), "{:?}", done.failed);
        for path in keep {
            assert!(root.join(path).exists(), "{path} was erased");
        }
        for path in gone {
            assert!(!root.join(path).exists(), "{path} was kept");
        }
        assert!(!root.join("home").exists());
        assert!(root.join(LOST_FOUND).is_dir());
        assert!(outside.join("precious").exists());
        assert!(!request.exists());
        // What went is in the trash, which the next erase leaves alone.
        let trash = root.join(relative(places::FACTORY_RESET_TRASH));
        assert!(trash.join("0").read_dir().unwrap().count() > 0);
        touch(&request);
        touch(&root.join("home/new/file"));
        let again = erase(root);
        assert_eq!(again.moved, 1);
        assert!(trash.join("0").exists() && trash.join("1").exists());
    }

    #[test]
    fn without_a_request_nothing_is_erased() {
        let root = &scratch("none");
        touch(&root.join("home/a/b"));
        assert!(!erase_if_asked(root));
        assert!(root.join("home/a/b").exists());
    }

    #[test]
    fn keeps_the_request_and_the_trash_while_it_moves() {
        let kept = kept();
        assert!(kept.contains(&relative(places::FACTORY_RESET)));
        assert!(kept.contains(&relative(places::FACTORY_RESET_TRASH)));
        assert!(!kept.contains(&PathBuf::from(LOST_FOUND)));
    }

    #[test]
    fn asks_for_the_machine_s_exact_name() {
        assert!(edel::install::is_machine_name("office-pc\n", "office-pc"));
        assert!(!edel::install::is_machine_name("office", "office-pc"));
        assert!(!edel::install::is_machine_name("Office-PC", "office-pc"));
        assert!(!edel::install::is_machine_name("", ""));
    }
}
