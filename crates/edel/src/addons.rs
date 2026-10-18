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
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use edel::addon_state::{failed_in, names_in};
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

/// apk's log of an image build, a cache add-ons leave out and images drop (M8.6).
pub(crate) const APK_LOG: &str = "var/log/apk.log";

/// Single files that are caches (and so left out of an add-on).
const CACHE_FILES: [&str; 5] = [
    "etc/shells",
    APK_LOG,
    "etc/udev/hwdb.bin",
    "usr/lib/udev/hwdb.bin",
    "usr/share/info/dir",
];

/// Paths that may use `*` for one component, that are caches.
const CACHE_PATTERNS: [&str; 1] = ["usr/share/icons/*/icon-theme.cache"];

/// Paths that may use `*` for one component, that are merged caches: a
/// package trigger wrote them for the whole system and nothing reads their
/// sources, so an add-on keeps its copy (see [`is_merged_cache`]).
const MERGED_PATTERNS: [&str; 3] = [
    "usr/share/glib-2.0/schemas/gschemas.compiled",
    "usr/share/applications/mimeinfo.cache",
    "usr/lib/gdk-pixbuf-2.0/*/loaders.cache",
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

/// Whether `path` lies under the folder `dir`, both without a leading slash.
fn under(path: &str, dir: &str) -> bool {
    path.strip_prefix(dir)
        .is_some_and(|rest| rest.starts_with('/'))
}

/// Whether `path` (relative to a root) is a cache the system rewrites for
/// itself, or that apps check against their folder's time and rebuild: left
/// out of an add-on, so the slot's own is used.
pub fn is_system_cache(path: &str) -> bool {
    is_font_cache(path)
        || CACHE_DIRS.iter().any(|dir| under(path, dir))
        || CACHE_FILES.contains(&path)
        || CACHE_PATTERNS
            .iter()
            .any(|pattern| edel::keep::matches(pattern, path))
}

/// Whether `path` (relative to a root) is in the fonts' baked cache
/// (`places::FONT_CACHE_DIR`, M6.2a): a system cache, but kept, as a merged
/// one, by the add-on that brings the fonts and those over it (M8.17c).
pub fn is_font_cache(path: &str) -> bool {
    let font = rel(places::FONT_CACHE_DIR);
    path == font || under(path, font)
}

/// Whether `path` (relative to a root) is a cache a package trigger wrote
/// for the whole system that nothing reads the sources of (compiled GSettings
/// schemas, GIO, gdk-pixbuf and GTK module caches, the MIME and desktop
/// databases): an add-on keeps its copy, which holds the image's entries and
/// its own, and the overlay shows it over the slot's. Two add-ons that both
/// change one are refused as any two add-ons shipping one path unalike.
pub fn is_merged_cache(path: &str) -> bool {
    (under(path, "usr/share/mime") && !under(path, "usr/share/mime/packages"))
        || (path.starts_with("usr/lib/")
            && (path.ends_with("/immodules.cache") || path.ends_with("/giomodule.cache")))
        || MERGED_PATTERNS
            .iter()
            .any(|pattern| edel::keep::matches(pattern, path))
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
    /// Links to the base's small tools a package replaced, which stay the
    /// image's.
    pub kept_applets: Vec<String>,
}

/// Whether a file is unchanged: a link by its target alone, as copying a
/// root gives every link a new time; anything else by its whole stamp.
fn same(was: &Stamp, now: &Stamp) -> bool {
    match (&was.link, &now.link) {
        (Some(a), Some(b)) => a == b,
        _ => was == now,
    }
}

/// Whether an entry of the image is a link to the base's small tools'
/// program, which an add-on's package may replace with its own copy of
/// the tool (`bc` for printing): the add-on keeps the image's.
fn is_applet(stamp: &Stamp) -> bool {
    stamp
        .link
        .as_deref()
        .is_some_and(|target| target.rsplit('/').next() == Some(edel::base::APPLETS))
}

/// The difference between two scans. A link to the base's small tools that
/// a package replaced is not a change: the image's link stays.
pub fn diff(before: &BTreeMap<String, Stamp>, after: &BTreeMap<String, Stamp>) -> Diff {
    let mut out = Diff::default();
    for (path, now) in after {
        match before.get(path) {
            None => out.new.push(path.clone()),
            Some(was) => {
                if !now.dir && !same(was, now) {
                    if is_applet(was) {
                        out.kept_applets.push(path.clone());
                    } else {
                        out.changed.push(path.clone());
                    }
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
    /// The caches it keeps a merged copy of, laid over the slot's.
    pub merged: Vec<String>,
}

/// Sorts the changes of an add-on's root into what its tree keeps. Every
/// refusal is gathered and reported together, so one build names them all.
pub fn plan_tree(
    diff: &Diff,
    after: &BTreeMap<String, Stamp>,
    name: &str,
    font_cache: bool,
) -> Result<TreePlan> {
    let is_merged_cache = |path: &str| is_merged_cache(path) || (font_cache && is_font_cache(path));
    let mut plan = TreePlan::default();
    let mut problems = Vec::new();
    for path in &diff.new {
        let Some(stamp) = after.get(path) else {
            continue;
        };
        if is_discarded(path) {
            continue;
        }
        if is_merged_cache(path) {
            if !stamp.dir {
                plan.copies.push(copy_of(path, path.to_string(), stamp));
                plan.merged.push(path.clone());
            }
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
        } else if is_merged_cache(path) {
            if let Some(stamp) = after.get(path) {
                plan.copies.push(copy_of(path, path.to_string(), stamp));
                plan.merged.push(path.clone());
            }
        } else if is_system_cache(path) {
            plan.caches.push(path.clone());
        } else {
            problems.push(format!("{path} is a file of the image the add-on changes"));
        }
    }
    for path in &diff.removed {
        if !(is_system_cache(path) || is_merged_cache(path)) {
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

/// The names (the first field) of the account lines that both texts have
/// with a different line in `after`: a package that added a member to an
/// existing group, or changed an existing user's shell. `new_accounts` gives
/// the names only `after` has.
pub fn changed_accounts(before: &str, after: &str) -> Vec<String> {
    let known: BTreeMap<&str, &str> = before
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| (line.split(':').next().unwrap_or_default(), line))
        .collect();
    after
        .lines()
        .filter(|line| !line.is_empty())
        .filter(|line| {
            let name = line.split(':').next().unwrap_or_default();
            known.get(name).is_some_and(|was| was != line)
        })
        .map(|line| line.split(':').next().unwrap_or_default().to_string())
        .collect()
}

/// The file, beside an add-on's `group` lines, of the members it adds to
/// groups the image has (M7.2b), `GROUP:USER,USER` a line.
pub const MEMBERS_FILE: &str = "members";

/// The members of one `group` line, empty when it has none.
fn members_of(line: &str) -> Vec<&str> {
    line.split(':')
        .nth(3)
        .unwrap_or_default()
        .split(',')
        .filter(|m| !m.is_empty())
        .collect()
}

/// The members `after` adds to groups `before` has, as `GROUP:USER,USER`
/// lines (`group` files' text): a package that puts its user into a
/// group the image has, as QEMU's puts `qemu` into `kvm` (M7.2b). A
/// group whose name, password or id changed is left out, as is one that
/// lost a member; [`members_only`] tells them apart.
pub fn added_members(before: &str, after: &str) -> String {
    let known: BTreeMap<&str, &str> = before
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| (line.split(':').next().unwrap_or_default(), line))
        .collect();
    let mut out = String::new();
    for line in after.lines().filter(|line| !line.is_empty()) {
        let name = line.split(':').next().unwrap_or_default();
        let Some(was) = known.get(name) else {
            continue;
        };
        if !members_only(was, line) {
            continue;
        }
        let had = members_of(was);
        let new: Vec<&str> = members_of(line)
            .into_iter()
            .filter(|m| !had.contains(m))
            .collect();
        if !new.is_empty() {
            out.push_str(&format!("{name}:{}\n", new.join(",")));
        }
    }
    out
}

/// Whether `now` is the `group` line `was` with members added and none
/// taken away.
pub fn members_only(was: &str, now: &str) -> bool {
    let (a, b): (Vec<&str>, Vec<&str>) = (was.split(':').collect(), now.split(':').collect());
    a.len() == 4 && b.len() == 4 && a[..3] == b[..3] && {
        let after = members_of(now);
        members_of(was).iter().all(|m| after.contains(m))
    }
}

/// `group` (a file's text) with the members of `members` (`GROUP:USER,...`
/// lines) added to the groups it has, each once; `None` when nothing is
/// added. A group the file lacks is left alone (M7.2b).
pub fn add_members(group: &str, members: &str) -> Option<String> {
    let mut wanted: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for line in members.lines() {
        if let Some((name, users)) = line.split_once(':') {
            let list = wanted.entry(name).or_default();
            for user in users.split(',').filter(|u| !u.is_empty()) {
                if !list.contains(&user) {
                    list.push(user);
                }
            }
        }
    }
    let mut changed = false;
    let mut out = String::new();
    for line in group.lines() {
        let name = line.split(':').next().unwrap_or_default();
        let fields: Vec<&str> = line.split(':').collect();
        match wanted.get(name) {
            Some(users) if fields.len() == 4 => {
                let mut list = members_of(line);
                for user in users {
                    if !list.contains(user) {
                        list.push(user);
                        changed = true;
                    }
                }
                out.push_str(&format!("{}:{}", fields[..3].join(":"), list.join(",")));
            }
            _ => out.push_str(line),
        }
        out.push('\n');
    }
    changed.then_some(out)
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

/// The order the add-ons `named` (each with the add-on it is built on, if
/// any) lie over `/usr`, the topmost first, and those left off with the
/// add-on they need (M8.17c): an add-on built on another lies above it, so
/// its copy of a merged cache shows, and one whose base is not here is
/// left off, as its files need the base's.
pub fn layers(named: &[(String, Option<String>)]) -> (Vec<String>, Vec<(String, String)>) {
    let here = |base: &str| named.iter().any(|(n, on)| n == base && on.is_none());
    let mut skipped = Vec::new();
    let (mut over, mut rest) = (Vec::new(), Vec::new());
    for (name, on) in named {
        match on {
            Some(base) if here(base) => over.push(name.clone()),
            Some(base) => skipped.push((name.clone(), base.clone())),
            None => rest.push(name.clone()),
        }
    }
    over.extend(rest);
    (over, skipped)
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

/// The paths of a build-log list, joined by commas, with those under
/// `usr/share/mime/` (the MIME database's hundreds of files) folded into one
/// entry that counts them.
pub fn folded(paths: &[String]) -> String {
    let (mime, other): (Vec<&String>, Vec<&String>) =
        paths.iter().partition(|path| under(path, "usr/share/mime"));
    let mut shown: Vec<String> = other.into_iter().cloned().collect();
    if !mime.is_empty() {
        let count = mime.len();
        shown.push(format!("usr/share/mime/ ({count} files)"));
    }
    shown.join(", ")
}

// ---- On a machine (M7.2a): fetching, mounting and linking. ----

/// The add-ons the machine's settings file names (`addons.installed`),
/// its profiles' and the slot's default included (M7.6, M8.17c), read
/// leniently: the default alone while there is no file yet.
pub fn wanted_on_machine() -> Vec<String> {
    edel::addon_state::installed_on_machine()
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
    // While the file names no add-ons of its own, the new slot's default
    // decides, so a desktop whose files leave the slot for an add-on keeps
    // its desktop through the update (M8.17c).
    let wanted = {
        let path = places::found(&places::machine_settings());
        let default = edel::addon_state::default_in(&slot_file(slot, "/usr/lib/os-release"));
        match edel::settings::read_on_machine(&path) {
            Ok(read) => edel::addon_state::installed(&read.file, &default),
            Err(_) => default,
        }
    };
    let (to_fetch, notes) = fetch_plan(&wanted, &entries, &manifest.version, &built_in);
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
        fetch_one(location, entry, allow_http, &dir, false)?;
    }
    prune(&[&running, &manifest.version]);
    Ok(())
}

/// Whether the slot on `device` carries feature `name`, read with
/// debugfs, so the slot is never mounted.
fn built_into(device: &Path, name: &str) -> bool {
    !slot_file(device, &format!("{}/{name}.toml", edel::features::DIR)).is_empty()
}

/// The text of `file` in the slot on `device`, read with debugfs, so the
/// slot is never mounted; empty when it has none.
fn slot_file(device: &Path, file: &str) -> String {
    Command::new("debugfs")
        .args(["-R", &format!("cat {file}")])
        .arg(device)
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
        .unwrap_or_default()
}

/// Fetches one add-on to `dir/NAME.ext4` through a `.part` file renamed
/// only once it is checked; one already there with the right sha256 is
/// kept. `apply` says whether `edel settings apply` fetches it (M7.2b),
/// else `edel update`, which its messages name.
fn fetch_one(
    location: &str,
    entry: &AddonEntry,
    allow_http: bool,
    dir: &Path,
    apply: bool,
) -> Result<()> {
    let who = if apply {
        "edel settings"
    } else {
        "edel update"
    };
    let dest = dir.join(format!("{}.ext4", entry.name));
    let here = fs::File::open(&dest)
        .ok()
        .and_then(|mut file| crate::update::copy_hashing(&mut file, &mut std::io::sink()).ok());
    if here.is_some_and(|(size, sum)| size == entry.size && to_hex(&sum) == entry.sha256) {
        println!(
            "{who}: {}",
            trf("add-on {name} is already here", &[("name", &entry.name)])
        );
        return Ok(());
    }
    let part = dir.join(format!("{}.ext4.part", entry.name));
    if let Err(err) = fetch_part(location, entry, allow_http, &part, &dest, apply) {
        // A failed download, check or rename leaves no part file behind.
        let _ = fs::remove_file(&part);
        return Err(err);
    }
    println!(
        "{who}: {}",
        trf(
            "add-on {name} fetched and checked",
            &[("name", &entry.name)]
        )
    );
    Ok(())
}

/// Downloads an add-on to `part`, checks it by size and sha256 and renames
/// it to `dest`. The download stops one byte past the size, so a stream that
/// is too long fails the size check instead of filling the data partition,
/// where the running system's `/var` and `/home` live.
fn fetch_part(
    location: &str,
    entry: &AddonEntry,
    allow_http: bool,
    part: &Path,
    dest: &Path,
    apply: bool,
) -> Result<()> {
    let failed = || {
        if apply {
            trf(
                "could not fetch add-on {name}; nothing changes at the next restart; check the network and run edel settings apply again",
                &[("name", &entry.name)],
            )
        } else {
            trf(
                "could not fetch add-on {name}; the new slot stays off and this system is unchanged; check the network and run edel update again",
                &[("name", &entry.name)],
            )
        }
    };
    let source = crate::release::open_addon(location, entry, allow_http).with_context(failed)?;
    let mut source = source.take(entry.size.saturating_add(1));
    let mut file = fs::File::create(part).with_context(failed)?;
    let (size, sum) = crate::update::copy_hashing(&mut source, &mut file).with_context(failed)?;
    file.sync_all().with_context(failed)?;
    drop(file);
    if size != entry.size || to_hex(&sum) != entry.sha256 {
        if apply {
            bail!(
                "{}",
                trf(
                    "refused: add-on {name} does not match the sha256 in release.toml, so it is damaged or was changed on the way; nothing of it was kept; run edel settings apply again",
                    &[("name", &entry.name)],
                )
            );
        }
        bail!(
            "{}",
            trf(
                "refused: add-on {name} does not match the sha256 in release.toml, so it is damaged or was changed on the way; the new slot stays off and this system is unchanged; run edel update again",
                &[("name", &entry.name)],
            )
        );
    }
    fs::rename(part, dest).with_context(failed)?;
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

// ---- Pending add-ons (M7.2b): fetched by apply, tried at one boot. ----

/// What a boot does with the pending add-ons ([`boot_plan`]).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct BootPlan {
    /// Mounted with the others, once.
    pub tried: Vec<String>,
    /// The boot before did not finish with them: they go, and are listed.
    pub failed: Vec<String>,
    /// No longer chosen: they go, and nothing is said.
    pub discard: Vec<String>,
}

/// What a boot does with the `pending` add-ons: with the one-shot mark
/// (`marked`), the chosen ones are tried; without it, a boot that tried
/// them did not finish, so the chosen ones failed. Pending ones no longer
/// chosen go either way.
pub fn boot_plan(pending: &[String], wanted: &[String], marked: bool) -> BootPlan {
    let (chosen, discard): (Vec<String>, Vec<String>) =
        pending.iter().cloned().partition(|n| wanted.contains(n));
    if marked {
        BootPlan {
            tried: chosen,
            discard,
            ..BootPlan::default()
        }
    } else {
        BootPlan {
            failed: chosen,
            discard,
            ..BootPlan::default()
        }
    }
}

/// Of the add-ons this boot tried, those to keep (still chosen) and those
/// to take away, once the boot is confirmed.
pub fn promote_plan(tried: &[String], wanted: &[String]) -> (Vec<String>, Vec<String>) {
    tried.iter().cloned().partition(|n| wanted.contains(n))
}

/// The failed list after a boot: the names still chosen, then `new`, each
/// once. A name taken out of `addons.installed` leaves the list, so
/// choosing it again tries it again.
pub fn with_failed(failed: &[String], wanted: &[String], new: &[String]) -> Vec<String> {
    let mut list: Vec<String> = failed
        .iter()
        .filter(|n| wanted.contains(n))
        .cloned()
        .collect();
    for name in new {
        if !list.contains(name) {
            list.push(name.clone());
        }
    }
    list
}

/// The chosen add-ons apply fetches: neither here, pending, built into
/// the slot nor failed at their first boot.
pub fn missing(
    wanted: &[String],
    here: &[String],
    pending: &[String],
    built_in: &dyn Fn(&str) -> bool,
    failed: &[String],
) -> Vec<String> {
    wanted
        .iter()
        .filter(|n| {
            !here.contains(n) && !pending.contains(n) && !failed.contains(n) && !built_in(n)
        })
        .cloned()
        .collect()
}

/// One name a line.
fn lines(names: &[String]) -> String {
    names.iter().map(|n| format!("{n}\n")).collect()
}

/// Where the release list of `version` came from, as `edel update` kept
/// it in `places::RELEASE_FROM`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ReleaseFrom {
    pub format: u32,
    pub version: String,
    pub location: String,
    #[serde(default)]
    pub allow_http: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
}

/// Keeps where the release just installed came from, so apply fetches
/// that version's add-ons from the same list; a local path is kept whole.
/// A failure is said and changes nothing else.
pub fn remember_source(location: &str, version: &str, accept: &crate::release::Accept) {
    let location = if location.contains("://") {
        location.to_string()
    } else {
        fs::canonicalize(location)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| location.to_string())
    };
    let from = ReleaseFrom {
        format: 1,
        version: version.to_string(),
        location,
        allow_http: accept.allow_http,
        channel: accept.channel.clone(),
    };
    let written = toml::to_string(&from)
        .map_err(anyhow::Error::from)
        .and_then(|text| Ok(fs::write(places::RELEASE_FROM, text)?));
    if let Err(err) = written {
        eprintln!(
            "warning: {}",
            trf(
                "could not keep where the release came from ({why}); edel settings apply fetches add-ons from the channel's list instead",
                &[("why", &format!("{err:#}"))]
            )
        );
    }
}

/// The list to fetch `running`'s add-ons from: the one `edel update` took
/// it from, else the channel's.
fn source(running: &str) -> Result<(String, crate::release::Accept)> {
    let from: Option<ReleaseFrom> = fs::read_to_string(places::RELEASE_FROM)
        .ok()
        .and_then(|text| toml::from_str(&text).ok());
    match from {
        Some(from) if from.version == running => Ok((
            from.location,
            crate::release::Accept {
                allow_downgrade: true,
                channel: from.channel,
                allow_http: from.allow_http,
            },
        )),
        _ => {
            let accept = crate::release::Accept {
                allow_downgrade: true,
                ..crate::release::Accept::default()
            };
            Ok((crate::release::default_location(&accept)?, accept))
        }
    }
}

/// The network part of `edel settings apply` (M7.2b): fetches the chosen
/// add-ons this version lacks into its pending folder, checked as an
/// update's are, and leaves the one-shot mark, so the next boot tries
/// them. Nothing is fetched when nothing is missing, so apply needs no
/// network then. A list for another version (the channel moved on) only
/// says the add-on comes with that update.
pub fn fetch_chosen() -> Result<()> {
    let os_release = fs::read_to_string("/usr/lib/os-release").unwrap_or_default();
    let image = os_release_value(&os_release, "EDEL_IMAGE").unwrap_or_default();
    let running = os_release_value(&os_release, "VERSION_ID").unwrap_or_default();
    let dir = version_dir(&running);
    let pending = dir.join(places::ADDONS_PENDING);
    let wanted = wanted_on_machine();
    let failed = failed_in(&dir);
    let kept = with_failed(&failed, &wanted, &[]);
    if kept != failed {
        fs::write(dir.join(places::ADDONS_FAILED), lines(&kept))?;
    }
    for name in &kept {
        println!(
            "edel settings: {}",
            trf(
                "add-on {name} did not start at its first boot, so it is not fetched again; take it out of addons.installed and choose it again to try once more",
                &[("name", name)]
            )
        );
    }
    let built_in = |name: &str| {
        Path::new(edel::features::DIR)
            .join(format!("{name}.toml"))
            .exists()
    };
    // Pending ones no longer chosen go now; the boot would take them out.
    for name in names_in(&pending) {
        if !wanted.contains(&name) {
            let _ = fs::remove_file(pending.join(format!("{name}.ext4")));
        }
    }
    let missing = missing(
        &wanted,
        &names_in(&dir),
        &names_in(&pending),
        &built_in,
        &kept,
    );
    if missing.is_empty() {
        return Ok(());
    }
    // Fetching is network and disk work, which waits while a game plays (M7.7c).
    crate::shell::wait_for_game("edel settings");
    let (location, accept) = source(&running)?;
    let manifest = crate::release::verified(&location, &accept)?;
    if manifest.version != running {
        for name in &missing {
            println!(
                "edel settings: {}",
                trf(
                    "add-on {name} comes with the update to {version}, as the release list has moved on from {running}",
                    &[
                        ("name", name),
                        ("version", &manifest.version),
                        ("running", &running)
                    ]
                )
            );
        }
        return Ok(());
    }
    let entries = crate::release::addons_for(&manifest, &image);
    let (to_fetch, notes) = fetch_plan(&missing, &entries, &running, &built_in);
    for note in notes {
        println!("edel settings: {note}");
    }
    if to_fetch.is_empty() {
        return Ok(());
    }
    fs::create_dir_all(&pending).with_context(|| {
        trf(
            "could not make {path}",
            &[("path", &pending.display().to_string())],
        )
    })?;
    // The mark first: a fetch cut short leaves only checked files, which
    // the next boot tries, or none.
    fs::write(pending.join(places::ADDONS_TRY), "")?;
    for entry in &to_fetch {
        fetch_one(&location, entry, accept.allow_http, &pending, true)?;
    }
    let names: Vec<&str> = to_fetch.iter().map(|e| e.name.as_str()).collect();
    println!(
        "edel settings: {}",
        trf(
            "add-on {names} starts at the next restart",
            &[("names", &names.join(", "))]
        )
    );
    Ok(())
}

/// `edel install` (M7.2b): puts the add-ons the new machine's settings
/// file (`settings`, its text) chooses into the new data partition, mounted
/// at `data`, as pending ones with the one-shot mark, so its first boot
/// tries them as apply's are tried. Each comes from this machine's own
/// copy of the same version when it has one, else from the release list
/// this version came from, when it can be reached. Nothing here fails the
/// install: an add-on not staged is said and comes with apply later.
pub fn stage_for_install(settings: &str, data: &Path) {
    let default = edel::addon_state::running_default();
    let wanted = edel::settings::read(settings)
        .map(|read| edel::addon_state::installed(&read.file, &default))
        .unwrap_or_default();
    if wanted.is_empty() {
        return;
    }
    let os_release = fs::read_to_string("/usr/lib/os-release").unwrap_or_default();
    let image = os_release_value(&os_release, "EDEL_IMAGE").unwrap_or_default();
    let running = os_release_value(&os_release, "VERSION_ID").unwrap_or_default();
    let here = version_dir(&running);
    let under = |place: &Path| data.join(place.strip_prefix(places::DATA_MOUNT).unwrap_or(place));
    let pending = under(&here).join(places::ADDONS_PENDING);
    let built_in = |name: &str| {
        Path::new(edel::features::DIR)
            .join(format!("{name}.toml"))
            .exists()
            && !edel::addon_state::mounted().iter().any(|m| m == name)
    };
    // An add-on running here now has started on this machine's hardware,
    // so it goes straight beside the new slot, not to be tried: a first
    // boot that misses its health once must not take the desktop away
    // (M8.17c).
    let running_now = edel::addon_state::mounted();
    let mut copied = Vec::new();
    let mut staged = Vec::new();
    let mut missing = Vec::new();
    let said = |name: &str, why: String| {
        eprintln!(
            "warning: {}",
            trf(
                "add-on {name} was not copied ({why}); it comes once the new system runs edel settings apply online",
                &[("name", name), ("why", &why)]
            )
        )
    };
    if let Err(err) = fs::create_dir_all(&pending) {
        said(&wanted.join(", "), err.to_string());
        return;
    }
    for name in wanted.iter().filter(|n| !built_in(n)) {
        let file = format!("{name}.ext4");
        if here.join(&file).is_file() {
            let running = running_now.contains(name);
            let to = if running {
                under(&here)
            } else {
                pending.clone()
            };
            match fs::copy(here.join(&file), to.join(&file)) {
                Ok(_) if running => copied.push(name.clone()),
                Ok(_) => staged.push(name.clone()),
                Err(err) => said(name, err.to_string()),
            }
        } else {
            missing.push(name.clone());
        }
    }
    if !missing.is_empty() {
        let fetched = source(&running).and_then(|(location, accept)| {
            let manifest = crate::release::verified(&location, &accept)?;
            if manifest.version != running {
                bail!(
                    "{}",
                    trf(
                        "the release list has moved on to {version}",
                        &[("version", &manifest.version)]
                    )
                );
            }
            let entries = crate::release::addons_for(&manifest, &image);
            let (to_fetch, notes) = fetch_plan(&missing, &entries, &running, &built_in);
            for note in notes {
                println!("edel install: {note}");
            }
            let mut got = Vec::new();
            for entry in to_fetch {
                fetch_one(&location, entry, accept.allow_http, &pending, true)?;
                got.push(entry.name.clone());
            }
            Ok(got)
        });
        match fetched {
            Ok(got) => staged.extend(got),
            Err(err) => said(&missing.join(", "), format!("{err:#}")),
        }
    }
    if !copied.is_empty() {
        println!(
            "edel install: {}",
            trf(
                "add-on {names} copied; it starts with the new system",
                &[("names", &copied.join(", "))]
            )
        );
    }
    if staged.is_empty() {
        let _ = fs::remove_dir_all(&pending);
        return;
    }
    if let Err(err) = fs::write(pending.join(places::ADDONS_TRY), "") {
        said(&staged.join(", "), err.to_string());
        return;
    }
    println!(
        "edel install: {}",
        trf(
            "add-on {names} copied; it starts at the first boot",
            &[("names", &staged.join(", "))]
        )
    );
}

/// Once the guard confirms this boot (M7.2b): the pending add-ons it
/// tried and that are still chosen join the others, the rest go. Returns
/// what happened, one line each; a failure is a warning, as the boot is
/// fine either way and the next one tries them again.
pub fn promote() -> Vec<String> {
    let tried: Vec<String> = fs::read_to_string(places::ADDONS_TRIED)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect();
    if tried.is_empty() {
        return Vec::new();
    }
    let os_release = fs::read_to_string("/usr/lib/os-release").unwrap_or_default();
    let version = os_release_value(&os_release, "VERSION_ID").unwrap_or_default();
    let dir = version_dir(&version);
    let pending = dir.join(places::ADDONS_PENDING);
    let (keep, drop) = promote_plan(&tried, &wanted_on_machine());
    let mut said = Vec::new();
    for name in &keep {
        let file = format!("{name}.ext4");
        match fs::rename(pending.join(&file), dir.join(&file)) {
            Ok(()) => said.push(format!(
                "edel update: {}",
                trf("add-on {name} started, so it stays", &[("name", name)])
            )),
            Err(err) => said.push(format!(
                "warning: {}",
                trf(
                    "add-on {name} started but could not join the others ({why}); the next boot tries it again",
                    &[("name", name), ("why", &err.to_string())]
                )
            )),
        }
    }
    for name in &drop {
        let _ = fs::remove_file(pending.join(format!("{name}.ext4")));
    }
    let _ = fs::remove_file(places::ADDONS_TRIED);
    if names_in(&pending).is_empty() && !pending.join(places::ADDONS_TRY).exists() {
        let _ = fs::remove_dir_all(&pending);
    }
    said
}

/// `edel boot mount-data`, first: mounts each add-on the settings file
/// names that this slot's version has on `/data`, read-only at
/// `/run/edel/addons/NAME`, and lays their `/usr` over the slot's, so the
/// rest of the boot sees them. A chosen add-on that is not here is said,
/// never fatal: `edel settings apply` or the next update fetches it.
///
/// The pending ones `edel settings apply` fetched (M7.2b) are tried once:
/// with the mark there, it is taken away (and written to the disk) before
/// they mount, so a boot that hangs comes back without them; without it,
/// they are the ones a boot did not finish with, so they go, are listed as
/// failed and the failure is recorded for the person.
pub fn mount_all() -> Result<()> {
    let os_release = fs::read_to_string("/usr/lib/os-release").unwrap_or_default();
    let version = os_release_value(&os_release, "VERSION_ID").unwrap_or_default();
    let dir = version_dir(&version);
    let pending = dir.join(places::ADDONS_PENDING);
    let mark = pending.join(places::ADDONS_TRY);
    let wanted = wanted_on_machine();
    let plan = boot_plan(&names_in(&pending), &wanted, mark.exists());
    if mark.exists() {
        fs::remove_file(&mark)?;
        // The mark must be gone from the disk before anything new mounts.
        fs::File::open(&pending)?.sync_all()?;
    }
    for name in plan.failed.iter().chain(&plan.discard) {
        let _ = fs::remove_file(pending.join(format!("{name}.ext4")));
    }
    if !plan.failed.is_empty() {
        let failed = with_failed(&failed_in(&dir), &wanted, &plan.failed);
        if let Err(err) = fs::write(dir.join(places::ADDONS_FAILED), lines(&failed)) {
            eprintln!("warning: edel-data: could not list the failed add-ons: {err}");
        }
        let names = plan.failed.join(", ");
        println!(
            "edel-data: {}",
            trf(
                "add-on {names} did not start at the last boot, so Edel OS started without it; it is not fetched again until it is taken out of addons.installed and chosen again",
                &[("names", &names)]
            )
        );
        if let Err(err) = crate::update::record_failed_addons(&plan.failed) {
            eprintln!(
                "warning: edel-data: the add-on that did not start could not be recorded: {err:#}; Settings will not mention it"
            );
        }
    }
    let mut found = Vec::new();
    for name in wanted {
        let file = dir.join(format!("{name}.ext4"));
        let file = if !file.is_file() && plan.tried.contains(&name) {
            pending.join(format!("{name}.ext4"))
        } else {
            file
        };
        if !file.is_file() {
            if !Path::new(edel::features::DIR)
                .join(format!("{name}.toml"))
                .exists()
            {
                println!(
                    "edel-data: {}",
                    trf(
                        "add-on {name} is chosen but not here yet; edel settings apply fetches it, and it starts at the next restart",
                        &[("name", &name)]
                    )
                );
            }
            continue;
        }
        // The add-on it is built over, from its own feature file (M8.17c).
        let text = slot_file(&file, &format!("{}/{name}.toml", edel::features::DIR));
        let on = edel::features::read(&text).ok().and_then(|(f, _)| f.on);
        found.push((name, file, on));
    }
    let named: Vec<(String, Option<String>)> = found
        .iter()
        .map(|(n, _, on)| (n.clone(), on.clone()))
        .collect();
    let (order, skipped) = layers(&named);
    for (name, base) in skipped {
        println!(
            "edel-data: {}",
            trf(
                "add-on {name} needs the {base} add-on, so it is left off; add it with edel settings set addons.installed+={base}",
                &[("name", &name), ("base", &base)]
            )
        );
    }
    let mut mounts = Vec::new();
    let mut tried = String::new();
    for name in order {
        let Some((_, file, _)) = found.iter().find(|(n, _, _)| *n == name) else {
            continue;
        };
        let file = file.clone();
        let at = Path::new(places::ADDONS_RUN).join(&name);
        fs::create_dir_all(&at)?;
        if !crate::data::is_mounted(&at.to_string_lossy())? {
            let _ = Command::new("modprobe").arg("loop").status();
            let mounted = run(Command::new("mount")
                .args(["-t", "ext4", "-o", "ro,loop,nodev"])
                .arg(&file)
                .arg(&at))
            .with_context(|| {
                trf(
                    "could not mount add-on {name}; the other add-ons still mount, and the next update fetches it again",
                    &[("name", &name)],
                )
            });
            // One add-on that does not mount never keeps the others away.
            if let Err(err) = mounted {
                eprintln!("warning: edel-data: {err:#}");
                continue;
            }
        }
        if file.starts_with(&pending) {
            tried.push_str(&name);
            tried.push('\n');
        }
        mounts.push(at);
    }
    if !tried.is_empty() {
        fs::write(places::ADDONS_TRIED, &tried)?;
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
    if !tried.is_empty() {
        println!(
            "edel-data: {}",
            trf(
                "trying the add-ons fetched since the last boot: {names}; they stay once this boot is confirmed",
                &[("names", &tried.trim_end().replace('\n', ", "))]
            )
        );
    }
    Ok(())
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

/// The members the mounted add-ons add to groups the slot has (M7.2b),
/// every add-on's `GROUP:USER,...` lines in name order.
pub fn member_lines() -> String {
    let mut dirs: Vec<PathBuf> = fs::read_dir(places::ADDON_ACCOUNTS_DIR)
        .map(|entries| entries.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    dirs.sort();
    dirs.iter()
        .filter_map(|d| fs::read_to_string(d.join(MEMBERS_FILE)).ok())
        .collect()
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
    /// Paths under `/etc` that hold something of the machine's or the
    /// slot's: a file, a folder or a link that is not ours, which are never
    /// replaced.
    pub theirs: BTreeSet<String>,
}

/// Whether a mounted add-on brings a service: its init script is in
/// `places::ADDON_INIT_DIR`, from where apply links it into `/etc/init.d`.
pub fn has_services() -> bool {
    fs::read_dir(places::ADDON_INIT_DIR)
        .map(|mut entries| entries.next().is_some())
        .unwrap_or(false)
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
                Path::new("/etc").join(rest).symlink_metadata().is_ok()
                    && !ours.iter().any(|(mine, _)| mine == *rest)
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
    /// `edel::addon_state::State::word`: `installed`, `built-in`,
    /// `pending`, `failed`, `chosen`, `leaving` or empty.
    pub status: String,
}

/// The rows of `edel addon list`: the image's add-ons (`offered`, name and
/// summary), then any chosen or mounted one it lacks, each with where it
/// stands by `facts`.
pub fn rows(offered: &[(String, String)], facts: &edel::addon_state::Facts) -> Vec<Row> {
    let status = |name: &str| edel::addon_state::state(name, facts).word().to_string();
    let mut rows: Vec<Row> = offered
        .iter()
        .map(|(name, summary)| Row {
            name: name.clone(),
            summary: summary.clone(),
            status: status(name),
        })
        .collect();
    for name in facts.mounted.iter().chain(&facts.wanted) {
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

/// `edel addon list`: the add-ons this image was built with, read without
/// the network (M7.2b), each with where it stands; an image that names
/// none lists those of its channel's release for it, which needs the
/// network, and offline says so and lists what this machine has.
pub fn list() -> Result<()> {
    let mut offered = edel::addon_state::offered();
    if offered.is_empty() {
        let accept = crate::release::Accept::default();
        let os_release = fs::read_to_string("/usr/lib/os-release").unwrap_or_default();
        let image = os_release_value(&os_release, "EDEL_IMAGE").unwrap_or_default();
        match crate::release::default_location(&accept)
            .and_then(|at| crate::release::verified(&at, &accept))
        {
            Ok(manifest) => {
                offered = crate::release::addons_for(&manifest, &image)
                    .into_iter()
                    .map(|a| (a.name.clone(), a.summary.clone()))
                    .collect()
            }
            Err(err) => eprintln!(
                "warning: {}",
                trf(
                    "could not read the release list ({why}), so only this machine's add-ons are shown",
                    &[("why", &format!("{err:#}"))]
                )
            ),
        }
    }
    let rows = rows(&offered, &edel::addon_state::Facts::read());
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
        tr(
            "to add one: edel settings set addons.installed+=NAME, then edel settings apply; it starts at the next restart"
        )
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

    #[test]
    fn an_add_on_lies_over_the_one_it_is_built_on_or_stays_off() {
        let named = |list: &[(&str, Option<&str>)]| -> Vec<(String, Option<String>)> {
            list.iter()
                .map(|(n, on)| (n.to_string(), on.map(String::from)))
                .collect()
        };
        let (order, skipped) = layers(&named(&[
            ("desktop", None),
            ("printing", None),
            ("orca", Some("desktop")),
        ]));
        assert_eq!(order, ["orca", "desktop", "printing"]);
        assert!(skipped.is_empty());
        let (order, skipped) = layers(&named(&[("orca", Some("desktop")), ("printing", None)]));
        assert_eq!(order, ["printing"]);
        assert_eq!(skipped, [("orca".to_string(), "desktop".to_string())]);
    }
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
            ..link("coreutils")
        };
        let before = tree(&[("bin/ls", link("coreutils")), ("etc/x", file(1))]);
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
            "usr/share/info/dir",
            "etc/shells",
            "etc/udev/hwdb.bin",
            "usr/lib/udev/hwdb.bin",
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
    fn a_merged_cache_is_recognised_by_its_path() {
        for cache in [
            "usr/share/glib-2.0/schemas/gschemas.compiled",
            "usr/share/applications/mimeinfo.cache",
            "usr/share/mime/mime.cache",
            "usr/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache",
            "usr/lib/gio/modules/giomodule.cache",
            "usr/lib/gtk-3.0/3.0.0/immodules.cache",
            "usr/lib/gtk-4.0/4.0.0/printbackends/giomodule.cache",
        ] {
            assert!(is_merged_cache(cache), "{cache}");
        }
        assert!(!is_merged_cache("usr/share/mime/packages/x.xml"));
        assert!(!is_merged_cache("usr/share/icons/hicolor/icon-theme.cache"));
        assert!(!is_merged_cache("usr/bin/tool"));
        assert!(!is_system_cache(
            "usr/share/glib-2.0/schemas/gschemas.compiled"
        ));
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
                kept_applets: vec![],
            }
        );
        // A folder whose only change is its time is not changed.
        let same = tree(&[("usr", dir(9))]);
        assert_eq!(diff(&before, &same).changed, Vec::<String>::new());
        // A small tool of the base a package replaced keeps the base's link.
        let applet = tree(&[("usr/bin/bc", link("/bin/busybox"))]);
        let replaced = tree(&[("usr/bin/bc", file(7))]);
        assert_eq!(
            diff(&applet, &replaced),
            Diff {
                kept_applets: vec!["usr/bin/bc".into()],
                ..Default::default()
            }
        );
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
        let plan = plan_tree(&diff(&before, &after), &after, "ci-hello", false).unwrap();
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
    fn plan_tree_keeps_a_merged_cache_the_packages_rewrote() {
        let before = tree(&[
            ("usr/share/glib-2.0/schemas/gschemas.compiled", file(1)),
            ("usr/share/icons/hicolor/icon-theme.cache", file(1)),
        ]);
        let after = tree(&[
            ("usr/lib/gtk-3.0/3.0.0/immodules.cache", file(3)),
            ("usr/share/glib-2.0/schemas/gschemas.compiled", file(2)),
            ("usr/share/icons/hicolor/icon-theme.cache", file(2)),
        ]);
        let plan = plan_tree(&diff(&before, &after), &after, "ci-hello", false).unwrap();
        let copies: Vec<(&str, &str)> = plan
            .copies
            .iter()
            .map(|c| (c.from.as_str(), c.to.as_str()))
            .collect();
        assert_eq!(
            copies,
            vec![
                (
                    "usr/lib/gtk-3.0/3.0.0/immodules.cache",
                    "usr/lib/gtk-3.0/3.0.0/immodules.cache"
                ),
                (
                    "usr/share/glib-2.0/schemas/gschemas.compiled",
                    "usr/share/glib-2.0/schemas/gschemas.compiled"
                ),
            ]
        );
        assert_eq!(
            plan.merged,
            [
                "usr/lib/gtk-3.0/3.0.0/immodules.cache",
                "usr/share/glib-2.0/schemas/gschemas.compiled"
            ]
        );
        assert_eq!(plan.caches, ["usr/share/icons/hicolor/icon-theme.cache"]);
    }

    #[test]
    fn plan_tree_refuses_a_tree_and_names_every_refusal() {
        let before = tree(&[("usr/lib/libc.so", file(5)), ("usr/share/doc/x", file(1))]);
        let after = tree(&[("opt/x", file(1)), ("usr/lib/libc.so", file(6))]);
        let error = plan_tree(&diff(&before, &after), &after, "ci-hello", false)
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
        let plan = plan_tree(&diff(&BTreeMap::new(), &after), &after, "ci-hello", false).unwrap();
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
    fn changed_accounts_names_lines_a_package_rewrote() {
        let image = "root:x:0:0:root:/root:/bin/sh\nwheel:x:10:\n";
        let addon = "root:x:0:0:root:/root:/bin/sh\nwheel:x:10:alice\napp:x:1001:1001::/:/bin/sh\n";
        assert_eq!(changed_accounts(image, addon), ["wheel"]);
        assert!(changed_accounts(image, image).is_empty());
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
    fn folded_lists_the_mime_database_as_one_entry() {
        let paths = [
            "usr/lib/gio/modules/giomodule.cache",
            "usr/share/mime/mime.cache",
            "usr/share/mime/a/b.cache",
        ]
        .map(String::from);
        assert_eq!(
            folded(&paths),
            "usr/lib/gio/modules/giomodule.cache, usr/share/mime/ (2 files)"
        );
        assert_eq!(folded(&["etc/shells".to_string()]), "etc/shells");
        assert_eq!(folded(&[]), "");
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
        let facts = edel::addon_state::Facts {
            wanted: vec!["printing".to_string(), "ci-gone".to_string()],
            mounted: vec!["printing".to_string()],
            ..Default::default()
        };
        let rows = rows(&offered, &facts);
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

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn a_marked_boot_tries_the_pending_ones_and_one_without_the_mark_fails_them() {
        let pending = names(&["ci-hang", "gone", "virtualization"]);
        let wanted = names(&["ci-hello", "ci-hang", "virtualization"]);
        let tried = boot_plan(&pending, &wanted, true);
        assert_eq!(tried.tried, names(&["ci-hang", "virtualization"]));
        assert!(tried.failed.is_empty());
        assert_eq!(tried.discard, names(&["gone"]));
        // The boot that tried them did not finish: no mark is left.
        let failed = boot_plan(&pending, &wanted, false);
        assert!(failed.tried.is_empty());
        assert_eq!(failed.failed, names(&["ci-hang", "virtualization"]));
        assert_eq!(failed.discard, names(&["gone"]));
        assert_eq!(boot_plan(&[], &wanted, true), BootPlan::default());
    }

    #[test]
    fn a_confirmed_boot_keeps_the_tried_ones_still_chosen() {
        let (keep, drop) = promote_plan(&names(&["a", "b"]), &names(&["b", "c"]));
        assert_eq!(keep, names(&["b"]));
        assert_eq!(drop, names(&["a"]));
    }

    #[test]
    fn a_failed_add_on_is_fetched_again_only_once_chosen_again() {
        let failed = with_failed(
            &names(&["a", "b"]),
            &names(&["b", "c"]),
            &names(&["c", "b"]),
        );
        assert_eq!(failed, names(&["b", "c"]));
        assert!(with_failed(&names(&["a"]), &[], &[]).is_empty());
        let built_in = |name: &str| name == "fonts";
        let wanted = names(&["here", "pending", "fonts", "failed", "new"]);
        assert_eq!(
            missing(
                &wanted,
                &names(&["here"]),
                &names(&["pending"]),
                &built_in,
                &names(&["failed"])
            ),
            names(&["new"])
        );
    }

    #[test]
    fn members_an_add_on_adds_to_a_group_join_it_at_boot() {
        let image = "root:x:0:root\nkvm:x:34:\naudio:x:18:a\n";
        let addon = "root:x:0:root\nkvm:x:34:qemu\naudio:x:18:a,b\nqemu:x:100:\n";
        assert_eq!(added_members(image, addon), "kvm:qemu\naudio:b\n");
        // Another id, or a member taken away, is no member added.
        assert_eq!(added_members(image, "kvm:x:35:qemu\naudio:x:18:b\n"), "");
        assert!(members_only("kvm:x:34:", "kvm:x:34:qemu"));
        assert!(!members_only("audio:x:18:a", "audio:x:18:b"));
        let machine = "root:x:0:root\nkvm:x:34:\naudio:x:18:a,b\n";
        assert_eq!(
            add_members(machine, "kvm:qemu\naudio:b\nmissing:x\nkvm:qemu,other\n").as_deref(),
            Some("root:x:0:root\nkvm:x:34:qemu,other\naudio:x:18:a,b\n")
        );
        assert_eq!(add_members(machine, "audio:a\n"), None);
        assert_eq!(add_members(machine, ""), None);
    }
}
