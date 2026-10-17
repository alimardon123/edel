//! Platform levels (ADR-005, roadmap M8.8): the numbered contracts of what
//! apps can count on. Each level is `platform/level-N.toml` at the
//! repository root, built in with `include_str!` and listed only here. An
//! image promises [`LEVEL`], and `edel image build` refuses one that falls
//! below it ([`shortfalls`]); `edel platform status` shows a machine's level
//! ([`status`]). Pure functions only: the files are read by the callers.

use std::cmp::Ordering;

use serde::Deserialize;

use crate::i18n::{tr, trf};
use crate::version;

/// The built-in levels, by number, in order from 1.
const BUILT_IN: &[(u32, &str)] = &[(1, include_str!("../../../platform/level-1.toml"))];

/// The level the images built now promise. A new level is a file, a line
/// in [`BUILT_IN`] and this number raised with it; a level only rises.
pub const LEVEL: u32 = 1;

/// The format of `platform/level-N.toml`.
pub const FORMAT: u32 = 1;

/// The key `os-release` carries the level under, so a machine says which
/// level it keeps.
pub const OS_RELEASE_KEY: &str = "EDEL_PLATFORM_LEVEL";

/// One level, as `platform/level-N.toml` writes it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Level {
    pub format: u32,
    /// The oldest kernel an image of this level boots, as dotted numbers.
    pub kernel: String,
    /// The runtimes apps of this level run on.
    pub runtimes: Vec<String>,
    /// The app platform's packages, each at least its version.
    #[serde(default)]
    pub packages: Vec<Floor>,
}

/// A package the level promises at least a version of, wherever the
/// `feature` is in an image.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Floor {
    pub name: String,
    pub at_least: String,
    pub feature: String,
}

/// The built-in level `number`, or none for a number this release does
/// not have. A test checks that every built-in level parses.
pub fn level(number: u32) -> Option<Level> {
    let (_, text) = BUILT_IN.iter().find(|(n, _)| *n == number)?;
    toml::from_str(text).ok()
}

/// Whether `version` is at least `floor`. The part after the first `-`
/// is cut off first: apk's release (`-r0`) and a kernel's flavour
/// (`-0-lts`) are not part of the number.
pub fn at_least(version: &str, floor: &str) -> bool {
    let number = short(version);
    version::compare(number, floor) != Ordering::Less
}

/// A version without its `-` suffix, as people read it.
fn short(version: &str) -> &str {
    version.split('-').next().unwrap_or(version)
}

/// Splits one line of the packages file (`name-version`, apk's version
/// ending in `-rN`) into its name and its version. The name may hold
/// dashes of its own, so the version is the last two dash parts.
pub fn split_package(line: &str) -> Option<(&str, &str)> {
    let mut parts = line.rsplitn(3, '-');
    parts.next()?;
    parts.next()?;
    let name = parts.next()?;
    Some((name, &line[name.len() + 1..]))
}

/// What the image lacks to keep level `number`, one message each; none
/// when it keeps the level. `packages` are the image's (name, version);
/// `kernel_package` is the kernel's package name, when the image has a
/// kernel; `features` are the feature names it is built of. Pure.
pub fn shortfalls(
    number: u32,
    level: &Level,
    packages: &[(&str, &str)],
    kernel_package: Option<&str>,
    features: &[&str],
) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(name) = kernel_package {
        match packages.iter().find(|(n, _)| *n == name) {
            None => out.push(format!(
                "the image has no {name:?} package, which platform level {number}'s kernel floor is read from; keep vm.kernel and its package together"
            )),
            Some((_, version)) if !at_least(version, &level.kernel) => {
                let kernel = &level.kernel;
                out.push(format!(
                    "{name:?} {version:?} is older than platform level {number}'s kernel floor {kernel:?}; build from a newer Alpine branch, as the level only rises"
                ));
            }
            Some(_) => {}
        }
    }
    for floor in &level.packages {
        let name = floor.name.as_str();
        match packages.iter().find(|(n, _)| *n == name) {
            Some((_, version)) if !at_least(version, &floor.at_least) => {
                let at_least = &floor.at_least;
                out.push(format!(
                    "{name:?} {version:?} is older than platform level {number} allows ({at_least:?} or newer); build from a newer Alpine branch, as the level only rises"
                ));
            }
            Some(_) => {}
            None if features.contains(&floor.feature.as_str()) => {
                let feature = &floor.feature;
                out.push(format!(
                    "the {feature:?} feature is in the image but {name:?} is not, and platform level {number} promises it; find the package's new name and change platform/level-{number}.toml and the feature together"
                ));
            }
            None => {}
        }
    }
    out
}

