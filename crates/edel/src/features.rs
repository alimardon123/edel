//! Feature files (ADR-008, roadmap M4.0): `features/NAME.toml`, the unit
//! Edel OS is made of. A feature names its packages, its OpenRC services by
//! runlevel, its kernel modules and mkinitfs features, and the health files
//! it writes; `features/NAME/`, if present, is copied over the root. Image
//! definitions list features (`crate::def` in the binary merges them), and
//! every image carries the files of its features in
//! `/usr/share/edel/features/`, so the parts can ask what a machine has.
//!
//! Two readers, as for the settings file: [`check`] is strict, for `edel
//! image build` and `image check`; [`read`] is lenient, for readers on a
//! machine, and returns what it ignored so the caller can report it.

use anyhow::{Context, Result, bail};
use serde::Deserialize;

/// The only feature format so far.
pub const FORMAT: u32 = 1;

/// Where every image keeps the files of its features.
pub const DIR: &str = crate::places::FEATURES_DIR;

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
    /// Also built as a signed add-on (M7.2a).
    #[serde(default)]
    pub addon: bool,
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
    if name != "apps" && !(feature.flatpak.is_empty() && feature.flatpak_dropped.is_empty()) {
        bail!("only the apps feature lists flatpak apps");
    }
    Ok(feature)
}

/// a-z, 0-9 and `-`, not starting with `-`: feature and service names.
pub fn is_name(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('-')
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Alpine package names: lowercase letters, digits and `-_.+`.
fn is_package(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('-')
        && s.chars().all(|c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.' | '+')
        })
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

    #[test]
    fn only_apps_lists_flatpak_apps() {
        let text = SSH.replace(
            "switchable",
            "flatpak = [\"org.gnome.Calculator\"]\nswitchable",
        );
        assert!(check("ssh", &text).is_err());
        assert!(check("apps", &text).is_ok());
    }
}
