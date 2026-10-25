//! The table of every feature in `docs/FEATURES.md` (the whole-system review
//! of 2026-10-25): written from `features/*.toml` and the image definitions
//! by a cargo test, so it never drifts from the files. It is test-only:
//! `#![cfg(test)]` keeps it out of the binary, and writing the attribute here
//! rather than as `#[cfg(test)]` in `main.rs` keeps `i18n`'s scan of that
//! file whole, as `hardware_table.rs` does.

#![cfg(test)]

use edel::features::{Feature, Services};
use std::fmt::Write;

/// An image as the table reads it: its name, its `features` and its
/// `[vm] carry`.
type Image = (String, Vec<String>, Vec<String>);

/// A table cell's text with its pipes escaped.
fn cell(text: &str) -> String {
    text.replace('|', "\\|")
}

/// The packages of a feature as a cell: each in backticks, joined by ", ",
/// or `none`; a feature made of parts says `its parts'`.
fn packages(feature: &Feature) -> String {
    if !feature.features.is_empty() {
        return "its parts'".to_string();
    }
    if feature.packages.is_empty() {
        return "none".to_string();
    }
    feature
        .packages
        .iter()
        .map(|p| format!("`{p}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The services of a feature as a cell: the runlevel groups separated by
/// "; ", the names in a group joined by ", ", or `none`; a feature made of
/// parts says `its parts'`.
fn services(feature: &Feature) -> String {
    if !feature.features.is_empty() {
        return "its parts'".to_string();
    }
    let Services {
        sysinit,
        boot,
        default,
        shutdown,
    } = &feature.services;
    let groups: Vec<String> = [sysinit, boot, default, shutdown]
        .into_iter()
        .filter(|group| !group.is_empty())
        .map(|group| group.join(", "))
        .collect();
    if groups.is_empty() {
        "none".to_string()
    } else {
        groups.join("; ")
    }
}

/// Where a feature is: the images that list it, its add-on form, the
/// add-ons made of it, and whether an image carries it, joined by ", ".
fn places(
    name: &str,
    feature: &Feature,
    features: &[(String, Feature)],
    images: &[Image],
) -> String {
    let mut phrases: Vec<String> = Vec::new();
    for (image, listed, _) in images {
        if listed.iter().any(|f| f == name) {
            phrases.push(image.clone());
        }
    }
    if feature.addon {
        phrases.push(match &feature.on {
            Some(base) => format!("add-on over `{base}`"),
            None => "add-on".to_string(),
        });
    }
    for (part_of, whole) in features {
        if whole.features.iter().any(|p| p == name) {
            phrases.push(format!("the `{part_of}` add-on"));
        }
    }
    if images
        .iter()
        .any(|(_, _, carry)| carry.iter().any(|c| c == name))
    {
        phrases.push("carried".to_string());
    }
    if phrases.is_empty() {
        "CI only".to_string()
    } else {
        phrases.join(", ")
    }
}

/// The table of `features`, given as (name, parsed feature) pairs sorted by
/// name, and `images`, given as (image name, its `features` list, its
/// `[vm] carry` list): one row per feature, then the
/// count. The table is what `docs/FEATURES.md` shows between its markers.
pub fn table(features: &[(String, Feature)], images: &[Image]) -> String {
    let mut images = images.to_vec();
    images.sort();
    let mut out = String::from(
        "| Feature | What it gives | Packages | Services | In |\n|---|---|---|---|---|\n",
    );
    for (name, feature) in features {
        let _ = writeln!(
            out,
            "| `{name}` | {} | {} | {} | {} |",
            cell(&feature.summary),
            cell(&packages(feature)),
            cell(&services(feature)),
            cell(&places(name, feature, features, &images)),
        );
    }
    let _ = writeln!(out, "\n{} features.", features.len());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const BEGIN: &str =
        "<!-- written by feature_table.rs: EDEL_WRITE_DOCS=1 cargo test -p edel feature_table -->";
    const END: &str = "<!-- end of the features -->";

    /// The byte range of the generated table in the features page: from the
    /// line after `BEGIN` to the start of `END`.
    fn generated(page: &str) -> Option<std::ops::Range<usize>> {
        let start = page.find(BEGIN)? + BEGIN.len();
        let start = start + page[start..].find('\n')? + 1;
        let end = start + page[start..].find(END)?;
        Some(start..end)
    }

    /// The names of a TOML array under `value`, or none when it is missing.
    fn list_of(value: Option<&toml::Value>) -> Vec<String> {
        value
            .and_then(toml::Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The features page's table is the feature files: written by
    /// `EDEL_WRITE_DOCS=1 cargo test -p edel feature_table`, and failing
    /// while the text between the markers differs.
    #[test]
    fn the_feature_table_is_the_files() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../features");
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".toml"))
            .collect();
        names.sort_by(|a, b| a.trim_end_matches(".toml").cmp(b.trim_end_matches(".toml")));
        let features: Vec<(String, Feature)> = names
            .iter()
            .map(|file| {
                let name = file.strip_suffix(".toml").unwrap().to_string();
                let text = fs::read_to_string(format!("{dir}/{file}")).unwrap();
                let feature = edel::features::check(&name, &text)
                    .unwrap_or_else(|e| panic!("features/{file}: {e:#}"));
                (name, feature)
            })
            .collect();

        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../images");
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".toml"))
            .collect();
        names.sort();
        let images: Vec<Image> = names
            .iter()
            .map(|file| {
                let name = file.strip_suffix(".toml").unwrap().to_string();
                let text = fs::read_to_string(format!("{dir}/{file}")).unwrap();
                let image: toml::Table =
                    toml::from_str(&text).unwrap_or_else(|e| panic!("images/{file}: {e:#}"));
                let carry = list_of(image.get("vm").and_then(|vm| vm.get("carry")));
                (name, list_of(image.get("features")), carry)
            })
            .collect();

        let text = table(&features, &images);
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/FEATURES.md");
        if std::env::var_os("EDEL_WRITE_DOCS").is_some() {
            let page = fs::read_to_string(path).unwrap();
            let span = generated(&page).expect("docs/FEATURES.md has no generated feature table");
            let written = format!("{}{}{}", &page[..span.start], text, &page[span.end..]);
            fs::write(path, written).unwrap();
        }
        let page = fs::read_to_string(path).unwrap();
        let span = generated(&page).expect("docs/FEATURES.md has no generated feature table");
        assert!(
            page[span] == text,
            "docs/FEATURES.md's feature table differs from features/*.toml; run EDEL_WRITE_DOCS=1 cargo test -p edel feature_table"
        );
    }
}
