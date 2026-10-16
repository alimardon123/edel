//! Add-ons (M7.2a, docs/FEATURES.md): the rules of building, fetching and
//! mounting them. They are pure functions over paths and file stamps, so
//! the tests run without root, Alpine or a disk; `edel image build` and the
//! updater call them and do the copying, mounting and packing themselves.
//!
//! An add-on is a feature (`addon = true`) that a machine mounts beside the
//! slot's root, so a newer slot never has to ship it and an old one never
//! breaks it. What it installs lands in the folders below; anything it
//! would change in the slot itself is refused.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use edel::i18n::{tr, trf};
use edel::places;

use crate::release::{AddonEntry, Manifest, os_release_value, to_hex};
use crate::update::run;

/// The files of the account database an add-on may add lines to: its new
/// users and groups are merged into the slot's files, never replace them.
pub const ACCOUNT_FILES: [&str; 3] = ["etc/passwd", "etc/group", "etc/shadow"];

/// The folders at the root whose contents [`scan`] skips: the kernel's and
/// the run-time's, which no add-on ships.
const SKIPPED: [&str; 5] = ["proc", "sys", "dev", "run", "tmp"];

/// Folders whose every file is a cache a package trigger or apk rewrites
/// for the whole system.
const CACHE_DIRS: [&str; 4] = ["etc/apk", "lib/apk", "usr/lib/apk", "var/cache"];

/// Single files that are caches (and so left out of an add-on).
const CACHE_FILES: [&str; 5] = [
    "etc/shells",
    "var/log/apk.log",
    "etc/udev/hwdb.bin",
    "usr/lib/udev/hwdb.bin",
    "usr/share/info/dir",
];

/// Paths that may use `*` for one component, that are caches.
const CACHE_PATTERNS: [&str; 5] = [
    "usr/share/icons/*/icon-theme.cache",
    "usr/share/applications/mimeinfo.cache",
    "usr/share/glib-2.0/schemas/gschemas.compiled",
    "usr/lib/gdk-pixbuf-2.0/*/loaders.cache",
    "usr/lib/gio/modules/giomodule.cache",
];

/// Files the system rewrites for itself, which an add-on never ships.
const DISCARDED: [&str; 4] = ["etc/passwd-", "etc/group-", "etc/shadow-", "etc/machine-id"];

/// `place` without its leading slash, for a path relative to a root.
fn rel(place: &'static str) -> &'static str {
    place.trim_start_matches('/')
}

/// The folder an add-on's own `/etc` files are in, relative to a root.
pub fn etc_dir(name: &str) -> String {
    format!("{}/{name}", rel(places::ADDON_ETC_DIR))
}

/// The folder an add-on's own `/var` files are in, relative to a root.
pub fn var_dir(name: &str) -> String {
    format!("{}/{name}", rel(places::ADDON_VAR_DIR))
}

/// The folder an add-on's account lines are in, relative to a root.
pub fn accounts_dir(name: &str) -> String {
    format!("{}/{name}", rel(places::ADDON_ACCOUNTS_DIR))
}

/// Where a file of an add-on goes in the slot's layout, for a path relative
/// to the add-on's root: `etc/init.d/X` (one component) is run at boot, any
/// other `etc/...` is kept in the add-on's own `/etc` folder, `var/...` in
/// its own `/var` folder, and `usr/...` stays where it is. Anything else,
/// and the runlevels (which `apply` owns), gives `None`.
pub fn relocate(path: &str, name: &str) -> Option<String> {
    if let Some(rest) = path.strip_prefix("etc/") {
        if let Some(file) = rest.strip_prefix("init.d/") {
            let one = !file.is_empty() && !file.contains('/');
            return one.then(|| format!("{}/{file}", rel(places::ADDON_INIT_DIR)));
        }
        if rest.is_empty() || rest == "runlevels" || rest.starts_with("runlevels/") {
            return None;
        }
        return Some(format!("{}/{rest}", etc_dir(name)));
    }
    if let Some(rest) = path.strip_prefix("var/") {
        return (!rest.is_empty()).then(|| format!("{}/{rest}", var_dir(name)));
    }
    if let Some(rest) = path.strip_prefix("usr/") {
        return (!rest.is_empty()).then(|| path.to_string());
    }
    // Programs and libraries Alpine still puts at the root go under /usr,
    // which is on every search path; the kernel's modules and firmware
    // never come from an add-on.
    for top in ["bin/", "sbin/", "lib/"] {
        if let Some(rest) = path.strip_prefix(top) {
            let kernel = ["modules", "firmware"]
                .iter()
                .any(|dir| rest == *dir || rest.starts_with(&format!("{dir}/")));
            return (!rest.is_empty() && !kernel).then(|| format!("usr/{path}"));
        }
    }
    None
}

/// Whether `path` (relative to a root) is a cache the system rewrites for
/// itself. A cache is left out of an add-on, so the slot's own is used.
pub fn is_system_cache(path: &str) -> bool {
    let under = |dir: &str| {
        path.strip_prefix(dir)
            .is_some_and(|rest| rest.starts_with('/'))
    };
    let font = rel(places::FONT_CACHE_DIR);
    CACHE_DIRS.iter().any(|dir| under(dir))
        || path == font
        || under(font)
        || (under("usr/share/mime") && !under("usr/share/mime/packages"))
        || (path.starts_with("usr/lib/") && path.ends_with("/immodules.cache"))
        || CACHE_FILES.contains(&path)
        || CACHE_PATTERNS
            .iter()
            .any(|pattern| glob_matches(path, pattern))
}

/// Whether `pattern` matches `path`, where `*` stands for one component.
fn glob_matches(path: &str, pattern: &str) -> bool {
    let want: Vec<&str> = pattern.split('/').collect();
    let got: Vec<&str> = path.split('/').collect();
    want.len() == got.len()
        && want
            .iter()
            .zip(got.iter())
            .all(|(w, g)| *w == "*" || w == g)
}

/// Whether the system writes `path` for itself (a copy of the account
/// database, a machine's id), so an add-on never ships it.
pub fn is_discarded(path: &str) -> bool {
    DISCARDED.contains(&path)
}

/// What a file of a root looks like, without following links: enough to see
/// that it changed, not its contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stamp {
    /// Whether it is a folder.
    pub dir: bool,
    /// Its size in bytes (for a link, the length of its target).
    pub size: u64,
    /// Its modification time, in seconds.
    pub mtime: i64,
    /// Its mode, with the file type bits.
    pub mode: u32,
    /// Where a link points, as written.
    pub link: Option<String>,
}

