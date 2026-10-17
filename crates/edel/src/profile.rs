//! The power profile the kernel offers (roadmap M7.8): `power.on_battery`
//! says how hard the machine works, and this module writes that to the
//! kernel's `platform_profile` file, which the firmware's own profile
//! follows (`low-power`, `balanced`, `performance`...). The kernel's names
//! differ by firmware, so each of our names maps to the first kernel name
//! the machine offers. Shell-ui asks for a profile when the charger or the
//! settings change, through `edel power profile`, which writes the file as
//! root. A machine whose kernel offers no profiles has no file, and nothing
//! here is then asked. Std only, so the desktop parts read it without `os`.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::i18n::{tr, trf};

/// The kernel's power profile, one word, which the firmware follows.
pub const PLATFORM_PROFILE: &str = "/sys/firmware/acpi/platform_profile";

/// The profiles the kernel offers, separated by spaces, in the same folder.
pub const PLATFORM_PROFILE_CHOICES: &str = "/sys/firmware/acpi/platform_profile_choices";

/// The kernel names a profile of ours may take, first to last: the first
/// one the machine offers is written. `power-saver` is the firmware's
/// quietest, and `performance` its fastest, with balanced-performance as
/// the next best.
fn preferred(ours: &str) -> &'static [&'static str] {
    match ours {
        "power-saver" => &["low-power", "quiet", "cool"],
        "balanced" => &["balanced"],
        "performance" => &["performance", "balanced-performance"],
        _ => &[],
    }
}

/// The kernel's name for our profile `ours` among the `offered` ones; none
/// when the machine offers none of the names we may take.
pub fn kernel_profile<'a>(ours: &str, offered: &[&'a str]) -> Option<&'a str> {
    preferred(ours)
        .iter()
        .find_map(|want| offered.iter().copied().find(|name| name == want))
}

/// The profiles a choices file lists, split on whitespace.
pub fn offered(text: &str) -> Vec<&str> {
    text.split_whitespace().collect()
}

/// The profile the kernel uses now and the ones it offers, read from the
/// two files; none when the machine has no profile file.
pub fn now() -> Option<(String, Vec<String>)> {
    now_at(
        Path::new(PLATFORM_PROFILE),
        Path::new(PLATFORM_PROFILE_CHOICES),
    )
}

/// [`now`] for the files at `profile` and `choices`.
fn now_at(profile: &Path, choices: &Path) -> Option<(String, Vec<String>)> {
    let current = fs::read_to_string(profile).ok()?;
    let listed = fs::read_to_string(choices).ok()?;
    let names = offered(&listed).into_iter().map(String::from).collect();
    Some((current.trim().to_string(), names))
}

/// Sets the kernel's profile for our profile `ours` and returns the kernel's
/// name for it. Errors say what failed and what to do; the profile is left
/// as it was in each.
pub fn set(ours: &str) -> Result<String> {
    set_at(
        Path::new(PLATFORM_PROFILE),
        Path::new(PLATFORM_PROFILE_CHOICES),
        ours,
    )
}