/// The rows `edel platform status` shows for a machine: its kernel, each
/// floor's package and the runtimes, as (label, value). Pure.
pub fn describe(
    level: &Level,
    kernel: Option<&str>,
    packages: &[(&str, &str)],
) -> Vec<(String, String)> {
    let kernel_row = match kernel {
        Some(version) => trf(
            "{version} ({floor} or newer)",
            &[("version", short(version)), ("floor", &level.kernel)],
        ),
        None => tr("not on this system").to_string(),
    };
    let mut rows = vec![(tr("kernel").to_string(), kernel_row)];
    for floor in &level.packages {
        let value = match packages.iter().find(|(n, _)| *n == floor.name) {
            Some((_, version)) => trf(
                "{version} ({floor} or newer)",
                &[("version", short(version)), ("floor", &floor.at_least)],
            ),
            None => trf(
                "not on this system ({floor} or newer where it is)",
                &[("floor", &floor.at_least)],
            ),
        };
        rows.push((floor.name.clone(), value));
    }
    rows.push((tr("runtimes").to_string(), level.runtimes.join(", ")));
    rows
}

/// The lines `edel platform status` prints. `level` is the os-release
/// value (none when absent or not a number), `packages` the text of the
/// packages file and `kernel` the kernel's release, trimmed. Pure.
pub fn status(level: Option<u32>, packages: &str, kernel: Option<&str>) -> Vec<String> {
    let number = match level {
        None | Some(0) => {
            return vec![
                tr("This system promises no platform level: it was built before level 1")
                    .to_string(),
            ];
        }
        Some(number) => number,
    };
    let Some(known) = self::level(number) else {
        return vec![trf(
            "This system promises platform level {n}, which this edel does not know",
            &[("n", &number.to_string())],
        )];
    };
    let installed: Vec<(&str, &str)> = packages.lines().filter_map(split_package).collect();
    let rows = describe(&known, kernel, &installed);
    // Counted in characters, so a translated label lines up too.
    let width = rows
        .iter()
        .map(|(label, _)| label.chars().count())
        .max()
        .unwrap_or(0);
    let mut out = vec![trf(
        "Platform level {n}: what apps can count on here, kept by every later release",
        &[("n", &number.to_string())],
    )];
    for (label, value) in rows {
        let pad = " ".repeat(width - label.chars().count());
        out.push(format!("  {label}{pad}  {value}"));
    }
    out.push(tr("The contract: docs/platform-levels.md").to_string());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dotted(text: &str) -> bool {
        !text.is_empty()
            && text
                .split('.')
                .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
    }

    /// A level for the rules, without the file.
    fn sample(packages: Vec<Floor>) -> Level {
        Level {
            format: FORMAT,
            kernel: "6.18".into(),
            runtimes: vec!["org.freedesktop.Platform".into()],
            packages,
        }
    }

    fn floor(name: &str, at_least: &str, feature: &str) -> Floor {
        Floor {
            name: name.into(),
            at_least: at_least.into(),
            feature: feature.into(),
        }
    }

    #[test]
    fn level_is_the_newest_built_in() {
        assert_eq!(BUILT_IN.last().map(|(n, _)| *n), Some(LEVEL));
        for (index, (number, _)) in BUILT_IN.iter().enumerate() {
            assert_eq!(*number, index as u32 + 1);
        }
    }

    #[test]
    fn every_built_in_level_parses_with_its_format_and_dotted_versions() {
        for (number, _) in BUILT_IN {
            let level = level(*number).unwrap_or_else(|| panic!("level {number} does not parse"));
            assert_eq!(level.format, FORMAT, "level {number}");
            assert!(
                dotted(&level.kernel),
                "level {number} kernel {:?}",
                level.kernel
            );
            for floor in &level.packages {
                assert!(
                    dotted(&floor.at_least),
                    "{} at_least {:?}",
                    floor.name,
                    floor.at_least
                );
            }
        }
        assert!(level(0).is_none() && level(LEVEL + 1).is_none());
    }

    #[test]
    fn a_release_of_a_minor_version_is_at_least_that_minor() {
        assert!(at_least("6.18.57-r0", "6.18"));
        assert!(at_least("6.18-r0", "6.18"));
        assert!(at_least("6.18.57-0-lts", "6.18"));
        assert!(at_least("1.21.2-r0", "1.21"));
    }

    #[test]
    fn an_older_minor_version_is_below_the_floor() {
        assert!(!at_least("6.17.9-r0", "6.18"));
        assert!(!at_least("1.9.0-r1", "1.16"));
    }

    #[test]
    fn versions_compare_as_numbers_not_text() {
        assert!(at_least("1.16.6-r0", "1.9"));
        assert!(!at_least("1.9.0-r1", "1.16"));
    }

    #[test]
    fn a_package_line_splits_into_its_name_and_version() {
        assert_eq!(
            split_package("xdg-desktop-portal-gtk-1.15.3-r1"),
            Some(("xdg-desktop-portal-gtk", "1.15.3-r1"))
        );
        assert_eq!(
            split_package("linux-lts-6.18.57-r0"),
            Some(("linux-lts", "6.18.57-r0"))
        );
        assert_eq!(
            split_package("py3-6to4-1.0-r0"),
            Some(("py3-6to4", "1.0-r0"))
        );
    }

    #[test]
    fn a_line_without_two_dashes_is_no_package() {
        assert_eq!(split_package("flatpak"), None);
        assert_eq!(split_package("flatpak-1.16.6"), None);
    }

    #[test]
    fn an_image_that_keeps_the_level_has_no_shortfalls() {
        let level = sample(vec![floor("flatpak", "1.16", "flatpak")]);
        let packages = [("linux-lts", "6.18.57-r0"), ("flatpak", "1.16.6-r0")];
        let found = shortfalls(1, &level, &packages, Some("linux-lts"), &["flatpak"]);
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_missing_kernel_package_is_a_shortfall() {
        let level = sample(vec![]);
        let found = shortfalls(1, &level, &[], Some("linux-virt"), &[]);
        assert_eq!(
            found,
            vec![
                "the image has no \"linux-virt\" package, which platform level 1's kernel floor is read from; keep vm.kernel and its package together"
            ]
        );
    }

    #[test]
    fn an_old_kernel_is_a_shortfall() {
        let level = sample(vec![]);
        let found = shortfalls(
            1,
            &level,
            &[("linux-lts", "6.17.9-r0")],
            Some("linux-lts"),
            &[],
        );
        assert_eq!(
            found,
            vec![
                "\"linux-lts\" \"6.17.9-r0\" is older than platform level 1's kernel floor \"6.18\"; build from a newer Alpine branch, as the level only rises"
            ]
        );
    }

    #[test]
    fn an_old_floor_package_is_a_shortfall() {
        let level = sample(vec![floor("flatpak", "1.16", "flatpak")]);
        let found = shortfalls(
            1,
            &level,
            &[("linux-lts", "6.18.57-r0"), ("flatpak", "1.15.9-r0")],
            Some("linux-lts"),
            &["flatpak"],
        );
        assert_eq!(
            found,
            vec![
                "\"flatpak\" \"1.15.9-r0\" is older than platform level 1 allows (\"1.16\" or newer); build from a newer Alpine branch, as the level only rises"
            ]
        );
    }

    #[test]
    fn a_missing_package_is_a_shortfall_only_when_its_feature_is_in_the_image() {
        let level = sample(vec![floor("flatpak", "1.16", "flatpak")]);
        let found = shortfalls(1, &level, &[], None, &["flatpak"]);
        assert_eq!(
            found,
            vec![
                "the \"flatpak\" feature is in the image but \"flatpak\" is not, and platform level 1 promises it; find the package's new name and change platform/level-1.toml and the feature together"
            ]
        );
        assert!(shortfalls(1, &level, &[], None, &[]).is_empty());
    }

    #[test]
    fn a_status_of_no_level_or_level_zero_says_so() {
        let text = "This system promises no platform level: it was built before level 1";
        assert_eq!(status(None, "", Some("6.18.57-0-lts")), vec![text]);
        assert_eq!(status(Some(0), "", None), vec![text]);
    }

    #[test]
    fn a_status_of_an_unknown_level_says_so() {
        assert_eq!(
            status(Some(9), "", None),
            vec!["This system promises platform level 9, which this edel does not know"]
        );
    }

    #[test]
    fn a_status_of_level_one_shows_each_floor_and_the_contract() {
        let packages = "flatpak-1.16.6-r0\nlinux-lts-6.18.57-r0\nxdg-desktop-portal-1.21.2-r0\nxdg-desktop-portal-gtk-1.15.3-r1\n";
        let lines = status(Some(1), packages, Some("6.18.57-0-lts"));
        assert_eq!(
            lines,
            vec![
                "Platform level 1: what apps can count on here, kept by every later release",
                "  kernel                  6.18.57 (6.18 or newer)",
                "  flatpak                 1.16.6 (1.16 or newer)",
                "  xdg-desktop-portal      1.21.2 (1.21 or newer)",
                "  xdg-desktop-portal-gtk  1.15.3 (1.15 or newer)",
                "  xdg-desktop-portal-wlr  not on this system (0.8 or newer where it is)",
                "  runtimes                org.freedesktop.Platform",
                "The contract: docs/platform-levels.md",
            ]
        );
    }

    #[test]
    fn a_describe_row_for_a_missing_kernel_says_it_is_not_on_this_system() {
        let level = sample(vec![]);
        assert_eq!(
            describe(&level, None, &[]),
            vec![
                ("kernel".to_string(), "not on this system".to_string()),
                (
                    "runtimes".to_string(),
                    "org.freedesktop.Platform".to_string()
                ),
            ]
        );
    }
}
