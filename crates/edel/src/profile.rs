//! The power profile the kernel offers (roadmap M7.8) and the processor's
//! hint that goes with it (M7.16c). `power.on_battery` says how hard the
//! machine works; `edel power profile NAME` writes that to the kernel's
//! `platform_profile` file, which the firmware's own profile follows
//! (`low-power`, `balanced`, `performance`...), and to the processor's hint
//! of every cpufreq policy: its energy performance preference (EPP) where
//! the driver has one, else its governor. The kernel's names differ by
//! firmware, so each of our names maps to the first kernel name the machine
//! offers. Shell-ui asks for a profile when the charger or the settings
//! change, through `edel power profile`, which writes the files as root. A
//! machine that offers none of these has nothing here, and nothing is then
//! asked. Std only, so the desktop parts read it without `os`.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::battery_care;
use crate::i18n::{tr, trf};
use crate::places;

/// The kernel's power profile, one word, which the firmware follows.
pub const PLATFORM_PROFILE: &str = "/sys/firmware/acpi/platform_profile";

/// The profiles the kernel offers, separated by spaces, in the same folder.
pub const PLATFORM_PROFILE_CHOICES: &str = "/sys/firmware/acpi/platform_profile_choices";

/// The cpufreq folder: one `policyN` folder per group of CPUs, each holding
/// the processor's hints (M7.16c).
pub const CPUFREQ: &str = "/sys/devices/system/cpu/cpufreq";

/// The processor's energy performance preference, the hint of an
/// intel_pstate or amd_pstate policy, and the file that lists its names.
const EPP: &str = "energy_performance_preference";
const EPP_OFFERED: &str = "energy_performance_available_preferences";

/// The governor of a policy, and the file that lists the governors it offers.
const GOVERNOR: &str = "scaling_governor";
const GOVERNOR_OFFERED: &str = "scaling_available_governors";

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

/// The EPP names a profile of ours may take (M7.16c). Balanced takes only
/// `balance_performance`: the kernel reads back the name it was given, so
/// `default` would never look settled.
fn epp_names(ours: &str) -> &'static [&'static str] {
    match ours {
        "power-saver" => &["power", "balance_power"],
        "balanced" => &["balance_performance"],
        "performance" => &["performance"],
        _ => &[],
    }
}

/// The governors a profile of ours may take on a machine with no EPP
/// (M7.16c). The `powersave` governor pins the lowest speed, which makes the
/// desktop slow, so power-saver leaves the speed to `schedutil` (principle
/// Instant).
fn governor_names(ours: &str) -> &'static [&'static str] {
    match ours {
        "power-saver" | "balanced" => &["schedutil"],
        "performance" => &["performance"],
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

/// The cpufreq policies under `cpufreq`, one folder each, sorted by name.
fn policies(cpufreq: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(cpufreq) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_dir()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("policy"))
        })
        .collect();
    found.sort();
    found
}

/// The processor hint the policies take: EPP when some policy offers it,
/// else the governor. Returns the hint's file name and the file that lists
/// the values it may take.
fn hint_files(policies: &[PathBuf]) -> (&'static str, &'static str) {
    if policies
        .iter()
        .any(|policy| policy.join(EPP_OFFERED).exists())
    {
        (EPP, EPP_OFFERED)
    } else {
        (GOVERNOR, GOVERNOR_OFFERED)
    }
}

/// The values of the processor hint `file` that a profile of ours may take.
fn hint_names(file: &str, ours: &str) -> &'static [&'static str] {
    if file == EPP {
        epp_names(ours)
    } else {
        governor_names(ours)
    }
}

/// The policies a processor hint `(file, value)` is written to: each policy
/// that has the file, except that an EPP other than `performance` is not
/// written to a policy whose governor is `performance`, because intel_pstate
/// refuses it there (EBUSY). That policy keeps its state (M7.16c).
fn targets(cpufreq: &Path, (file, value): (&str, &str)) -> Vec<PathBuf> {
    policies(cpufreq)
        .into_iter()
        .filter(|policy| {
            if !policy.join(file).exists() {
                return false;
            }
            let pinned = file == EPP
                && value != "performance"
                && read_trimmed(&policy.join(GOVERNOR)).as_deref() == Some("performance");
            !pinned
        })
        .collect()
}

/// The processor hint the first policy offers for our profile `ours`: the
/// file's name and the value, or none when the policy lists none of ours.
fn processor_at(cpufreq: &Path, ours: &str) -> Option<(&'static str, String)> {
    let policies = policies(cpufreq);
    let first = policies.first()?;
    let (file, listed_in) = hint_files(&policies);
    let listed = fs::read_to_string(first.join(listed_in)).unwrap_or_default();
    let names = offered(&listed);
    let value = hint_names(file, ours)
        .iter()
        .copied()
        .find(|want| names.contains(want))?;
    Some((file, value.to_string()))
}