/// Every entry under `root`, by its path relative to it (no leading slash).
/// Links are not followed. The folders `proc`, `sys`, `dev`, `run` and
/// `tmp` at the root are entries, but their contents are not.
pub fn scan(root: &Path) -> Result<BTreeMap<String, Stamp>> {
    let mut map = BTreeMap::new();
    scan_into(root, "", &mut map)?;
    Ok(map)
}

fn scan_into(root: &Path, under: &str, map: &mut BTreeMap<String, Stamp>) -> Result<()> {
    let dir = if under.is_empty() {
        root.to_path_buf()
    } else {
        root.join(under)
    };
    let entries = fs::read_dir(&dir).with_context(|| format!("cannot read {}", dir.display()))?;
    for entry in entries {
        let entry = entry.with_context(|| format!("cannot read {}", dir.display()))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let key = if under.is_empty() {
            name.clone()
        } else {
            format!("{under}/{name}")
        };
        let meta = fs::symlink_metadata(&path)
            .with_context(|| format!("cannot read {}", path.display()))?;
        let link = if meta.file_type().is_symlink() {
            let target = fs::read_link(&path)
                .with_context(|| format!("cannot read the link {}", path.display()))?;
            Some(target.to_string_lossy().into_owned())
        } else {
            None
        };
        let is_dir = meta.is_dir();
        map.insert(
            key.clone(),
            Stamp {
                dir: is_dir,
                size: meta.len(),
                mtime: meta.mtime(),
                mode: meta.mode(),
                link,
            },
        );
        if is_dir && !(under.is_empty() && SKIPPED.contains(&name.as_str())) {
            scan_into(root, &key, map)?;
        }
    }
    Ok(())
}

/// What changed between two scans, each list sorted.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Diff {
    /// Paths that are new, folders included.
    pub new: Vec<String>,
    /// Files whose stamp differs. A folder never counts as changed.
    pub changed: Vec<String>,
    /// Paths that are gone.
    pub removed: Vec<String>,
}

/// Whether a file is unchanged: a link by its target alone, as copying a
/// root gives every link a new time; anything else by its whole stamp.
fn same(was: &Stamp, now: &Stamp) -> bool {
    match (&was.link, &now.link) {
        (Some(a), Some(b)) => a == b,
        _ => was == now,
    }
}

/// The difference between two scans.
pub fn diff(before: &BTreeMap<String, Stamp>, after: &BTreeMap<String, Stamp>) -> Diff {
    let mut out = Diff::default();
    for (path, now) in after {
        match before.get(path) {
            None => out.new.push(path.clone()),
            Some(was) => {
                if !now.dir && !same(was, now) {
                    out.changed.push(path.clone());
                }
            }
        }
    }
    for path in before.keys() {
        if !after.contains_key(path) {
            out.removed.push(path.clone());
        }
    }
    out
}

/// One path an add-on copies into the slot's layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Copy {
    /// The path in the add-on's root.
    pub from: String,
    /// The path in the add-on's tree, relative to the root.
    pub to: String,
    /// For a link, the target to write (already absolute where it was
    /// rewritten); none for a file or a folder.
    pub link: Option<String>,
}

/// What an add-on's tree holds and what it does to the account database.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct TreePlan {
    /// The paths to copy into the tree.
    pub copies: Vec<Copy>,
    /// The account files it adds lines to, to be merged at boot.
    pub accounts: Vec<String>,
    /// The caches it changed or made, which are left out.
    pub caches: Vec<String>,
}

/// Sorts the changes of an add-on's root into what its tree keeps. Every
/// refusal is gathered and reported together, so one build names them all.
pub fn plan_tree(diff: &Diff, after: &BTreeMap<String, Stamp>, name: &str) -> Result<TreePlan> {
    let mut plan = TreePlan::default();
    let mut problems = Vec::new();
    for path in &diff.new {
        let Some(stamp) = after.get(path) else {
            continue;
        };
        if is_discarded(path) {
            continue;
        }
        if is_system_cache(path) {
            if !stamp.dir {
                plan.caches.push(path.clone());
            }
            continue;
        }
        match relocate(path, name) {
            Some(to) => plan.copies.push(copy_of(path, to, stamp)),
            None => problems.push(format!("{path} is outside /usr, /etc and /var")),
        }
    }
    for path in &diff.changed {
        if is_discarded(path) {
            continue;
        }
        if ACCOUNT_FILES.contains(&path.as_str()) {
            plan.accounts.push(path.clone());
        } else if is_system_cache(path) {
            plan.caches.push(path.clone());
        } else {
            problems.push(format!("{path} is a file of the image the add-on changes"));
        }
    }
    for path in &diff.removed {
        if !is_system_cache(path) {
            problems.push(format!(
                "{path} is a file of the image the add-on takes out"
            ));
        }
    }
    if !problems.is_empty() {
        bail!(
            "add-on {name} cannot be built: {}; an add-on adds files under /usr, /etc and /var only",
            problems.join("; ")
        );
    }
    Ok(plan)
}

/// The copy of one new path: a relocated link with a relative target gets
/// the absolute target it had, resolved from its original folder.
fn copy_of(path: &str, to: String, stamp: &Stamp) -> Copy {
    let link = stamp.link.as_ref().map(|target| {
        if to.as_str() != path && !target.starts_with('/') {
            absolute(path, target)
        } else {
            target.clone()
        }
    });
    Copy {
        from: path.to_string(),
        to,
        link,
    }
}