/// [`set`] for the files at `profile` and `choices`.
fn set_at(profile: &Path, choices: &Path, ours: &str) -> Result<String> {
    let listed = match fs::read_to_string(choices) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            let path = choices.display().to_string();
            bail!(trf(
                "this computer has no power profiles: {path} is missing, so the power profile is not changed",
                &[("path", &path)]
            ));
        }
        Err(err) => {
            let path = choices.display().to_string();
            let why = err.to_string();
            bail!(trf(
                "could not read {path}: {why}; the power profile is not changed",
                &[("path", &path), ("why", &why)]
            ));
        }
    };
    let names = offered(&listed);
    let Some(kernel) = kernel_profile(ours, &names) else {
        if names.is_empty() {
            bail!(tr(
                "this computer offers no power profiles, so the power profile is not changed"
            ));
        }
        let offers = names.join(" ");
        bail!(trf(
            "this computer has no power profile for {ours} (it offers {offers}), so the power profile is not changed",
            &[("ours", ours), ("offers", &offers)]
        ));
    };
    let path = profile.display().to_string();
    fs::write(profile, format!("{kernel}\n")).with_context(|| {
        trf(
            "could not set the power profile through {path}; this changes the system, so run it as root with doas",
            &[("path", &path)],
        )
    })?;
    Ok(kernel.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An empty directory under the temp dir, named after the test and
    /// this process.
    fn scratch(test: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("edel-profile-{test}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn our_names_take_the_first_kernel_name_offered() {
        let firmware = ["balanced", "cool", "performance"];
        assert_eq!(kernel_profile("power-saver", &firmware), Some("cool"));
        assert_eq!(kernel_profile("balanced", &firmware), Some("balanced"));
        assert_eq!(
            kernel_profile("performance", &firmware),
            Some("performance")
        );
        let laptop = ["low-power", "balanced", "balanced-performance"];
        assert_eq!(kernel_profile("power-saver", &laptop), Some("low-power"));
        assert_eq!(
            kernel_profile("performance", &["balanced-performance"]),
            Some("balanced-performance")
        );
    }

    #[test]
    fn no_offered_name_is_none() {
        assert_eq!(kernel_profile("balanced", &["quiet"]), None);
        assert_eq!(kernel_profile("balanced", &[]), None);
        assert_eq!(kernel_profile("turbo", &["balanced"]), None);
    }

    #[test]
    fn the_choices_file_splits_on_spaces_and_newlines() {
        assert_eq!(
            offered("low-power balanced performance\n"),
            ["low-power", "balanced", "performance"]
        );
        assert!(offered("\n").is_empty());
    }

    #[test]
    fn reads_the_profile_now_and_what_is_offered() {
        let dir = scratch("reads");
        let profile = dir.join("platform_profile");
        let choices = dir.join("platform_profile_choices");
        fs::write(&profile, "balanced\n").unwrap();
        fs::write(&choices, "low-power balanced\n").unwrap();
        assert_eq!(
            now_at(&profile, &choices),
            Some((
                "balanced".to_string(),
                vec!["low-power".to_string(), "balanced".to_string()]
            ))
        );
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(now_at(&profile, &choices), None, "no files, no profile");
    }

    #[test]
    fn setting_writes_the_kernel_name() {
        let dir = scratch("setting");
        let profile = dir.join("platform_profile");
        let choices = dir.join("platform_profile_choices");
        fs::write(&profile, "balanced\n").unwrap();
        fs::write(&choices, "low-power balanced performance\n").unwrap();
        assert_eq!(
            set_at(&profile, &choices, "power-saver").unwrap(),
            "low-power"
        );
        assert_eq!(fs::read_to_string(&profile).unwrap(), "low-power\n");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn setting_with_no_file_says_the_computer_has_no_profiles() {
        let dir = scratch("no-file");
        let profile = dir.join("platform_profile");
        let choices = dir.join("platform_profile_choices");
        let error = set_at(&profile, &choices, "balanced")
            .unwrap_err()
            .to_string();
        assert!(error.contains("has no power profiles"), "{error}");
        assert!(!profile.exists(), "nothing is written");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn setting_a_name_the_machine_lacks_names_what_it_offers() {
        let dir = scratch("lacks");
        let profile = dir.join("platform_profile");
        let choices = dir.join("platform_profile_choices");
        fs::write(&profile, "balanced\n").unwrap();
        fs::write(&choices, "quiet\n").unwrap();
        let error = set_at(&profile, &choices, "performance")
            .unwrap_err()
            .to_string();
        assert!(error.contains("it offers quiet"), "{error}");
        assert_eq!(fs::read_to_string(&profile).unwrap(), "balanced\n");
        fs::remove_dir_all(&dir).unwrap();
    }
}