/// What a profile of ours writes on this machine (M7.16c): the kernel's
/// platform profile name, and the processor hint's file name and value for
/// every cpufreq policy. None where the machine offers none of the names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub platform: Option<String>,
    pub processor: Option<(&'static str, String)>,
}

/// The plan for `ours` from the files at `profile`, `choices` and
/// `cpufreq`. The platform part needs the profile file to write to; a
/// processor part only where some policy takes the hint.
fn plan_at(profile: &Path, choices: &Path, cpufreq: &Path, ours: &str) -> Plan {
    let platform = if profile.exists() {
        fs::read_to_string(choices)
            .ok()
            .and_then(|listed| kernel_profile(ours, &offered(&listed)).map(String::from))
    } else {
        None
    };
    let processor = processor_at(cpufreq, ours)
        .filter(|hint| !targets(cpufreq, (hint.0, hint.1.as_str())).is_empty());
    Plan {
        platform,
        processor,
    }
}

/// The plan for `ours` on this computer.
pub fn plan(ours: &str) -> Plan {
    plan_at(
        Path::new(PLATFORM_PROFILE),
        Path::new(PLATFORM_PROFILE_CHOICES),
        Path::new(CPUFREQ),
        ours,
    )
}

/// Whether every part of the plan for `ours` already holds: the platform
/// file says the plan's name, and every policy the plan writes to says the
/// plan's value. A plan with no part is settled.
fn settled_at(profile: &Path, choices: &Path, cpufreq: &Path, ours: &str) -> bool {
    let plan = plan_at(profile, choices, cpufreq, ours);
    let platform = plan
        .platform
        .as_deref()
        .is_none_or(|name| read_trimmed(profile).as_deref() == Some(name));
    let processor = plan.processor.as_ref().is_none_or(|hint| {
        let (file, value) = (hint.0, hint.1.as_str());
        targets(cpufreq, (file, value))
            .iter()
            .all(|policy| read_trimmed(&policy.join(file)).as_deref() == Some(value))
    });
    platform && processor
}

/// Whether this computer offers a platform profile or a processor hint, so
/// a game on the charger can be given full power (M7.7b, M7.16c).
pub fn offers_any() -> bool {
    let plan = plan("balanced");
    plan.platform.is_some() || plan.processor.is_some()
}

/// Whether `edel power profile ours` has something to do now: the plan is
/// not settled, or a trip's full charge is pending while the computer is
/// unplugged, which the same command ends (M7.16c).
pub fn needs_apply(ours: &str) -> bool {
    needs_apply_at(
        Path::new(PLATFORM_PROFILE),
        Path::new(PLATFORM_PROFILE_CHOICES),
        Path::new(CPUFREQ),
        Path::new(battery_care::POWER_SUPPLY),
        Path::new(places::CHARGE_FULL_ONCE),
        ours,
    )
}

/// [`needs_apply`] for the given files and folders.
fn needs_apply_at(
    profile: &Path,
    choices: &Path,
    cpufreq: &Path,
    power_supply: &Path,
    mark: &Path,
    ours: &str,
) -> bool {
    !settled_at(profile, choices, cpufreq, ours)
        || (mark.exists() && !battery_care::plugged_in(power_supply))
}

/// The processor hint the first policy shows now: the hint's file name, its
/// value and the values it offers. None on a machine with no cpufreq
/// policy, or whose policy has no hint.
pub fn processor_now() -> Option<(String, String, Vec<String>)> {
    let policies = policies(Path::new(CPUFREQ));
    let first = policies.first()?;
    let (file, listed_in) = hint_files(&policies);
    let value = read_trimmed(&first.join(file))?;
    let listed = fs::read_to_string(first.join(listed_in)).unwrap_or_default();
    let names = offered(&listed).into_iter().map(String::from).collect();
    Some((file.to_string(), value, names))
}

/// Writes the plan for `ours`: the platform profile, then each policy's
/// processor hint, each file only when it differs from the plan. A write
/// that fails does not stop the others; the first error is returned, and
/// the plan when all went well. A machine with no part to write is an error
/// that changes nothing.
pub fn apply(ours: &str) -> Result<Plan> {
    apply_at(
        Path::new(PLATFORM_PROFILE),
        Path::new(PLATFORM_PROFILE_CHOICES),
        Path::new(CPUFREQ),
        ours,
    )
}

