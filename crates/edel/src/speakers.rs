//! Speaker tuning profiles (roadmap M6.15, ADR-012 section 1): makers tune
//! a laptop's speakers for one system only, so the same speakers sound thin
//! elsewhere. A profile is a PipeWire filter-chain file of a few kilobytes,
//! `NAME.conf` in `places::SPEAKERS_DIR`, and a matching table there,
//! `*.toml`, gives each model its profile by the names the machine's
//! firmware reports (DMI's `sys_vendor` and `product_name`). The base ships
//! them, so they work offline and on the stick; nothing is downloaded and
//! nothing about the machine is sent anywhere (ADR-009).
//!
//! `edel boot speakers` links the matching profile into PipeWire's own
//! `pipewire.conf.d` at boot (`places::SPEAKER_PROFILE`), so the session's
//! PipeWire loads it when it starts; a machine with no profile has no link.
//! Adding a model is one profile file and one line of a table, one PR,
//! which any person can write for their own laptop and send back.
//!
//! A table:
//!
//! ```toml
//! format = 1
//!
//! [[model]]
//! vendor = "HP"
//! product = "HP 250 G8 Notebook PC"
//! profile = "hp-250-g8"
//! ```

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::i18n::trf;

/// The tables' format this release reads.
pub const FORMAT: i64 = 1;

/// One line of a matching table: a model, as its firmware names it, and the
/// profile that tunes its speakers.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Model {
    /// DMI's `sys_vendor`, such as `HP`
    pub vendor: String,
    /// DMI's `product_name`, such as `HP 250 G8 Notebook PC`
    pub product: String,
    /// The profile's name: `NAME.conf` beside the table
    pub profile: String,
}

/// A table as it is written. Keys this release does not know are left
/// alone, as a newer release may add some (ADR-008).
#[derive(Deserialize)]
struct Table {
    format: i64,
    #[serde(default)]
    model: Vec<Model>,
}

/// Reads one matching table; a format this release does not know is
/// refused, as is text that is not a table.
pub fn read_table(text: &str) -> Result<Vec<Model>> {
    let table: Table = toml::from_str(text).context("the table is not valid TOML")?;
    if table.format != FORMAT {
        bail!(
            "{}",
            trf(
                "the table is format {format}, and this release reads format {current}",
                &[
                    ("format", &table.format.to_string()),
                    ("current", &FORMAT.to_string())
                ]
            )
        );
    }
    Ok(table.model)
}

/// Whether `name` may name a profile: lowercase letters, digits and `-`,
/// so it is one plain file name.
pub fn is_profile_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// What is wrong with a table in `dir`, strictly, for the build and the
/// tests: each line's names filled in, its profile a plain name whose file
/// is in `dir`, and no model twice.
pub fn check(text: &str, dir: &Path) -> Vec<String> {
    let models = match read_table(text) {
        Ok(models) => models,
        Err(err) => return vec![format!("{err:#}")],
    };
    let mut problems = Vec::new();
    for (at, model) in models.iter().enumerate() {
        let line = at + 1;
        if model.vendor.trim().is_empty() || model.product.trim().is_empty() {
            problems.push(format!("model {line}: vendor and product must be given"));
        }
        if !is_profile_name(&model.profile) {
            problems.push(format!(
                "model {line}: {:?} is not a profile name; use lowercase letters, digits and -",
                model.profile
            ));
        } else if !profile_path(dir, &model.profile).is_file() {
            problems.push(format!(
                "model {line}: there is no {}",
                profile_path(dir, &model.profile).display()
            ));
        }
        if models[..at]
            .iter()
            .any(|m| same(&m.vendor, &model.vendor) && same(&m.product, &model.product))
        {
            problems.push(format!(
                "model {line}: {} {} is listed twice",
                model.vendor, model.product
            ));
        }
    }
    problems
}

/// Two firmware names are one when they are equal but for the spaces
/// around them, which firmware often pads with.
fn same(a: &str, b: &str) -> bool {
    a.trim() == b.trim()
}

/// The profile for the machine named `vendor` and `product`: the first
/// line of `models` that names it.
pub fn profile_for<'a>(models: &'a [Model], vendor: &str, product: &str) -> Option<&'a str> {
    models
        .iter()
        .find(|m| same(&m.vendor, vendor) && same(&m.product, product))
        .map(|m| m.profile.as_str())
}

/// The machine's vendor and product as its firmware reports them, from
/// `sys` (`/sys`, or a test's tree); none when it reports neither.
pub fn machine_model(sys: &Path) -> Option<(String, String)> {
    let read = |name: &str| {
        fs::read_to_string(sys.join("class/dmi/id").join(name))
            .map(|v| v.trim().to_string())
            .unwrap_or_default()
    };
    let (vendor, product) = (read("sys_vendor"), read("product_name"));
    (!vendor.is_empty() || !product.is_empty()).then_some((vendor, product))
}

/// Every model the tables in `dir` list, the tables read in their names'
/// order (so `ci.toml` beside the base's comes in a known place); a table
/// that cannot be read is skipped with a note, never the machine's sound.
pub fn tables_in(dir: &Path) -> (Vec<Model>, Vec<String>) {
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "toml"))
                .collect()
        })
        .unwrap_or_default();
    paths.sort();
    let mut models = Vec::new();
    let mut notes = Vec::new();
    for path in paths {
        match fs::read_to_string(&path)
            .map_err(anyhow::Error::from)
            .and_then(|text| read_table(&text))
        {
            Ok(found) => models.extend(found),
            Err(err) => notes.push(format!("{} was skipped: {err:#}", path.display())),
        }
    }
    (models, notes)
}