/// `target`, a relative link at `link`, as the absolute path it names.
fn absolute(link: &str, target: &str) -> String {
    let parent = link.rsplit_once('/').map_or("", |(parent, _)| parent);
    let mut parts: Vec<&str> = Vec::new();
    for part in parent.split('/').chain(target.split('/')) {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    format!("/{}", parts.join("/"))
}

/// The account lines of `after` that `before` does not have, each by its
/// first field (the name), in order, each ending in a newline.
pub fn new_accounts(before: &str, after: &str) -> String {
    let known: std::collections::BTreeSet<&str> = before
        .lines()
        .map(|line| line.split(':').next().unwrap_or_default())
        .collect();
    let mut out = String::new();
    for line in after.lines().filter(|line| !line.is_empty()) {
        let name = line.split(':').next().unwrap_or_default();
        if !known.contains(name) {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// The users, groups, user ids and group ids that two add-ons both add in
/// different lines. Each input is an add-on's name and its new `passwd` or
/// `group` lines; a `passwd` line has seven fields, a `group` line four.
/// The same line from both is no clash: a package two add-ons share adds
/// it the same way in each, and the boot adds it once.
pub fn account_clashes(sets: &[(String, String)]) -> Vec<String> {
    let mut first: BTreeMap<(&str, &str), (&str, &str)> = BTreeMap::new();
    let mut owners: BTreeMap<(&str, &str), Vec<&str>> = BTreeMap::new();
    for (name, text) in sets {
        for line in text.lines() {
            let fields: Vec<&str> = line.split(':').collect();
            let (name_kind, id_kind) = match fields.len() {
                7 => ("user", "uid"),
                4 => ("group", "gid"),
                _ => continue,
            };
            for (kind, value) in [(name_kind, fields[0]), (id_kind, fields[2])] {
                let (owner, same) = *first.entry((kind, value)).or_insert((name.as_str(), line));
                if owner != name.as_str() && same != line {
                    let list = owners.entry((kind, value)).or_insert_with(|| vec![owner]);
                    if !list.contains(&name.as_str()) {
                        list.push(name.as_str());
                    }
                }
            }
        }
    }
    owners
        .into_iter()
        .filter(|(_, list)| list.len() >= 2)
        .map(|((kind, value), list)| {
            format!("{kind} {value} is used by {} and {}", list[0], list[1])
        })
        .collect()
}

/// The paths two or more add-on trees both ship unalike, as `"PATH is
/// shipped by A and B"`, sorted by path. Each input is an add-on's name and
/// its paths; `same(path, a, b)` says whether add-ons `a` and `b` ship the
/// same file there, as two add-ons needing one package do, which is no
/// clash: the overlay shows one of the two, and either serves.
pub fn collisions(
    trees: &[(String, Vec<String>)],
    same: &dyn Fn(&str, &str, &str) -> bool,
) -> Vec<String> {
    let mut owners: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (name, paths) in trees {
        for path in paths {
            let list = owners.entry(path.as_str()).or_default();
            if !list.contains(&name.as_str()) {
                list.push(name.as_str());
            }
        }
    }
    owners
        .into_iter()
        .filter_map(|(path, list)| {
            let other = list[1..].iter().find(|b| !same(path, list[0], b))?;
            Some(format!("{path} is shipped by {} and {other}", list[0]))
        })
        .collect()
}

/// The `lowerdir` of the slot's overlay for `/usr`: each mounted add-on's
/// `usr` folder in the order given, then the slot's own `usr` last, so the
/// lowest layer is the slot's.
pub fn lowerdirs(mounts: &[PathBuf], usr: &Path) -> String {
    let mut parts: Vec<String> = mounts
        .iter()
        .map(|mount| mount.join("usr").to_string_lossy().into_owned())
        .collect();
    parts.push(usr.to_string_lossy().into_owned());
    parts.join(":")
}

/// The names of the installed slot versions that are not kept, sorted.
pub fn stale_versions(present: &[String], keep: &[&str]) -> Vec<String> {
    let mut stale: Vec<String> = present
        .iter()
        .filter(|name| !keep.contains(&name.as_str()))
        .cloned()
        .collect();
    stale.sort();
    stale
}

// ---- On a machine (M7.2a): fetching, mounting and linking. ----

/// The add-ons the machine's settings file names (`addons.installed`),
/// read leniently: none when the file is missing or unreadable.
pub fn wanted_on_machine() -> Vec<String> {
    let path = places::found(&places::machine_settings());
    edel::settings::read_on_machine(&path)
        .ok()
        .and_then(|read| read.file.addons.installed)
        .unwrap_or_default()
}

/// Where the add-ons of `version` are kept on the data partition.
pub fn version_dir(version: &str) -> PathBuf {
    Path::new(places::ADDONS_DIR).join(version)
}

/// Which of the `wanted` add-ons the release `version` has for this image
/// (`entries`), and a note for each it has not: built into the new slot
/// (`built_in` asks it), or not in the release at all.
pub fn fetch_plan<'a>(
    wanted: &[String],
    entries: &[&'a AddonEntry],
    version: &str,
    built_in: &dyn Fn(&str) -> bool,
) -> (Vec<&'a AddonEntry>, Vec<String>) {
    let mut fetch = Vec::new();
    let mut notes = Vec::new();
    for name in wanted {
        if let Some(entry) = entries.iter().find(|e| &e.name == name) {
            fetch.push(*entry);
        } else if built_in(name) {
            notes.push(trf(
                "add-on {name} is built into {version}, so it needs no download",
                &[("name", name), ("version", version)],
            ));
        } else {
            notes.push(trf(
                "add-on {name} is not in {version}; the update goes on without it, and edel addon list shows the add-ons there are",
                &[("name", name), ("version", version)],
            ));
        }
    }
    (fetch, notes)
}

/// Fetches the add-ons the machine names for `manifest`'s release into
/// `/data/addons/VERSION/`, each checked by size and sha256 as the slot
/// is, after the slot on `slot` is written and checked and before it is
/// switched on, so a failure leaves it off. Then only the folders of the
/// running and the new version are kept.
pub fn fetch(location: &str, manifest: &Manifest, allow_http: bool, slot: &Path) -> Result<()> {
    let os_release = fs::read_to_string("/usr/lib/os-release").unwrap_or_default();
    let image = os_release_value(&os_release, "EDEL_IMAGE").unwrap_or_default();
    let running = os_release_value(&os_release, "VERSION_ID").unwrap_or_default();
    let entries = crate::release::addons_for(manifest, &image);
    let built_in = |name: &str| built_into(slot, name);
    let (to_fetch, notes) =
        fetch_plan(&wanted_on_machine(), &entries, &manifest.version, &built_in);
    for note in notes {
        println!("edel update: {note}");
    }
    let dir = version_dir(&manifest.version);
    if !to_fetch.is_empty() {
        fs::create_dir_all(&dir).with_context(|| {
            trf(
                "could not make {path}",
                &[("path", &dir.display().to_string())],
            )
        })?;
    }
    for entry in to_fetch {
        fetch_one(location, entry, allow_http, &dir)?;
    }
    prune(&[&running, &manifest.version]);
    Ok(())
}

/// Whether the slot on `device` carries feature `name`, read with
/// debugfs, so the slot is never mounted.
fn built_into(device: &Path, name: &str) -> bool {
    let file = format!("{}/{name}.toml", edel::features::DIR);
    Command::new("debugfs")
        .args(["-R", &format!("cat {file}")])
        .arg(device)
        .stderr(Stdio::null())
        .output()
        .is_ok_and(|out| out.status.success() && !out.stdout.is_empty())
}

/// Fetches one add-on to `dir/NAME.ext4` through a `.part` file renamed
/// only once it is checked; one already there with the right sha256 is
/// kept.
fn fetch_one(location: &str, entry: &AddonEntry, allow_http: bool, dir: &Path) -> Result<()> {
    let dest = dir.join(format!("{}.ext4", entry.name));
    let here = fs::File::open(&dest)
        .ok()
        .and_then(|mut file| crate::update::copy_hashing(&mut file, &mut std::io::sink()).ok());
    if here.is_some_and(|(size, sum)| size == entry.size && to_hex(&sum) == entry.sha256) {
        println!(
            "edel update: {}",
            trf("add-on {name} is already here", &[("name", &entry.name)])
        );
        return Ok(());
    }
    let part = dir.join(format!("{}.ext4.part", entry.name));
    let failed = || {
        trf(
            "could not fetch add-on {name}; the new slot stays off and this system is unchanged; check the network and run edel update again",
            &[("name", &entry.name)],
        )
    };
    let mut source =
        crate::release::open_addon(location, entry, allow_http).with_context(failed)?;
    let mut file = fs::File::create(&part).with_context(failed)?;
    let (size, sum) = crate::update::copy_hashing(&mut source, &mut file).with_context(failed)?;
    file.sync_all().with_context(failed)?;
    drop(file);
    if size != entry.size || to_hex(&sum) != entry.sha256 {
        let _ = fs::remove_file(&part);
        bail!(
            "{}",
            trf(
                "refused: add-on {name} does not match the sha256 in release.toml, so it is damaged or was changed on the way; the new slot stays off and this system is unchanged; run edel update again",
                &[("name", &entry.name)],
            )
        );
    }
    fs::rename(&part, &dest).with_context(failed)?;
    println!(
        "edel update: {}",
        trf(
            "add-on {name} fetched and checked",
            &[("name", &entry.name)]
        )
    );
    Ok(())
}

/// Takes out the add-on folders of versions in neither slot.
fn prune(keep: &[&str]) {
    let Ok(entries) = fs::read_dir(places::ADDONS_DIR) else {
        return;
    };
    let present: Vec<String> = entries
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    for old in stale_versions(&present, keep) {
        let path = Path::new(places::ADDONS_DIR).join(&old);
        if let Err(err) = fs::remove_dir_all(&path) {
            eprintln!(
                "warning: {}",
                trf(
                    "could not take out {path}: {why}",
                    &[
                        ("path", &path.display().to_string()),
                        ("why", &err.to_string())
                    ]
                )
            );
        }
    }
}

/// `edel boot mount-data`, first: mounts each add-on the settings file
/// names that this slot's version has on `/data`, read-only at
/// `/run/edel/addons/NAME`, and lays their `/usr` over the slot's, so the
/// rest of the boot sees them. A chosen add-on that is not here is said,
/// never fatal: it arrives with the next update.
pub fn mount_all() -> Result<()> {
    let os_release = fs::read_to_string("/usr/lib/os-release").unwrap_or_default();
    let version = os_release_value(&os_release, "VERSION_ID").unwrap_or_default();
    let dir = version_dir(&version);
    let mut mounts = Vec::new();
    for name in wanted_on_machine() {
        let file = dir.join(format!("{name}.ext4"));
        if !file.is_file() {
            if !Path::new(edel::features::DIR)
                .join(format!("{name}.toml"))
                .exists()
            {
                println!(
                    "edel-data: {}",
                    trf(
                        "add-on {name} is chosen but not here yet; it arrives with the next update",
                        &[("name", &name)]
                    )
                );
            }
            continue;
        }
        let at = Path::new(places::ADDONS_RUN).join(&name);
        fs::create_dir_all(&at)?;
        if !crate::data::is_mounted(&at.to_string_lossy())? {
            let _ = Command::new("modprobe").arg("loop").status();
            let mounted = run(Command::new("mount")
                .args(["-t", "ext4", "-o", "ro,loop,nodev"])
                .arg(&file)
                .arg(&at))
            .with_context(|| trf("could not mount add-on {name}", &[("name", &name)]));
            // One add-on that does not mount never keeps the others away.
            if let Err(err) = mounted {
                eprintln!("edel-data: {err:#}");
                continue;
            }
        }
        mounts.push(at);
    }
    if mounts.is_empty() || crate::data::is_mounted("/usr")? {
        return Ok(());
    }
    run(Command::new("mount")
        .args(["-t", "overlay", "overlay", "-o"])
        .arg(format!(
            "ro,lowerdir={}",
            lowerdirs(&mounts, Path::new("/usr"))
        ))
        .arg("/usr"))
    .context(tr("could not lay the add-ons over /usr"))?;
    let names: Vec<String> = mounts
        .iter()
        .filter_map(|m| m.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect();
    println!(
        "edel-data: {}",
        trf("add-ons mounted: {names}", &[("names", &names.join(", "))])
    );
    Ok(())
}

/// The add-ons mounted now, by name, sorted.
pub fn mounted() -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(places::ADDONS_RUN)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| crate::data::is_mounted(&e.path().to_string_lossy()).unwrap_or(false))
                .filter_map(|e| e.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// The account lines the mounted add-ons add to `/etc/FILE` (`passwd`,
/// `group` or `shadow`), in name order.
pub fn account_lines(file: &str) -> String {
    let dir = Path::new(places::ADDON_ACCOUNTS_DIR);
    let mut names: Vec<PathBuf> = fs::read_dir(dir)
        .map(|entries| entries.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    names.sort();
    let texts: Vec<String> = names
        .iter()
        .filter_map(|d| fs::read_to_string(d.join(file)).ok())
        .collect();
    first_of_each(&texts)
}

/// The lines of `texts`, each name (the first field) once, in order: two
/// add-ons sharing a package add its user in the same line.
fn first_of_each(texts: &[String]) -> String {
    let mut seen = BTreeSet::new();
    let mut out = String::new();
    for line in texts.iter().flat_map(|t| t.lines()) {
        if !line.is_empty() && seen.insert(line.split(':').next().unwrap_or_default()) {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// The folders of `/var` the mounted add-ons bring, one per add-on.
pub fn var_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = fs::read_dir(places::ADDON_VAR_DIR)
        .map(|entries| entries.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    dirs.sort();
    dirs
}

/// The links in `/etc` that make the mounted add-ons' files show there:
/// what they provide, the links apply made before, and the paths this
/// machine has a file of its own at, which are never replaced.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Links {
    /// (path under `/etc`, target) for each file an add-on provides.
    pub provided: Vec<(String, String)>,
    /// (path under `/etc`, target) for each link into the add-ons' folders.
    pub ours: Vec<(String, String)>,
    /// Paths under `/etc` that hold something other than a link.
    pub theirs: BTreeSet<String>,
}

impl Links {
    /// Reads what the mounted add-ons provide and what `/etc` has now.
    pub fn read() -> Links {
        let mut provided = Vec::new();
        if let Ok(entries) = fs::read_dir(places::ADDON_INIT_DIR) {
            let mut names: Vec<String> = entries
                .flatten()
                .filter_map(|e| e.file_name().into_string().ok())
                .collect();
            names.sort();
            for name in names {
                provided.push((
                    format!("init.d/{name}"),
                    format!("{}/{name}", places::ADDON_INIT_DIR),
                ));
            }
        }
        let mut addons: Vec<PathBuf> = fs::read_dir(places::ADDON_ETC_DIR)
            .map(|entries| entries.flatten().map(|e| e.path()).collect())
            .unwrap_or_default();
        addons.sort();
        for dir in addons {
            let mut files = Vec::new();
            walk(&dir, "", &mut files, &|_, meta| !meta.is_dir());
            for rest in files {
                provided.push((rest.clone(), dir.join(&rest).to_string_lossy().into_owned()));
            }
        }
        let mut ours = Vec::new();
        let mut links = Vec::new();
        walk(Path::new("/etc"), "", &mut links, &|_, meta| {
            meta.file_type().is_symlink()
        });
        for rest in links {
            let Ok(target) = fs::read_link(Path::new("/etc").join(&rest)) else {
                continue;
            };
            let target = target.to_string_lossy().into_owned();
            if is_ours(&target) {
                ours.push((rest, target));
            }
        }
        let theirs = provided
            .iter()
            .map(|(rest, _)| rest)
            .filter(|rest| {
                Path::new("/etc")
                    .join(rest)
                    .symlink_metadata()
                    .is_ok_and(|m| !m.file_type().is_symlink())
            })
            .cloned()
            .collect();
        Links {
            provided,
            ours,
            theirs,
        }
    }

    /// The links to make, (path under `/etc`, target), and the paths of
    /// links to take out: the first add-on to provide a path has it, and a
    /// machine's own file is never replaced.
    pub fn plan(&self) -> (Vec<(String, String)>, Vec<String>) {
        let mut wanted: BTreeMap<&str, &str> = BTreeMap::new();
        for (rest, target) in &self.provided {
            if !self.theirs.contains(rest) {
                wanted.entry(rest).or_insert(target);
            }
        }
        let make = wanted
            .iter()
            .filter(|(rest, target)| !self.ours.iter().any(|(r, t)| r == *rest && t == *target))
            .map(|(rest, target)| (rest.to_string(), target.to_string()))
            .collect();
        let remove = self
            .ours
            .iter()
            .filter(|(rest, target)| wanted.get(rest.as_str()) != Some(&target.as_str()))
            .map(|(rest, _)| rest.clone())
            .collect();
        (make, remove)
    }
}

/// One line of `edel addon list`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub name: String,
    pub summary: String,
    /// `installed`, `built in`, `chosen` (it arrives with the next
    /// update) or empty.
    pub status: String,
}

/// The rows of `edel addon list`: the release's add-ons for this image
/// (`offered`, name and summary), then any chosen or mounted one it lacks.
pub fn rows(
    offered: &[(String, String)],
    mounted: &[String],
    built_in: &BTreeSet<String>,
    wanted: &[String],
) -> Vec<Row> {
    let status = |name: &str| {
        if mounted.iter().any(|m| m == name) {
            tr("installed").to_string()
        } else if built_in.contains(name) {
            tr("built in").to_string()
        } else if wanted.iter().any(|w| w == name) {
            tr("chosen").to_string()
        } else {
            String::new()
        }
    };
    let mut rows: Vec<Row> = offered
        .iter()
        .map(|(name, summary)| Row {
            name: name.clone(),
            summary: summary.clone(),
            status: status(name),
        })
        .collect();
    for name in mounted.iter().chain(wanted) {
        if !rows.iter().any(|r| &r.name == name) {
            rows.push(Row {
                name: name.clone(),
                summary: String::new(),
                status: status(name),
            });
        }
    }
    rows
}

/// `edel addon list`: the add-ons of this machine's channel for its image,
/// each marked installed, built in or chosen. Offline, it says so and
/// lists what this machine has.
pub fn list() -> Result<()> {
    let accept = crate::release::Accept::default();
    let os_release = fs::read_to_string("/usr/lib/os-release").unwrap_or_default();
    let image = os_release_value(&os_release, "EDEL_IMAGE").unwrap_or_default();
    let offered: Vec<(String, String)> = match crate::release::default_location(&accept)
        .and_then(|at| crate::release::verified(&at, &accept))
    {
        Ok(manifest) => crate::release::addons_for(&manifest, &image)
            .into_iter()
            .map(|a| (a.name.clone(), a.summary.clone()))
            .collect(),
        Err(err) => {
            eprintln!(
                "warning: {}",
                trf(
                    "could not read the release list ({why}), so only this machine's add-ons are shown",
                    &[("why", &format!("{err:#}"))]
                )
            );
            Vec::new()
        }
    };
    let mounted = mounted();
    let built_in: BTreeSet<String> = edel::services::installed()
        .into_iter()
        .filter(|(name, f)| f.addon && !mounted.contains(name))
        .map(|(name, _)| name)
        .collect();
    let rows = rows(&offered, &mounted, &built_in, &wanted_on_machine());
    if rows.is_empty() {
        println!("{}", tr("no add-ons for this image"));
        return Ok(());
    }
    for row in &rows {
        let line = format!("{:<16} {:<10} {}", row.name, row.status, row.summary);
        println!("{}", line.trim_end());
    }
    println!(
        "{}",
        tr("to add one: edel settings set addons.installed+=NAME; it arrives with the next update")
    );
    Ok(())
}

/// Whether a link's target is in the add-ons' folders, so apply made it.
fn is_ours(target: &str) -> bool {
    [places::ADDON_INIT_DIR, places::ADDON_ETC_DIR]
        .iter()
        .any(|dir| target.strip_prefix(dir).is_some_and(|r| r.starts_with('/')))
}

/// The paths under `dir`, relative to it, that `keep` picks, never
/// following a link to a folder.
fn walk(
    dir: &Path,
    under: &str,
    found: &mut Vec<String>,
    keep: &dyn Fn(&str, &fs::Metadata) -> bool,
) {
    let Ok(entries) = fs::read_dir(dir.join(under)) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let rest = if under.is_empty() {
            name
        } else {
            format!("{under}/{name}")
        };
        let Ok(meta) = entry.path().symlink_metadata() else {
            continue;
        };
        if keep(&rest, &meta) {
            found.push(rest.clone());
        }
        if meta.is_dir() {
            walk(dir, &rest, found, keep);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;
    use std::process;

    use super::*;

    fn file(size: u64) -> Stamp {
        Stamp {
            dir: false,
            size,
            mtime: 0,
            mode: 0o100644,
            link: None,
        }
    }

    fn dir(mtime: i64) -> Stamp {
        Stamp {
            dir: true,
            size: 0,
            mtime,
            mode: 0o040755,
            link: None,
        }
    }

    fn link(target: &str) -> Stamp {
        Stamp {
            dir: false,
            size: target.len() as u64,
            mtime: 0,
            mode: 0o120777,
            link: Some(target.to_string()),
        }
    }

    fn tree(entries: &[(&str, Stamp)]) -> BTreeMap<String, Stamp> {
        entries
            .iter()
            .map(|(path, stamp)| (path.to_string(), stamp.clone()))
            .collect()
    }

    #[test]
    fn a_link_copied_with_a_new_time_is_unchanged() {
        let later = Stamp {
            mtime: 9,
            ..link("busybox")
        };
        let before = tree(&[("bin/ls", link("busybox")), ("etc/x", file(1))]);
        let after = tree(&[
            ("bin/ls", later),
            (
                "etc/x",
                Stamp {
                    mtime: 9,
                    ..file(1)
                },
            ),
        ]);
        assert_eq!(diff(&before, &after).changed, ["etc/x"]);
        let moved = tree(&[("bin/ls", link("toybox")), ("etc/x", file(1))]);
        assert_eq!(diff(&before, &moved).changed, ["bin/ls"]);
    }

    #[test]
    fn relocate_puts_each_part_where_its_rule_says() {
        assert_eq!(
            relocate("etc/init.d/svc", "ci-hello").as_deref(),
            Some("usr/lib/edel/init.d/svc")
        );
        assert_eq!(relocate("etc/init.d/a/b", "ci-hello"), None);
        assert_eq!(
            relocate("etc/foo/bar", "ci-hello").as_deref(),
            Some("usr/lib/edel/etc/ci-hello/foo/bar")
        );
        assert_eq!(relocate("etc/runlevels/default/x", "ci-hello"), None);
        assert_eq!(relocate("etc", "ci-hello"), None);
        assert_eq!(
            relocate("var/lib/x", "ci-hello").as_deref(),
            Some("usr/lib/edel/var/ci-hello/lib/x")
        );
        assert_eq!(relocate("var", "ci-hello"), None);
        assert_eq!(
            relocate("usr/bin/tool", "ci-hello").as_deref(),
            Some("usr/bin/tool")
        );
        assert_eq!(relocate("usr", "ci-hello"), None);
        for (top, to) in [
            ("bin/x", "usr/bin/x"),
            ("sbin/x", "usr/sbin/x"),
            ("lib/libx.so", "usr/lib/libx.so"),
        ] {
            assert_eq!(relocate(top, "ci-hello").as_deref(), Some(to), "{top}");
        }
        for other in ["lib/modules/6.1/x.ko", "lib/firmware/x.bin", "opt/x", "bin"] {
            assert_eq!(relocate(other, "ci-hello"), None, "{other}");
        }
    }

    #[test]
    fn a_system_cache_is_recognised_by_its_path() {
        for cache in [
            "lib/apk/db/installed",
            "usr/lib/apk/db/x",
            "etc/apk/world",
            "var/cache/apk/index",
            "usr/share/icons/hicolor/icon-theme.cache",
            "usr/share/glib-2.0/schemas/gschemas.compiled",
            "usr/share/applications/mimeinfo.cache",
            "usr/share/mime/mime.cache",
            "usr/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache",
            "usr/lib/gio/modules/giomodule.cache",
            "usr/share/info/dir",
            "etc/shells",
            "etc/udev/hwdb.bin",
            "usr/lib/udev/hwdb.bin",
            "usr/lib/gtk-3.0/3.0.0/immodules.cache",
            "usr/lib/fontconfig/cache/a.cache-9",
        ] {
            assert!(is_system_cache(cache), "{cache}");
        }
        assert!(!is_system_cache("usr/share/mime/packages/x.xml"));
        assert!(!is_system_cache("usr/share/icons/hicolor/48x48/a.png"));
        assert!(!is_system_cache("usr/bin/tool"));
        assert!(!is_system_cache("etc/conf"));
    }

    #[test]
    fn diff_counts_new_paths_and_changed_files_only() {
        let before = tree(&[
            ("usr", dir(1)),
            ("etc/a", file(1)),
            ("etc/gone", file(1)),
            ("etc/l", link("a")),
        ]);
        let after = tree(&[
            ("usr", dir(2)),
            ("usr/new", file(1)),
            ("var", dir(1)),
            ("etc/a", file(2)),
            ("etc/l", link("b")),
        ]);
        assert_eq!(
            diff(&before, &after),
            Diff {
                new: vec!["usr/new".into(), "var".into()],
                changed: vec!["etc/a".into(), "etc/l".into()],
                removed: vec!["etc/gone".into()],
            }
        );
        // A folder whose only change is its time is not changed.
        let same = tree(&[("usr", dir(9))]);
        assert_eq!(diff(&before, &same).changed, Vec::<String>::new());
    }

    #[test]
    fn plan_tree_keeps_each_kind_of_change_in_its_place() {
        let before = tree(&[("etc/passwd", file(10)), ("usr/lib/libc.so", file(5))]);
        let after = tree(&[
            ("etc/conf", file(1)),
            ("etc/init.d/svc", file(1)),
            ("etc/passwd", file(11)),
            ("etc/passwd-", file(10)),
            ("usr/bin/tool", file(1)),
            ("usr/lib/libc.so", file(5)),
            ("var/cache/apk/index", file(1)),
            ("var/lib/x", file(1)),
        ]);
        let plan = plan_tree(&diff(&before, &after), &after, "ci-hello").unwrap();
        let copies: Vec<(&str, &str)> = plan
            .copies
            .iter()
            .map(|c| (c.from.as_str(), c.to.as_str()))
            .collect();
        assert_eq!(
            copies,
            vec![
                ("etc/conf", "usr/lib/edel/etc/ci-hello/conf"),
                ("etc/init.d/svc", "usr/lib/edel/init.d/svc"),
                ("usr/bin/tool", "usr/bin/tool"),
                ("var/lib/x", "usr/lib/edel/var/ci-hello/lib/x"),
            ]
        );
        assert_eq!(plan.accounts, vec!["etc/passwd".to_string()]);
        assert_eq!(plan.caches, vec!["var/cache/apk/index".to_string()]);
    }

    #[test]
    fn plan_tree_refuses_a_tree_and_names_every_refusal() {
        let before = tree(&[("usr/lib/libc.so", file(5)), ("usr/share/doc/x", file(1))]);
        let after = tree(&[("opt/x", file(1)), ("usr/lib/libc.so", file(6))]);
        let error = plan_tree(&diff(&before, &after), &after, "ci-hello")
            .unwrap_err()
            .to_string();
        assert!(
            error.starts_with("add-on ci-hello cannot be built: "),
            "{error}"
        );
        assert!(
            error.contains("opt/x is outside /usr, /etc and /var"),
            "{error}"
        );
        assert!(
            error.contains("usr/lib/libc.so is a file of the image the add-on changes"),
            "{error}"
        );
        assert!(
            error.contains("usr/share/doc/x is a file of the image the add-on takes out"),
            "{error}"
        );
    }

    #[test]
    fn plan_tree_rewrites_a_relocated_relative_link() {
        let after = tree(&[("etc/foo", dir(1)), ("etc/foo/a", link("../b"))]);
        let plan = plan_tree(&diff(&BTreeMap::new(), &after), &after, "ci-hello").unwrap();
        assert_eq!(plan.copies.len(), 2);
        assert_eq!(plan.copies[0].to, "usr/lib/edel/etc/ci-hello/foo");
        assert_eq!(plan.copies[0].link, None);
        assert_eq!(plan.copies[1].to, "usr/lib/edel/etc/ci-hello/foo/a");
        assert_eq!(plan.copies[1].link.as_deref(), Some("/etc/b"));
        assert_eq!(absolute("usr/bin/l", "../lib/x"), "/usr/lib/x");
    }

    #[test]
    fn new_accounts_keeps_only_names_the_image_does_not_have() {
        let before = "root:x:0:0:root:/root:/bin/sh\n";
        let after = "root:x:0:0:root:/root:/bin/sh\napp:x:1001:1001::/:/bin/sh\n";
        assert_eq!(new_accounts(before, after), "app:x:1001:1001::/:/bin/sh\n");
    }

    #[test]
    fn account_clashes_name_a_user_or_id_two_add_ons_use() {
        let sets = [
            (
                "a".to_string(),
                "app:x:1001:1001::/:/bin/sh\ngrp:x:2000:\n".to_string(),
            ),
            (
                "b".to_string(),
                "app:x:1002:1002::/:/bin/sh\nother:x:1001:1001::/:/bin/sh\n".to_string(),
            ),
        ];
        assert_eq!(
            account_clashes(&sets),
            vec![
                "uid 1001 is used by a and b".to_string(),
                "user app is used by a and b".to_string(),
            ]
        );
        assert!(account_clashes(&sets[..1]).is_empty());
        let shared = [
            ("a".to_string(), "kvm:x:34:\n".to_string()),
            ("b".to_string(), "kvm:x:34:\n".to_string()),
        ];
        assert!(account_clashes(&shared).is_empty());
        assert_eq!(
            first_of_each(&[shared[0].1.clone(), shared[1].1.clone()]),
            "kvm:x:34:\n"
        );
    }

    #[test]
    fn collisions_name_each_path_two_trees_ship() {
        let trees = [
            (
                "a".to_string(),
                vec!["usr/x".to_string(), "usr/y".to_string()],
            ),
            ("b".to_string(), vec!["usr/y".to_string()]),
            ("c".to_string(), vec!["usr/x".to_string()]),
        ];
        assert_eq!(
            collisions(&trees, &|_, _, _| false),
            vec![
                "usr/x is shipped by a and c".to_string(),
                "usr/y is shipped by a and b".to_string(),
            ]
        );
        let alike = |path: &str, _: &str, _: &str| path == "usr/y";
        assert_eq!(
            collisions(&trees, &alike),
            vec!["usr/x is shipped by a and c".to_string()]
        );
    }

    #[test]
    fn lowerdirs_stacks_the_add_ons_above_the_slot() {
        let mounts = [PathBuf::from("/run/edel/addons/a"), PathBuf::from("/x")];
        assert_eq!(
            lowerdirs(&mounts, Path::new("/slot/usr")),
            "/run/edel/addons/a/usr:/x/usr:/slot/usr"
        );
        assert_eq!(lowerdirs(&[], Path::new("/slot/usr")), "/slot/usr");
    }

    #[test]
    fn stale_versions_are_the_present_ones_not_kept() {
        let present = [
            "edel-2".to_string(),
            "edel-0".to_string(),
            "edel-1".to_string(),
        ];
        assert_eq!(
            stale_versions(&present, &["edel-2"]),
            vec!["edel-0".to_string(), "edel-1".to_string()]
        );
    }

    #[test]
    fn scan_lists_every_entry_but_not_the_inside_of_proc() {
        let root = std::env::temp_dir().join(format!("edel-addons-scan-{}", process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("d")).unwrap();
        fs::create_dir_all(root.join("proc")).unwrap();
        fs::write(root.join("file"), "x").unwrap();
        fs::write(root.join("d/inner"), "y").unwrap();
        fs::write(root.join("proc/x"), "z").unwrap();
        symlink("file", root.join("link")).unwrap();
        let map = scan(&root).unwrap();
        let keys: Vec<&str> = map.keys().map(String::as_str).collect();
        assert_eq!(keys, vec!["d", "d/inner", "file", "link", "proc"]);
        assert!(map["d"].dir && !map["file"].dir);
        assert_eq!(map["file"].size, 1);
        assert_eq!(map["link"].link.as_deref(), Some("file"));
        fs::remove_dir_all(root).unwrap();
    }

    fn entry(name: &str) -> AddonEntry {
        AddonEntry {
            name: name.into(),
            image: "edel-vm-x86_64".into(),
            base: "edel-vm-x86_64.ext4.gz".into(),
            summary: String::new(),
            file: format!("edel-vm-x86_64.{name}.ext4.gz"),
            sha256: "0".repeat(64),
            size: 1,
        }
    }

    #[test]
    fn fetches_the_wanted_add_ons_and_notes_the_rest() {
        let hello = entry("ci-hello");
        let entries = [&hello];
        let wanted = ["ci-hello", "ci-gone", "fonts-cjk"].map(String::from);
        let built_in = |name: &str| name == "fonts-cjk";
        let (fetch, notes) = fetch_plan(&wanted, &entries, "2026.10.1", &built_in);
        assert_eq!(fetch.len(), 1);
        assert_eq!(fetch[0].name, "ci-hello");
        assert_eq!(notes.len(), 2);
        assert!(
            notes[0].starts_with("add-on ci-gone is not in 2026.10.1"),
            "{}",
            notes[0]
        );
        assert!(
            notes[1].starts_with("add-on fonts-cjk is built into 2026.10.1"),
            "{}",
            notes[1]
        );
    }

    fn pair(a: &str, b: &str) -> (String, String) {
        (a.to_string(), b.to_string())
    }

    #[test]
    fn links_what_is_provided_and_takes_out_what_went() {
        let links = Links {
            provided: vec![
                pair("init.d/ci-hello", "/usr/lib/edel/init.d/ci-hello"),
                pair("ci-hello.conf", "/usr/lib/edel/etc/ci-hello/ci-hello.conf"),
                pair(
                    "cups/cupsd.conf",
                    "/usr/lib/edel/etc/printing/cups/cupsd.conf",
                ),
                pair("ci-hello.conf", "/usr/lib/edel/etc/other/ci-hello.conf"),
            ],
            ours: vec![
                pair("ci-hello.conf", "/usr/lib/edel/etc/ci-hello/ci-hello.conf"),
                pair("init.d/gone", "/usr/lib/edel/init.d/gone"),
            ],
            theirs: BTreeSet::from(["cups/cupsd.conf".to_string()]),
        };
        let (make, remove) = links.plan();
        assert_eq!(
            make,
            [pair("init.d/ci-hello", "/usr/lib/edel/init.d/ci-hello")]
        );
        assert_eq!(remove, ["init.d/gone"]);
        assert!(is_ours("/usr/lib/edel/init.d/x") && !is_ours("/usr/lib/edel/init.dx"));
        assert!(!is_ours("/usr/share/zoneinfo/UTC"));
    }

    #[test]
    fn lists_the_offered_and_the_chosen_add_ons() {
        let offered = [pair("fonts-cjk", "Fonts"), pair("printing", "Printers")];
        let rows = rows(
            &offered,
            &["printing".to_string()],
            &BTreeSet::new(),
            &["printing".to_string(), "ci-gone".to_string()],
        );
        let got: Vec<(&str, &str)> = rows
            .iter()
            .map(|r| (r.name.as_str(), r.status.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                ("fonts-cjk", ""),
                ("printing", "installed"),
                ("ci-gone", "chosen")
            ]
        );
    }
}