/// [`apply`] for the files at `profile`, `choices` and `cpufreq`.
fn apply_at(profile: &Path, choices: &Path, cpufreq: &Path, ours: &str) -> Result<Plan> {
    let plan = plan_at(profile, choices, cpufreq, ours);
    if plan.platform.is_none() && plan.processor.is_none() {
        bail!(tr(
            "this computer offers no power profiles and no processor hints, so nothing is changed"
        ));
    }
    let mut results = Vec::new();
    if let Some(name) = &plan.platform {
        if read_trimmed(profile).as_deref() != Some(name.as_str()) {
            results.push(write_value(profile, name));
        }
    }
    if let Some(hint) = &plan.processor {
        for policy in targets(cpufreq, (hint.0, hint.1.as_str())) {
            let path = policy.join(hint.0);
            if read_trimmed(&path).as_deref() != Some(hint.1.as_str()) {
                results.push(write_value(&path, &hint.1));
            }
        }
    }
    results.into_iter().collect::<Result<()>>()?;
    Ok(plan)
}

/// Writes `value` as one line to the file at `path`. A refusal names the
/// file and says to run as root.
fn write_value(path: &Path, value: &str) -> Result<()> {
    let shown = path.display().to_string();
    fs::write(path, format!("{value}\n")).with_context(|| {
        trf(
            "could not set the power profile through {path}; this changes the system, so run it as root with doas",
            &[("path", &shown)],
        )
    })
}

