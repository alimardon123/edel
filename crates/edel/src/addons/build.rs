//! The image builder's rules for an add-on (M7.2a): where each file it
//! installs lands, which of them are caches, what changed against the
//! image, the tree that ships, and how its accounts merge.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use anyhow::{Context, Result, bail};
use edel::places;

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
#[derive(Debug, Default, Clone, PartialEq, Eq)]
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

/// The lines of `earlier` (other add-ons' new account lines) whose names
/// `text` lacks, to hold their ids while an add-on's packages are added
/// (M8.17c): add-ons are built side by side from one root, and apk gives a
/// new system group or user the next free id, so without them two add-ons
/// a machine mounts together would give one id to two names.
pub fn held_accounts(text: &str, earlier: &str) -> String {
    let names: BTreeSet<&str> = text
        .lines()
        .map(|line| line.split(':').next().unwrap_or_default())
        .collect();
    earlier
        .lines()
        .filter(|line| {
            !line.is_empty() && !names.contains(line.split(':').next().unwrap_or_default())
        })
        .map(|line| format!("{line}\n"))
        .collect()
}

/// `text` without the lines `held` added to it, once the packages are in.
pub fn without_held(text: &str, held: &str) -> String {
    let held: BTreeSet<&str> = held.lines().collect();
    text.lines()
        .filter(|line| !held.contains(line))
        .map(|line| format!("{line}\n"))
        .collect()
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

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;
    use std::process;

    use super::*;
    use crate::addons::machine::first_of_each;

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
    fn earlier_add_ons_ids_are_held_while_the_packages_go_in() {
        let image = "root:x:0:\nlp:x:7:\n";
        let earlier = "seat:x:104:\nlp:x:7:\n";
        let held = held_accounts(image, earlier);
        assert_eq!(held, "seat:x:104:\n");
        let after = format!("{image}{held}cups:x:105:\n");
        assert_eq!(
            without_held(&after, &held),
            "root:x:0:\nlp:x:7:\ncups:x:105:\n"
        );
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
