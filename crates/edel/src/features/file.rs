//! The feature file reader (roadmap M4.0), the system's own work behind
//! the library's `os` feature (M6.17): the desktop asks only which
//! features a machine has, by name, through [`super`].

use super::is_name;
use anyhow::{Context, Result, bail};
use serde::Deserialize;

/// The only feature format so far.
pub const FORMAT: u32 = 1;

/// One feature file.
#[derive(Debug, Default, Deserialize)]
pub struct Feature {
    pub format: u32,
    /// One line for people: Settings, `edel report`, release notes.
    pub summary: String,
    /// The written reason for its packages and services (principle 3).
    #[serde(default)]
    pub why: String,
    #[serde(default)]
    pub packages: Vec<String>,
    #[serde(default)]
    pub services: Services,
    /// Kernel modules the initramfs loads before it looks for the root
    /// (`modules=`); everything else loads by hardware ID.
    #[serde(default)]
    pub modules: Vec<String>,
    /// mkinitfs features the initramfs is built with.
    #[serde(default)]
    pub initramfs: Vec<String>,
    /// Its services may be turned off (`services.NAME = false`, and `off`
    /// in an image definition).
    #[serde(default)]
    pub switchable: bool,
    /// A settings key that switches this switchable feature (M7.5): its
    /// services run only while the key is set to anything but `off` or
    /// `false`, so the updates timer runs only while `updates.automatic`
    /// asks for something; `services.NAME` still wins.
    #[serde(default)]
    pub while_set: Option<String>,
    /// Also built as a signed add-on (M7.2a).
    #[serde(default)]
    pub addon: bool,
    /// The features an add-on is made of (M8.17c): the desktop is one
    /// add-on of many features, each still its own file, so each is added,
    /// changed or dropped alone.
    #[serde(default)]
    pub features: Vec<String>,
    /// The add-on this add-on is built over and mounted above (M8.17c):
    /// the screen reader over the desktop, whose libraries it uses.
    #[serde(default)]
    pub on: Option<String>,
    /// The hardware a hardware pack serves (M7.11, ADR-012): patterns of the
    /// kernel's modalias strings, `*` any run of characters and `?` one, as
    /// `/sys/bus/*/devices/*/modalias` and `/sys/class/dmi/id/modalias` show
    /// them (`pci:v000010DEd*bc03*`). Only an add-on has it.
    #[serde(default)]
    pub hardware: Vec<String>,
    /// Only for a machine with a screen (M8.17): where the kernel shows no
    /// graphics card, its services do not start unless the settings file
    /// switches it on, and its health is not waited for
    /// (`EDEL_HEALTH_SCREEN`), so one image serves desktops and servers.
    #[serde(default)]
    pub screen: bool,
    /// Health files it writes, which the boot guard waits for (M1.5).
    #[serde(default)]
    pub health: Vec<String>,
    /// Programs of this repository's workspace it ships in `/usr/bin`,
    /// such as `edel-compositor` (M4.2b); `edel image build` copies each
    /// from beside itself.
    #[serde(default)]
    pub programs: Vec<String>,
    /// What the image keeps of its packages' files under a folder (M5.30,
    /// `edel::keep`): the laptop feature's firmware, the fonts feature's
    /// fonts.
    #[serde(default)]
    pub keep: Vec<crate::keep::Keep>,
    /// The default apps (M6.2); only the `apps` feature has them.
    #[serde(default)]
    pub flatpak: Vec<String>,
    /// Default apps a release dropped, kept for those who used them.
    #[serde(default)]
    pub flatpak_dropped: Vec<String>,
    /// The Alpine branch, mirror and repositories every image is built
    /// from: only the `base` feature names them, so the branch is written
    /// once (M5.27).
    #[serde(default)]
    pub alpine: Option<Alpine>,
    /// The basics checklist's rows this feature brings (M6.7): each tested
    /// by a desktop-test case or marked as needing hardware or a later step.
    #[serde(default)]
    pub basics: Vec<Basic>,
}