/// The file of the profile `name` in `dir`.
pub fn profile_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.conf"))
}

/// The profile file this machine should use: its model's profile in `dir`,
/// when a table names one and its file is there.
pub fn wanted(models: &[Model], model: Option<&(String, String)>, dir: &Path) -> Option<PathBuf> {
    let (vendor, product) = model?;
    let name = profile_for(models, vendor, product)?;
    let path = profile_path(dir, name);
    (is_profile_name(name) && path.is_file()).then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLE: &str = r#"format = 1

[[model]]
vendor = "HP"
product = "HP 250 G8 Notebook PC"
profile = "hp-250-g8"

[[model]]
vendor = "LENOVO"
product = "20XW0055GE"
profile = "thinkpad-x1-9"
"#;

    fn temp(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("edel-{test}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn matches_sample_models_to_their_profiles_and_to_none() {
        let models = read_table(TABLE).unwrap();
        assert_eq!(
            profile_for(&models, "HP", "HP 250 G8 Notebook PC"),
            Some("hp-250-g8")
        );
        assert_eq!(
            profile_for(&models, "LENOVO ", " 20XW0055GE\n"),
            Some("thinkpad-x1-9"),
            "the firmware's padding does not count"
        );
        assert_eq!(profile_for(&models, "HP", "HP 255 G8 Notebook PC"), None);
        assert_eq!(
            profile_for(&models, "QEMU", "Standard PC (Q35 + ICH9, 2009)"),
            None
        );
        assert_eq!(profile_for(&[], "HP", "HP 250 G8 Notebook PC"), None);
    }

    #[test]
    fn a_table_of_another_format_or_not_toml_is_refused() {
        assert!(read_table("format = 2\n").is_err());
        assert!(read_table("format = \n").is_err());
        assert_eq!(read_table("format = 1\n").unwrap(), []);
        assert_eq!(
            read_table("format = 1\nlater = true\n").unwrap(),
            [],
            "a key of a later release is left alone"
        );
    }

    #[test]
    fn checks_names_files_and_models_twice() {
        let dir = temp("speakers-check");
        fs::write(dir.join("hp-250-g8.conf"), "").unwrap();
        let problems = check(TABLE, &dir);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("thinkpad-x1-9.conf"), "{problems:?}");
        fs::write(dir.join("thinkpad-x1-9.conf"), "").unwrap();
        assert!(check(TABLE, &dir).is_empty());
        let twice = format!(
            "{TABLE}\n[[model]]\nvendor = \"HP\"\nproduct = \"HP 250 G8 Notebook PC\"\nprofile = \"Bad Name\"\n"
        );
        let problems = check(&twice, &dir);
        assert_eq!(problems.len(), 2, "{problems:?}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn every_table_we_ship_checks() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for dir in [
            "features/sound/usr/share/edel/speakers",
            "ci/desktop/features/desktop-test/usr/share/edel/speakers",
        ] {
            let dir = root.join(dir);
            let (_, notes) = tables_in(&dir);
            assert!(notes.is_empty(), "{notes:?}");
            for entry in fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.extension().is_some_and(|e| e == "toml") {
                    let problems = check(&fs::read_to_string(&path).unwrap(), &dir);
                    assert!(problems.is_empty(), "{}: {problems:?}", path.display());
                }
            }
        }
    }

    #[test]
    fn reads_the_machine_and_every_table_and_wants_a_profile_that_is_there() {
        let dir = temp("speakers-tables");
        let sys = dir.join("sys");
        fs::create_dir_all(sys.join("class/dmi/id")).unwrap();
        assert_eq!(machine_model(&sys), None);
        fs::write(sys.join("class/dmi/id/sys_vendor"), "Edel\n").unwrap();
        fs::write(sys.join("class/dmi/id/product_name"), "Speaker Test\n").unwrap();
        let model = machine_model(&sys);
        assert_eq!(model, Some(("Edel".into(), "Speaker Test".into())));
        let tables = dir.join("speakers");
        fs::create_dir_all(&tables).unwrap();
        fs::write(tables.join("base.toml"), TABLE).unwrap();
        fs::write(
            tables.join("ci.toml"),
            "format = 1\n[[model]]\nvendor = \"Edel\"\nproduct = \"Speaker Test\"\nprofile = \"ci-test\"\n",
        )
        .unwrap();
        fs::write(tables.join("broken.toml"), "format = 9\n").unwrap();
        let (models, notes) = tables_in(&tables);
        assert_eq!(models.len(), 3);
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert_eq!(
            wanted(&models, model.as_ref(), &tables),
            None,
            "no file yet"
        );
        fs::write(tables.join("ci-test.conf"), "# test\n").unwrap();
        assert_eq!(
            wanted(&models, model.as_ref(), &tables),
            Some(tables.join("ci-test.conf"))
        );
        assert_eq!(wanted(&models, None, &tables), None);
        let _ = fs::remove_dir_all(&dir);
    }
}