/// A file's text without its line end, or none when it cannot be read.
fn read_trimmed(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|text| text.trim().to_string())
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

    /// Writes `text` to `path`, its folders made first.
    fn put(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    /// The text of a file the test wrote.
    fn text(path: &Path) -> String {
        fs::read_to_string(path).unwrap()
    }

    /// Policy `n` under `cpufreq` with the EPP names `offered`, the EPP
    /// `epp` and the governor `governor`.
    fn epp_policy(cpufreq: &Path, n: u32, offered: &str, epp: &str, governor: &str) {
        let folder = cpufreq.join(format!("policy{n}"));
        put(&folder.join(EPP_OFFERED), &format!("{offered}\n"));
        put(&folder.join(EPP), &format!("{epp}\n"));
        put(&folder.join(GOVERNOR), &format!("{governor}\n"));
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
    fn a_platform_profile_and_epp_are_both_written_to_every_policy() {
        let dir = scratch("both");
        let profile = dir.join("platform_profile");
        let choices = dir.join("platform_profile_choices");
        let cpufreq = dir.join("cpufreq");
        put(&profile, "balanced\n");
        put(&choices, "low-power balanced performance\n");
        let offered = "power balance_power balance_performance performance";
        epp_policy(&cpufreq, 0, offered, "balance_performance", "powersave");
        epp_policy(&cpufreq, 1, offered, "balance_performance", "powersave");

        let want = Plan {
            platform: Some("low-power".to_string()),
            processor: Some((EPP, "power".to_string())),
        };
        assert_eq!(plan_at(&profile, &choices, &cpufreq, "power-saver"), want);
        assert!(!settled_at(&profile, &choices, &cpufreq, "power-saver"));
        let written = apply_at(&profile, &choices, &cpufreq, "power-saver").unwrap();
        assert_eq!(written, want);
        assert_eq!(text(&profile), "low-power\n");
        for n in 0..2 {
            let hint = cpufreq.join(format!("policy{n}")).join(EPP);
            assert_eq!(text(&hint), "power\n", "policy {n} takes the hint");
        }
        assert!(settled_at(&profile, &choices, &cpufreq, "power-saver"));

        apply_at(&profile, &choices, &cpufreq, "balanced").unwrap();
        assert_eq!(text(&profile), "balanced\n");
        assert_eq!(
            text(&cpufreq.join("policy1").join(EPP)),
            "balance_performance\n"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn balanced_takes_only_balance_performance_as_its_epp() {
        let dir = scratch("balanced-epp");
        let cpufreq = dir.join("cpufreq");
        epp_policy(&cpufreq, 0, "default power", "power", "powersave");
        let plan = plan_at(
            &dir.join("platform_profile"),
            &dir.join("platform_profile_choices"),
            &cpufreq,
            "balanced",
        );
        assert_eq!(plan.processor, None, "default is never the name we want");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_machine_with_only_the_processor_hint_takes_its_epp() {
        let dir = scratch("epp-only");
        let profile = dir.join("platform_profile");
        let choices = dir.join("platform_profile_choices");
        let cpufreq = dir.join("cpufreq");
        epp_policy(
            &cpufreq,
            0,
            "power balance_performance performance",
            "balance_performance",
            "powersave",
        );
        let plan = apply_at(&profile, &choices, &cpufreq, "power-saver").unwrap();
        assert_eq!(plan.platform, None, "no firmware file, no platform part");
        assert_eq!(text(&cpufreq.join("policy0").join(EPP)), "power\n");
        assert!(!profile.exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_machine_with_only_governors_takes_the_governor() {
        let dir = scratch("governors");
        let profile = dir.join("platform_profile");
        let choices = dir.join("platform_profile_choices");
        let cpufreq = dir.join("cpufreq");
        let folder = cpufreq.join("policy0");
        put(
            &folder.join(GOVERNOR_OFFERED),
            "performance powersave schedutil\n",
        );
        put(&folder.join(GOVERNOR), "powersave\n");

        apply_at(&profile, &choices, &cpufreq, "performance").unwrap();
        assert_eq!(text(&folder.join(GOVERNOR)), "performance\n");
        apply_at(&profile, &choices, &cpufreq, "balanced").unwrap();
        assert_eq!(text(&folder.join(GOVERNOR)), "schedutil\n");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_policy_pinned_to_performance_keeps_its_state_for_a_lower_epp() {
        let dir = scratch("pinned");
        let profile = dir.join("platform_profile");
        let choices = dir.join("platform_profile_choices");
        let cpufreq = dir.join("cpufreq");
        let offered = "power balance_performance performance";
        epp_policy(&cpufreq, 0, offered, "performance", "performance");
        epp_policy(&cpufreq, 1, offered, "balance_performance", "powersave");

        assert!(!settled_at(&profile, &choices, &cpufreq, "power-saver"));
        apply_at(&profile, &choices, &cpufreq, "power-saver").unwrap();
        let pinned = cpufreq.join("policy0");
        assert_eq!(text(&pinned.join(EPP)), "performance\n", "left as it was");
        assert_eq!(text(&pinned.join(GOVERNOR)), "performance\n");
        assert_eq!(text(&cpufreq.join("policy1").join(EPP)), "power\n");
        assert!(settled_at(&profile, &choices, &cpufreq, "power-saver"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_machine_with_neither_plans_nothing_and_writes_nothing() {
        let dir = scratch("neither");
        let profile = dir.join("platform_profile");
        let choices = dir.join("platform_profile_choices");
        let cpufreq = dir.join("cpufreq");
        assert_eq!(
            plan_at(&profile, &choices, &cpufreq, "balanced"),
            Plan {
                platform: None,
                processor: None
            }
        );
        assert!(settled_at(&profile, &choices, &cpufreq, "balanced"));
        let error = apply_at(&profile, &choices, &cpufreq, "balanced")
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("offers no power profiles and no processor hints"),
            "{error}"
        );
        assert!(!profile.exists() && !cpufreq.exists(), "nothing is written");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_settled_machine_needs_nothing_unless_a_trip_charge_ends() {
        let dir = scratch("needs");
        let profile = dir.join("platform_profile");
        let choices = dir.join("platform_profile_choices");
        let cpufreq = dir.join("cpufreq");
        let power = dir.join("power_supply");
        let mark = dir.join("charge-full-once");
        put(&profile, "balanced\n");
        put(&choices, "balanced performance\n");
        let ask = |ours: &str| needs_apply_at(&profile, &choices, &cpufreq, &power, &mark, ours);
        assert!(!ask("balanced"), "settled, no mark");
        assert!(
            ask("performance"),
            "the platform profile is not the wanted one"
        );

        put(&mark, "");
        put(&power.join("AC").join("type"), "Mains\n");
        put(&power.join("AC").join("online"), "0\n");
        assert!(ask("balanced"), "a trip's charge ends when unplugged");
        put(&power.join("AC").join("online"), "1\n");
        assert!(!ask("balanced"), "plugged in, the trip goes on");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_processor_hint_is_shown_for_the_first_policy() {
        let dir = scratch("shown");
        let cpufreq = dir.join("cpufreq");
        epp_policy(
            &cpufreq,
            0,
            "power balance_performance",
            "power",
            "powersave",
        );
        let policies = policies(&cpufreq);
        let (file, listed_in) = hint_files(&policies);
        assert_eq!((file, listed_in), (EPP, EPP_OFFERED));
        let value = read_trimmed(&policies[0].join(file));
        assert_eq!(value.as_deref(), Some("power"));
        fs::remove_dir_all(&dir).unwrap();
    }
}