/// One row of the basics checklist (M6.7, `ci/basics-test.sh`): a basic a
/// person expects to work, proved by the desktop test case `test`, or
/// marked `needs` hardware or the roadmap step that brings it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Basic {
    /// What the row says, such as `Audio`; at most 40 characters.
    pub name: String,
    /// The desktop-test case that proves it, a lowercase name such as `sound`.
    #[serde(default)]
    pub test: Option<String>,
    /// `hardware`, or the roadmap step that brings it, such as `M7.8`.
    #[serde(default)]
    pub needs: Option<String>,
}

/// Where an image's packages come from (ADR-003).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Alpine {
    /// The Alpine stable branch, such as `v3.24`.
    pub branch: String,
    pub mirror: String,
    pub repositories: Vec<String>,
    /// The digest of the multi-architecture `alpine` image of that branch
    /// (`sha256:` and 64 hex digits) that CI builds in, so a build never
    /// runs in a container nobody looked at (M3.9). Images do not use it.
    #[serde(default)]
    pub image_digest: Option<String>,
}

/// OpenRC services to enable, by runlevel.
#[derive(Debug, Default, Clone, PartialEq, Eq, Deserialize)]
pub struct Services {
    #[serde(default)]
    pub sysinit: Vec<String>,
    #[serde(default)]
    pub boot: Vec<String>,
    #[serde(default)]
    pub default: Vec<String>,
    #[serde(default)]
    pub shutdown: Vec<String>,
}

