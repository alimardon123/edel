//! What an image keeps of a package's files (M5.30): a feature's `[[keep]]`
//! names a folder of the root, the files under it to keep, and the ones of
//! which only the newest version a driver asks for is kept (a family the
//! driver does not name keeps every version); the image
//! builder takes every other file there out. The desktop stick keeps the
//! firmware of the laptops and desktops of the last ten years and the
//! fonts of the interface, so the rest of `linux-firmware` and of the font
//! packages does not weigh on every machine. Pure: it plans from a list
//! of names, and `image.rs` lists, reads the driver and removes.

use std::collections::BTreeMap;

use anyhow::{Result, bail};
use serde::Deserialize;

/// A feature's `[[keep]]`: under `under` (a folder of the root, without a
/// leading `/`), only the files `files` match stay. Of those `newest`
/// matches, only the newest version of each stays: the highest number at
/// the end of the name, and no higher than the driver `newest_by` (a
/// kernel module's name) says it can load, so a version newer than the
/// kernel never takes the place of one it can use.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Keep {
    pub under: String,
    pub files: Vec<String>,
    #[serde(default)]
    pub newest: Vec<String>,
    #[serde(default)]
    pub newest_by: Option<String>,
}

/// A file under the folder: its path from the folder, and where it points
/// when it is a symbolic link (relative to the link's own folder, as a
/// package writes it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: String,
    pub link: Option<String>,
    pub size: u64,
}

/// What stays and what goes.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub kept: Vec<String>,
    pub removed: Vec<String>,
    pub kept_bytes: u64,
    pub removed_bytes: u64,
}

/// Checks a `[[keep]]` strictly, for `edel image check` and the build.
pub fn check(keep: &Keep) -> Result<()> {
    let under = keep.under.trim_matches('/');
    if under.is_empty() || under.split('/').any(|p| p.is_empty() || p == "..") {
        bail!(
            "keep.under {:?} must be a folder of the root, such as \"lib/firmware\"",
            keep.under
        );
    }
    if keep.files.is_empty() {
        bail!("keep under {under} lists no files; say which files stay");
    }
    for pattern in keep.files.iter().chain(&keep.newest) {
        if pattern.is_empty() || pattern.starts_with('/') || pattern.contains("..") {
            bail!(
                "keep pattern {pattern:?} under {under} must be a path below it, such as \"i915/*\""
            );
        }
    }
    if let Some(p) = keep.newest.iter().find(|p| !keep.files.contains(p)) {
        bail!("keep.newest names {p:?}, which keep.files does not; list it there too");
    }
    if !keep.newest.is_empty() && keep.newest_by.is_none() {
        bail!(
            "keep under {under} has newest patterns but no newest_by; name the driver that loads them, so a version newer than the kernel is never the only one kept"
        );
    }
    Ok(())
}

/// Whether `name` matches `pattern`: `*` is any run of characters but `/`,
/// `?` one such character; everything else stands for itself.
pub fn matches(pattern: &str, name: &str) -> bool {
    fn go(p: &[u8], n: &[u8]) -> bool {
        match p.split_first() {
            None => n.is_empty(),
            Some((b'*', rest)) => (0..=n.len())
                .take_while(|&i| i == 0 || n[i - 1] != b'/')
                .any(|i| go(rest, &n[i..])),
            Some((b'?', rest)) => n.first().is_some_and(|&c| c != b'/') && go(rest, &n[1..]),
            Some((c, rest)) => n.first() == Some(c) && go(rest, &n[1..]),
        }
    }
    go(pattern.as_bytes(), name.as_bytes())
}