impl Services {
    /// Every (runlevel, service) pair, in boot order.
    pub fn entries(&self) -> Vec<(&'static str, &str)> {
        let levels: [(&'static str, &Vec<String>); 4] = [
            ("sysinit", &self.sysinit),
            ("boot", &self.boot),
            ("default", &self.default),
            ("shutdown", &self.shutdown),
        ];
        levels
            .into_iter()
            .flat_map(|(level, services)| services.iter().map(move |s| (level, s.as_str())))
            .collect()
    }

    /// Adds `service` to `level` unless it is there already.
    pub fn add(&mut self, level: &str, service: &str) {
        let list = match level {
            "sysinit" => &mut self.sysinit,
            "boot" => &mut self.boot,
            "default" => &mut self.default,
            _ => &mut self.shutdown,
        };
        if !list.iter().any(|s| s == service) {
            list.push(service.to_string());
        }
    }
}

/// Reads a feature file the way a machine does: unknown fields are
/// skipped and returned with a note, so a newer feature file never stops
/// an older reader (ADR-008).
pub fn read(text: &str) -> Result<(Feature, Vec<String>)> {
    let value: toml::Value = toml::from_str(text).context("parsing the feature file")?;
    let mut notes = Vec::new();
    let feature: Feature = serde_ignored::deserialize(value, |path| {
        notes.push(format!("unknown field {path} ignored"))
    })
    .context("reading the feature file")?;
    if feature.format > FORMAT {
        notes.push(format!(
            "format {} is newer than this edel's {FORMAT}; read what it understands",
            feature.format
        ));
    }
    Ok((feature, notes))
}

/// Reads and checks the feature file of feature `name` the way `edel image
/// build` and `image check` do: anything [`read`] would only note is
/// refused, and so is a package or service without a `why`.
pub fn check(name: &str, text: &str) -> Result<Feature> {
    if !is_name(name) {
        bail!("feature name {name:?} must use only a-z, 0-9 and '-'");
    }
    let (feature, notes) = read(text)?;
    if !notes.is_empty() {
        bail!(
            "{}: feature files are checked strictly when images are built",
            notes.join("; ")
        );
    }
    if feature.format != FORMAT {
        bail!(
            "format {} is not supported; this edel understands format {FORMAT}",
            feature.format
        );
    }
    if feature.summary.trim().is_empty() {
        bail!("summary is empty; say in one line what the feature gives people");
    }
    let adds = !feature.packages.is_empty()
        || !feature.services.entries().is_empty()
        || !feature.programs.is_empty();
    if adds && feature.why.trim().is_empty() {
        bail!(
            "why is empty; a feature that adds a package, a service or a program says why (principle 3)"
        );
    }
    if let Some(p) = feature.programs.iter().find(|p| !is_name(p)) {
        bail!("program {p:?} is not a program name");
    }
    if let Some(p) = feature.packages.iter().find(|p| !is_package(p)) {
        bail!("package {p:?} is not a package name");
    }
    for keep in &feature.keep {
        crate::keep::check(keep)?;
    }
    if !feature.hardware.is_empty() && !feature.addon {
        bail!(
            "feature {name} lists hardware but is no add-on; only an add-on can be a hardware pack, so add addon = true or take hardware out"
        );
    }
    if let Some(p) = feature.hardware.iter().find(|p| !is_modalias_pattern(p)) {
        bail!(
            "hardware pattern {p:?} in feature {name} is no modalias pattern; write it as the kernel shows it, such as pci:v000010DEd*bc03*"
        );
    }
    let mut seen: Vec<&str> = Vec::new();
    for (level, service) in feature.services.entries() {
        if !is_name(service) {
            bail!("service {service:?} in {level} is not a service name");
        }
        if seen.contains(&service) {
            bail!("service {service} is listed twice; a service has one runlevel");
        }
        seen.push(service);
    }
    for (field, list) in [
        ("modules", &feature.modules),
        ("initramfs", &feature.initramfs),
    ] {
        if let Some(m) = list.iter().find(|m| !is_module(m)) {
            bail!("{field} entry {m:?} may only use a-z, 0-9, '_' and '-'");
        }
    }
    if (!feature.features.is_empty() || feature.on.is_some()) && !feature.addon {
        bail!("features and on are for add-ons; add addon = true");
    }
    if let Some(f) = feature
        .features
        .iter()
        .chain(&feature.on)
        .find(|f| !is_name(f))
    {
        bail!("{f:?} is not a feature name");
    }
    if !feature.features.is_empty() {
        if adds || !feature.keep.is_empty() || !feature.health.is_empty() {
            bail!(
                "an add-on made of features brings nothing of its own; its features bring the packages, services, programs and files"
            );
        }
        if feature.on.is_some() {
            bail!(
                "an add-on made of features is built over the image alone; on is for an add-on over it"
            );
        }
    }
    if name != "apps" && !(feature.flatpak.is_empty() && feature.flatpak_dropped.is_empty()) {
        bail!("only the apps feature lists flatpak apps");
    }
    if let Some(problem) = basic_problems(name, &feature.basics).first() {
        bail!("{problem}");
    }
    if let Some(id) = feature
        .flatpak
        .iter()
        .chain(&feature.flatpak_dropped)
        .find(|id| !crate::apps::is_flatpak_id(id))
    {
        bail!(
            "{id:?} is not a Flatpak app id; write it as Flathub does, such as \"org.gnome.TextEditor\""
        );
    }
    Ok(feature)
}

/// What is wrong with the basics rows of feature `feature` (M6.7), one
/// line each, naming the feature and the row; empty when every row is
/// right. A row names either the case that tests it or what it needs.
pub fn basic_problems(feature: &str, basics: &[Basic]) -> Vec<String> {
    let mut problems = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for basic in basics {
        let name = basic.name.as_str();
        if name.trim().is_empty() {
            problems.push(format!(
                "{feature}: a basic has an empty name; name the row, such as \"Audio\""
            ));
            continue;
        }
        if name.chars().count() > 40 {
            problems.push(format!(
                "{feature}: basic {name:?} is {} characters long; the limit is 40",
                name.chars().count()
            ));
        }
        if seen.contains(&name) {
            problems.push(format!("{feature}: basic {name:?} is listed twice"));
        }
        seen.push(name);
        match (&basic.test, &basic.needs) {
            (Some(_), Some(_)) => problems.push(format!(
                "{feature}: basic {name:?} has both test and needs; a row is either tested or marked as needing something"
            )),
            (None, None) => problems.push(format!(
                "{feature}: basic {name:?} has neither test nor needs; name the case that tests it, or what it needs"
            )),
            (Some(test), None) => {
                if test.is_empty() || !test.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
                    problems.push(format!(
                        "{feature}: basic {name:?} has test {test:?}; a case name uses only lowercase letters, digits and '-'"
                    ));
                }
            }
            (None, Some(needs)) => {
                if needs != "hardware" && !is_step(needs) {
                    problems.push(format!(
                        "{feature}: basic {name:?} needs {needs:?}; it must be \"hardware\" or a roadmap step such as \"M7.8\""
                    ));
                }
            }
        }
    }
    problems
}

/// A roadmap step: `M`, digits, then optionally `.`, digits and lowercase
/// letters, such as `M7` or `M7.10a`.
fn is_step(s: &str) -> bool {
    let Some(rest) = s.strip_prefix('M') else {
        return false;
    };
    let (major, minor) = match rest.split_once('.') {
        Some((major, minor)) => (major, Some(minor)),
        None => (rest, None),
    };
    let digits = |t: &str| !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit());
    if !digits(major) {
        return false;
    }
    match minor {
        None => true,
        Some(minor) => {
            let n = minor.bytes().take_while(u8::is_ascii_digit).count();
            n > 0 && minor[n..].bytes().all(|b| b.is_ascii_lowercase())
        }
    }
}

/// Alpine package names: lowercase letters, digits and `-_.+`.
fn is_package(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('-')
        && s.chars().all(|c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.' | '+')
        })
}

/// A hardware pattern (M7.11): a modalias has a `:` and no `/` or white
/// space, and a pattern is written as the kernel shows the modalias.
fn is_modalias_pattern(s: &str) -> bool {
    !s.is_empty() && s.contains(':') && !s.contains('/') && !s.chars().any(char::is_whitespace)
}

/// Module and mkinitfs feature names; they are pasted into `grub.cfg` and
/// `mkinitfs.conf`, so nothing that either would interpret.
fn is_module(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    const SSH: &str = r#"
        format = 1
        summary = "Log in from another computer with ssh"
        why = "Servers and VMs are run from afar."
        packages = ["openssh-server"]
        switchable = true

        [services]
        default = ["sshd"]
    "#;

    #[test]
    fn only_an_add_on_is_made_of_features_or_built_on_one() {
        let made = "format = 1\nsummary = \"d\"\naddon = true\nfeatures = [\"seat\"]\n";
        assert_eq!(check("desktop", made).unwrap().features, ["seat"]);
        let cases = [
            (
                made.replace("addon = true\n", ""),
                "features and on are for add-ons",
            ),
            (
                format!("{made}packages = [\"mesa\"]\nwhy = \"w\"\n"),
                "an add-on made of features brings nothing of its own",
            ),
            (
                format!("{made}on = \"base\"\n"),
                "on is for an add-on over it",
            ),
            (
                made.replace("\"seat\"", "\"Seat\""),
                "\"Seat\" is not a feature name",
            ),
        ];
        for (text, message) in cases {
            let err = check("desktop", &text).unwrap_err();
            assert!(format!("{err:#}").contains(message), "{err:#}");
        }
    }

    #[test]
    fn checks_a_feature() {
        let ssh = check("ssh", SSH).unwrap();
        assert_eq!(ssh.packages, ["openssh-server"]);
        assert_eq!(ssh.services.entries(), [("default", "sshd")]);
        assert!(ssh.switchable);
    }

    #[test]
    fn an_unknown_field_is_refused_by_check_and_reported_by_read() {
        let text = format!("{SSH}\n[future]\nkey = 1\n");
        let text = text.replacen("switchable = true", "switchable = true\nshiny = true", 1);
        let err = check("ssh", &text).unwrap_err();
        assert!(err.to_string().contains("unknown field shiny"), "{err}");
        let (ssh, notes) = read(&text).unwrap();
        assert_eq!(ssh.packages, ["openssh-server"]);
        assert_eq!(
            notes,
            [
                "unknown field future ignored",
                "unknown field shiny ignored"
            ]
        );
    }

    #[test]
    fn a_runlevel_this_edel_lacks_is_noted_not_fatal() {
        let text = SSH.replace("[services]", "[services]\nlate = [\"x\"]");
        let (ssh, notes) = read(&text).unwrap();
        assert_eq!(ssh.services.default, ["sshd"]);
        assert_eq!(notes, ["unknown field services.late ignored"]);
        assert!(check("ssh", &text).is_err());
    }

    #[test]
    fn a_newer_format_is_refused_by_check_and_noted_by_read() {
        let text = SSH.replace("format = 1", "format = 2");
        assert!(check("ssh", &text).is_err());
        let (_, notes) = read(&text).unwrap();
        assert!(notes[0].starts_with("format 2 is newer"));
    }

    #[test]
    fn a_package_or_service_needs_a_why() {
        let text = SSH.replace("why = \"Servers and VMs are run from afar.\"", "");
        let err = check("ssh", &text).unwrap_err();
        assert!(err.to_string().contains("why is empty"), "{err}");
        let modules_only = "format = 1\nsummary = \"x\"\nmodules = [\"ext4\"]\n";
        assert!(check("disk", modules_only).is_ok());
    }

    const PACK: &str = r#"
        format = 1
        summary = "NVIDIA graphics"
        why = "The cards' firmware."
        packages = ["linux-firmware-nvidia"]
        addon = true
        hardware = ["pci:v000010DEd*bc03*"]
    "#;

    #[test]
    fn checks_a_hardware_pack() {
        let pack = check("graphics-nvidia", PACK).unwrap();
        assert_eq!(pack.hardware, ["pci:v000010DEd*bc03*"]);
    }

    #[test]
    fn hardware_needs_an_add_on() {
        let text = PACK.replace("addon = true", "");
        let err = check("graphics-nvidia", &text).unwrap_err();
        assert!(
            err.to_string().contains("lists hardware but is no add-on"),
            "{err}"
        );
    }

    #[test]
    fn a_hardware_pattern_must_be_a_modalias_pattern() {
        for bad in ["", "pci", "pci:v000010DE/bc03", "pci: v000010DE"] {
            let text = PACK.replace("\"pci:v000010DEd*bc03*\"", &format!("{bad:?}"));
            let err = check("graphics-nvidia", &text).unwrap_err();
            assert!(
                err.to_string().contains("is no modalias pattern"),
                "{bad:?}: {err}"
            );
        }
    }

    #[test]
    fn a_service_has_one_runlevel() {
        let text = SSH.replace(
            "default = [\"sshd\"]",
            "boot = [\"sshd\"]\ndefault = [\"sshd\"]",
        );
        let err = check("ssh", &text).unwrap_err();
        assert!(err.to_string().contains("one runlevel"), "{err}");
    }

    #[test]
    fn refuses_names_grub_or_apk_would_misread() {
        assert!(check("Ssh", SSH).is_err());
        let module = SSH.replace("switchable", "modules = [\"ext4 init=/x\"]\nswitchable");
        assert!(check("ssh", &module).is_err());
        let package = SSH.replace("openssh-server", "openssh server");
        assert!(check("ssh", &package).is_err());
    }

    #[test]
    fn programs_are_names_and_need_a_why() {
        let text = SSH.replace("switchable", "programs = [\"edel-compositor\"]\nswitchable");
        assert_eq!(check("ssh", &text).unwrap().programs, ["edel-compositor"]);
        let path = text.replace("edel-compositor", "../edel");
        assert!(check("ssh", &path).is_err());
        let bare = "format = 1\nsummary = \"x\"\nprograms = [\"edel-compositor\"]\n";
        let err = check("compositor", bare).unwrap_err();
        assert!(err.to_string().contains("why is empty"), "{err}");
    }

    fn row(name: &str, test: Option<&str>, needs: Option<&str>) -> Basic {
        Basic {
            name: name.into(),
            test: test.map(Into::into),
            needs: needs.map(Into::into),
        }
    }

    #[test]
    fn a_good_row_of_each_kind_has_no_problem() {
        let rows = [
            row("Audio", Some("sound"), None),
            row("Bluetooth audio", None, Some("hardware")),
            row("Lid and power key", None, Some("M7.8")),
            row("Undo", None, Some("M7.10a")),
        ];
        assert!(basic_problems("sound", &rows).is_empty());
    }

    #[test]
    fn a_row_needs_a_name() {
        let problems = basic_problems("sound", &[row("  ", Some("sound"), None)]);
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("empty name"), "{problems:?}");
    }

    #[test]
    fn a_name_is_at_most_40_characters() {
        let long = "x".repeat(41);
        let problems = basic_problems("sound", &[row(&long, Some("sound"), None)]);
        assert!(
            problems[0].contains("is 41 characters long"),
            "{problems:?}"
        );
        let ok = "x".repeat(40);
        assert!(basic_problems("sound", &[row(&ok, Some("sound"), None)]).is_empty());
    }

    #[test]
    fn a_name_is_listed_once_per_feature() {
        let rows = [
            row("Audio", Some("sound"), None),
            row("Audio", Some("sound"), None),
        ];
        let problems = basic_problems("sound", &rows);
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("listed twice"), "{problems:?}");
    }

    #[test]
    fn a_row_is_tested_or_needs_something_not_both_or_neither() {
        let both = basic_problems("sound", &[row("Audio", Some("sound"), Some("hardware"))]);
        assert!(both[0].contains("has both test and needs"), "{both:?}");
        let neither = basic_problems("sound", &[row("Audio", None, None)]);
        assert!(
            neither[0].contains("has neither test nor needs"),
            "{neither:?}"
        );
    }

    #[test]
    fn a_test_is_a_case_name() {
        assert!(basic_problems("sound", &[row("Audio", Some("sound-2"), None)]).is_empty());
        for bad in ["Sound", "sound_card", "", "sound card"] {
            let problems = basic_problems("sound", &[row("Audio", Some(bad), None)]);
            assert_eq!(problems.len(), 1, "{bad:?}");
            assert!(problems[0].contains("lowercase letters"), "{problems:?}");
        }
    }

    #[test]
    fn needs_is_hardware_or_a_step() {
        for good in ["hardware", "M7", "M7.8", "M7.10a", "M12"] {
            assert!(
                basic_problems("power", &[row("Battery", None, Some(good))]).is_empty(),
                "{good}"
            );
        }
        for bad in [
            "software", "m7.8", "M", "M7.", "M7.8A", "M7.8a1", "7.8", "M7.x",
        ] {
            let problems = basic_problems("power", &[row("Battery", None, Some(bad))]);
            assert_eq!(problems.len(), 1, "{bad:?}");
            assert!(
                problems[0].contains("must be \"hardware\" or a roadmap step"),
                "{problems:?}"
            );
        }
    }

    #[test]
    fn check_refuses_a_bad_row_and_names_the_feature() {
        let text = "format = 1\nsummary = \"x\"\n\n[[basics]]\nname = \"Audio\"\n";
        let err = check("sound", text).unwrap_err();
        assert!(
            err.to_string()
                .contains("sound: basic \"Audio\" has neither"),
            "{err}"
        );
        let good =
            "format = 1\nsummary = \"x\"\n\n[[basics]]\nname = \"Audio\"\ntest = \"sound\"\n";
        assert_eq!(
            check("sound", good).unwrap().basics[0].test.as_deref(),
            Some("sound")
        );
    }

    #[test]
    fn only_apps_lists_flatpak_apps() {
        let text = SSH.replace(
            "switchable",
            "flatpak = [\"org.gnome.Calculator\"]\nswitchable",
        );
        assert!(check("ssh", &text).is_err());
        assert!(check("apps", &text).is_ok());
        let bad = text.replace("org.gnome.Calculator", "calculator");
        let err = check("apps", &bad).unwrap_err().to_string();
        assert!(
            err.contains("\"calculator\" is not a Flatpak app id"),
            "{err}"
        );
    }
}