/// A versioned file's family and version: `iwlwifi-so-a0-gf-a0-89.ucode.zst`
/// is family `iwlwifi-so-a0-gf-a0` (with `.ucode`) at 89, and
/// `iwlwifi-sc-a0-wh-b0-c103.ucode` at core version 103 of the `c` kind,
/// which never mixes with plain numbers. Only the file's own name counts.
pub fn version(path: &str) -> Option<(String, String, u32)> {
    let name = path.rsplit('/').next()?;
    let stem_end = name.find('.').unwrap_or(name.len());
    let (stem, ext) = name.split_at(stem_end);
    let dash = stem.rfind('-')?;
    let tail = &stem[dash + 1..];
    let (kind, digits) = match tail.strip_prefix('c') {
        Some(d) => ("c", d),
        None => ("", tail),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let ext = ext.split('.').nth(1).unwrap_or("");
    let family = format!("{}{kind}.{ext}", &stem[..dash]);
    Some((family, kind.to_string(), digits.parse().ok()?))
}

/// The highest version a driver says it loads, for each family, from the
/// names it declares (`firmware=` in its module).
pub fn declared_max(declared: &[String]) -> BTreeMap<String, u32> {
    let mut max = BTreeMap::new();
    for name in declared {
        if let Some((family, _, v)) = version(name) {
            let e = max.entry(family).or_insert(v);
            *e = (*e).max(v);
        }
    }
    max
}

/// Plans what stays under the folder. Fails when a pattern of `files`
/// matches nothing (a listed device with no firmware is a mistake to fix,
/// not a file to lose quietly).
pub fn plan(entries: &[Entry], keep: &Keep, declared: &[String]) -> Result<Plan> {
    let max = declared_max(declared);
    let listed = |path: &str| keep.files.iter().any(|p| matches(p, path));
    let versioned = |path: &str| keep.newest.iter().any(|p| matches(p, path));

    // The newest version the driver loads of each family it names, among
    // the real files. A family it does not name keeps every version: what
    // it would ask for is not known, so nothing of it is guessed away.
    let mut best: BTreeMap<String, u32> = BTreeMap::new();
    for e in entries
        .iter()
        .filter(|e| e.link.is_none() && versioned(&e.path))
    {
        if let Some((family, _, v)) = version(&e.path) {
            if max.get(&family).is_some_and(|m| v <= *m) {
                let b = best.entry(family).or_insert(v);
                *b = (*b).max(v);
            }
        }
    }
    let stays_file = |e: &Entry| {
        listed(&e.path)
            && (!versioned(&e.path)
                || version(&e.path).is_none_or(|(family, _, v)| {
                    !max.contains_key(&family) || best.get(&family) == Some(&v)
                }))
    };
    let files: BTreeMap<&str, &Entry> = entries.iter().map(|e| (e.path.as_str(), e)).collect();
    let mut kept = std::collections::BTreeSet::new();
    for e in entries.iter().filter(|e| e.link.is_none()) {
        if stays_file(e) {
            kept.insert(e.path.clone());
        }
    }
    // A link stays when it is listed and what it points at stays.
    for e in entries {
        let Some(target) = &e.link else { continue };
        if !listed(&e.path) {
            continue;
        }
        let mut seen = 0;
        let mut at = resolve(&e.path, target);
        while let Some(next) = files.get(at.as_str()).and_then(|t| t.link.as_ref()) {
            seen += 1;
            if seen > 8 {
                break;
            }
            at = resolve(&at, next);
        }
        if kept.contains(&at) {
            kept.insert(e.path.clone());
        }
    }
    for pattern in &keep.files {
        if !entries.iter().any(|e| matches(pattern, &e.path)) {
            bail!(
                "keep pattern {pattern:?} under {} matches no file: the package that had it changed, or the pattern is wrong; fix the list in the feature",
                keep.under
            );
        }
    }
    let mut plan = Plan::default();
    for e in entries {
        if kept.contains(&e.path) {
            plan.kept.push(e.path.clone());
            plan.kept_bytes += e.size;
        } else {
            plan.removed.push(e.path.clone());
            plan.removed_bytes += e.size;
        }
    }
    Ok(plan)
}

/// Where `target`, written in the link at `path`, points, from the folder.
fn resolve(path: &str, target: &str) -> String {
    let mut parts: Vec<&str> = path.split('/').collect();
    parts.pop();
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str) -> Entry {
        Entry {
            path: path.into(),
            link: None,
            size: 10,
        }
    }

    fn link(path: &str, to: &str) -> Entry {
        Entry {
            path: path.into(),
            link: Some(to.into()),
            size: 0,
        }
    }

    fn keep(files: &[&str], newest: &[&str]) -> Keep {
        Keep {
            under: "lib/firmware".into(),
            files: files.iter().map(|s| s.to_string()).collect(),
            newest: newest.iter().map(|s| s.to_string()).collect(),
            newest_by: (!newest.is_empty()).then(|| "iwlwifi".into()),
        }
    }

    #[test]
    fn a_star_stays_inside_one_folder() {
        assert!(matches("i915/*", "i915/tgl_dmc.bin.zst"));
        assert!(!matches("i915/*", "i915/sub/x.bin"));
        assert!(matches(
            "ath11k/WCN6855/*/*",
            "ath11k/WCN6855/hw2.0/amss.bin.zst"
        ));
        assert!(matches("iwlwifi-?u-*", "iwlwifi-Qu-b0-hr-b0-77.ucode.zst"));
        assert!(!matches("mediatek/mt79*", "mediatek/mt8183/scp.img"));
    }

    #[test]
    fn a_version_is_the_number_at_the_end_of_the_name() {
        assert_eq!(
            version("intel/iwlwifi/iwlwifi-so-a0-gf-a0-89.ucode.zst"),
            Some(("iwlwifi-so-a0-gf-a0.ucode".into(), "".into(), 89))
        );
        assert_eq!(
            version("iwlwifi-sc-a0-wh-b0-c103.ucode"),
            Some(("iwlwifi-sc-a0-wh-b0c.ucode".into(), "c".into(), 103))
        );
        assert_eq!(version("intel/iwlwifi/iwlwifi-so-a0-gf-a0.pnvm.zst"), None);
    }

    #[test]
    fn only_the_newest_version_the_driver_loads_stays() {
        let entries = [
            file("intel/iwlwifi/iwlwifi-so-a0-gf-a0-86.ucode.zst"),
            file("intel/iwlwifi/iwlwifi-so-a0-gf-a0-89.ucode.zst"),
            file("intel/iwlwifi/iwlwifi-so-a0-gf-a0-92.ucode.zst"),
            file("intel/iwlwifi/iwlwifi-so-a0-gf-a0.pnvm.zst"),
            link(
                "iwlwifi-so-a0-gf-a0-89.ucode.zst",
                "intel/iwlwifi/iwlwifi-so-a0-gf-a0-89.ucode.zst",
            ),
            link(
                "iwlwifi-so-a0-gf-a0-86.ucode.zst",
                "intel/iwlwifi/iwlwifi-so-a0-gf-a0-86.ucode.zst",
            ),
            file("intel/qat_4xxx.bin.zst"),
        ];
        let k = keep(
            &["intel/iwlwifi/*", "iwlwifi-*"],
            &["intel/iwlwifi/iwlwifi-*.ucode.zst"],
        );
        // The kernel loads up to 89 of this family, so 92 is no use to it.
        let declared = vec!["iwlwifi-so-a0-gf-a0-89.ucode".to_string()];
        let plan = plan(&entries, &k, &declared).unwrap();
        assert_eq!(
            plan.kept,
            [
                "intel/iwlwifi/iwlwifi-so-a0-gf-a0-89.ucode.zst",
                "intel/iwlwifi/iwlwifi-so-a0-gf-a0.pnvm.zst",
                "iwlwifi-so-a0-gf-a0-89.ucode.zst",
            ]
        );
        assert_eq!(plan.removed.len(), 4);
        assert_eq!(plan.kept_bytes, 20);
    }

    #[test]
    fn a_family_the_driver_does_not_name_keeps_every_version() {
        let entries = [
            file("intel/iwlwifi/iwlwifi-sc-a0-wh-b0-c101.ucode.zst"),
            file("intel/iwlwifi/iwlwifi-sc-a0-wh-b0-c103.ucode.zst"),
            file("intel/iwlwifi/iwlwifi-sc-a0-wh-b0-101.ucode.zst"),
        ];
        let k = keep(&["intel/iwlwifi/*"], &["intel/iwlwifi/*.ucode.zst"]);
        let plan = plan(&entries, &k, &[]).unwrap();
        assert_eq!(plan.kept.len(), 3);
        assert!(plan.removed.is_empty());
    }

    #[test]
    fn versions_all_newer_than_the_driver_loads_go() {
        let entries = [
            file("intel/iwlwifi/iwlwifi-gl-c0-fm-c0-c101.ucode.zst"),
            file("intel/iwlwifi/iwlwifi-gl-c0-fm-c0-101.ucode.zst"),
        ];
        let k = keep(&["intel/iwlwifi/*"], &["intel/iwlwifi/*.ucode.zst"]);
        let declared = vec!["iwlwifi-gl-c0-fm-c0-c99.ucode".to_string()];
        let plan = plan(&entries, &k, &declared).unwrap();
        assert_eq!(
            plan.kept,
            ["intel/iwlwifi/iwlwifi-gl-c0-fm-c0-101.ucode.zst"]
        );
    }

    #[test]
    fn a_listed_device_with_no_firmware_fails_the_plan() {
        let entries = [file("i915/tgl_dmc.bin.zst")];
        let k = keep(&["i915/*", "mediatek/mt7925/*"], &[]);
        let err = plan(&entries, &k, &[]).unwrap_err().to_string();
        assert!(err.contains("\"mediatek/mt7925/*\""), "{err}");
        assert!(err.contains("matches no file"), "{err}");
    }

    #[test]
    fn a_file_no_listed_device_uses_goes_and_a_link_follows_its_target() {
        let entries = [
            file("mediatek/mt7996/mt7996_wm.bin.zst"),
            file("mediatek/WIFI_RAM_CODE_MT7922_1.bin.zst"),
            link("mediatek/old.bin.zst", "mt7996/mt7996_wm.bin.zst"),
            link("mediatek/kept.bin.zst", "WIFI_RAM_CODE_MT7922_1.bin.zst"),
        ];
        let k = keep(&["mediatek/*MT7922*", "mediatek/*.bin.zst"], &[]);
        let plan = plan(&entries, &k, &[]).unwrap();
        assert_eq!(
            plan.kept,
            [
                "mediatek/WIFI_RAM_CODE_MT7922_1.bin.zst",
                "mediatek/kept.bin.zst"
            ]
        );
        assert!(plan.removed.contains(&"mediatek/old.bin.zst".to_string()));
        assert!(
            plan.removed
                .contains(&"mediatek/mt7996/mt7996_wm.bin.zst".to_string())
        );
    }

    #[test]
    fn a_keep_is_checked_strictly() {
        assert!(check(&keep(&["i915/*"], &[])).is_ok());
        let mut k = keep(&["i915/*"], &[]);
        k.under = "../etc".into();
        assert!(check(&k).is_err());
        let k = keep(&["a/*"], &["b/*"]);
        assert!(
            check(&k)
                .unwrap_err()
                .to_string()
                .contains("keep.files does not")
        );
        let mut k = keep(&["a/*"], &["a/*"]);
        k.newest_by = None;
        assert!(check(&k).unwrap_err().to_string().contains("newest_by"));
    }

    #[test]
    fn a_link_points_from_its_own_folder() {
        assert_eq!(
            resolve("intel/dsp_fw_kbl.bin.zst", "avs/skl/dsp_basefw.bin.zst"),
            "intel/avs/skl/dsp_basefw.bin.zst"
        );
        assert_eq!(resolve("a/b/c", "../d"), "a/d");
    }
}
