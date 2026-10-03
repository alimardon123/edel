//! Image definitions (format 2, roadmap M4.0): the TOML files under
//! `images/` that list an image's features and its facts. What goes into
//! an image (packages, services, kernel modules, mkinitfs features, health
//! files and files) comes only from the feature files it lists
//! (`features/NAME.toml`, ADR-008), which [`ImageDef::load`] merges here.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use edel::features::{self, Feature, Services};
use serde::Deserialize;

use crate::boot;
use crate::guard;

/// The only definition format this version of `edel` understands. Every
/// definition lives in this repository, so a bump converts them all in the
/// same PR, and the older format is refused.
pub const FORMAT: u32 = 2;

/// A definition as written.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DefFile {
    #[allow(dead_code)]
    format: u32,
    name: String,
    variant: Variant,
    arch: String,
    hostname: Option<String>,
    /// Feature names, merged in this order.
    features: Vec<String>,
    /// Switchable features shipped with their services off.
    #[serde(default)]
    off: Vec<String>,
    alpine: Alpine,
    vm: Option<Vm>,
    #[serde(default)]
    image: ImageFacts,
    #[serde(default)]
    release: ReleaseKeys,
}

/// A feature file found next to a definition.
#[derive(Debug)]
pub struct Listed {
    pub name: String,
    /// `features/NAME.toml`
    pub path: PathBuf,
    /// The file's text, copied into the image as it is.
    pub text: String,
    /// `features/NAME/`, when the feature ships files.
    pub files: Option<PathBuf>,
    pub feature: Feature,
}

/// An image definition with its features merged.
#[derive(Debug)]
pub struct ImageDef {
    pub name: String,
    pub variant: Variant,
    pub arch: String,
    pub hostname: Option<String>,
    pub alpine: Alpine,
    pub vm: Option<Vm>,
    pub image: ImageFacts,
    pub release: ReleaseKeys,
    /// The listed features, in order.
    pub features: Vec<Listed>,
    /// Switchable features shipped with their services off, written as
    /// `EDEL_SERVICES_OFF` in os-release.
    pub off: Vec<String>,
    /// The packages of every listed feature, each once, in order.
    pub packages: Vec<String>,
    /// The services to enable: those of the features not in `off`.
    pub services: Services,
    /// The modules the initramfs loads first (`modules=`), in order.
    pub modules: Vec<String>,
    /// The mkinitfs features, sorted.
    pub initramfs: Vec<String>,
    /// The health files the boot guard waits for (`EDEL_HEALTH`).
    pub health: Vec<String>,
    /// Our own programs to copy to `/usr/bin`, each once, in order.
    pub programs: Vec<String>,
}

/// `[release]`: public key files (relative to the definition) that may sign
/// this image's updates, copied to `/usr/share/edel/keys/` (M1.6).
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseKeys {
    #[serde(default)]
    pub public_keys: Vec<PathBuf>,
}

/// `[image]`: how long `edel boot guard` waits for the health files before
/// the watchdog restarts the machine (M1.5), as `EDEL_HEALTH_TIMEOUT`. The
/// health files themselves come from the features.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageFacts {
    #[serde(default = "default_health_timeout")]
    pub health_timeout: u64,
}

impl Default for ImageFacts {
    fn default() -> Self {
        ImageFacts {
            health_timeout: default_health_timeout(),
        }
    }
}

fn default_health_timeout() -> u64 {
    guard::DEFAULT_TIMEOUT
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Variant {
    Container,
    Vm,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Alpine {
    /// Alpine stable branch, for example "v3.24".
    pub branch: String,
    pub mirror: String,
    pub repositories: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Vm {
    /// Alpine kernel flavor; a feature must install `linux-<flavor>`.
    pub kernel: String,
    /// Size of each of the two root slots on the disk (ADR-006).
    pub slot_mib: u64,
    /// Kernel arguments added to the ones the A/B boot needs, for example
    /// the console to use. `modules=` is not one of them: `edel image
    /// build` writes it from the features' `modules`.
    #[serde(default)]
    pub cmdline: String,
    /// Size of the data partition in the image (M1.2). It grows to the
    /// end of the disk on first boot, so it only needs room for a start.
    #[serde(default = "default_data_mib")]
    pub data_mib: u64,
}

fn default_data_mib() -> u64 {
    64
}

/// Where a definition's features come from: `features/` in its own
/// directory, if there is one (CI's test features, under `ci/`), and the
/// repository's, the `features/` of the nearest directory above that also
/// holds `images/`.
pub fn feature_dirs(def_dir: &Path) -> Result<Vec<PathBuf>> {
    let dir = fs::canonicalize(def_dir)
        .with_context(|| format!("finding the directory {}", def_dir.display()))?;
    let mut dirs = Vec::new();
    if dir.join("features").is_dir() {
        dirs.push(dir.join("features"));
    }
    let Some(repo) = dir
        .ancestors()
        .find(|d| d.join("features").is_dir() && d.join("images").is_dir())
    else {
        bail!(
            "no directory at or above {} holds both features/ and images/",
            dir.display()
        );
    };
    if !dirs.contains(&repo.join("features")) {
        dirs.push(repo.join("features"));
    }
    Ok(dirs)
}

/// Every feature in `dirs`, each checked strictly. A name found twice, a
/// `NAME/` without its `NAME.toml` and anything else in a features
/// directory are refused, so a dropped feature leaves nothing behind.
pub fn find_features(dirs: &[PathBuf]) -> Result<BTreeMap<String, Listed>> {
    let mut found: BTreeMap<String, Listed> = BTreeMap::new();
    for dir in dirs {
        let mut entries: Vec<PathBuf> = fs::read_dir(dir)
            .with_context(|| format!("reading {}", dir.display()))?
            .map(|e| e.map(|e| e.path()))
            .collect::<std::io::Result<_>>()?;
        entries.sort();
        for path in entries {
            let file_name = path.file_name().unwrap_or_default().to_string_lossy();
            if path.is_dir() {
                if !dir.join(format!("{file_name}.toml")).is_file() {
                    bail!("{} has no {file_name}.toml beside it", path.display());
                }
                continue;
            }
            let Some(name) = file_name.strip_suffix(".toml") else {
                bail!("{} is neither NAME.toml nor NAME/", path.display());
            };
            if let Some(other) = found.get(name) {
                bail!(
                    "feature {name} is both {} and {}",
                    other.path.display(),
                    path.display()
                );
            }
            let text =
                fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
            let feature = features::check(name, &text)
                .with_context(|| format!("checking feature file {}", path.display()))?;
            let files = Some(dir.join(name)).filter(|d| d.is_dir());
            found.insert(
                name.to_string(),
                Listed {
                    name: name.to_string(),
                    path: path.clone(),
                    text,
                    files,
                    feature,
                },
            );
        }
    }
    Ok(found)
}

/// Every file below `dir` (not directories), relative to it.
fn files_below(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut todo = vec![PathBuf::new()];
    while let Some(rel) = todo.pop() {
        for entry in fs::read_dir(dir.join(&rel))? {
            let entry = entry?;
            let path = rel.join(entry.file_name());
            if entry.file_type()?.is_dir() {
                todo.push(path);
            } else {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

/// Appends the entries of `from` that `to` lacks.
fn add_new(to: &mut Vec<String>, from: &[String]) {
    for item in from {
        if !to.contains(item) {
            to.push(item.clone());
        }
    }
}

impl ImageDef {
    pub fn load(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("reading image definition {}", path.display()))?;
        let def_dir = path.parent().filter(|d| !d.as_os_str().is_empty());
        let found = feature_dirs(def_dir.unwrap_or(Path::new(".")))
            .and_then(|dirs| find_features(&dirs))
            .with_context(|| format!("finding the features of {}", path.display()))?;
        Self::merge(&text, found)
            .with_context(|| format!("checking image definition {}", path.display()))
    }

    /// Reads the definition `text` and merges the features it lists from
    /// `found`: packages, services, modules, mkinitfs features and health
    /// as sets, in the order listed.
    pub fn merge(text: &str, mut found: BTreeMap<String, Listed>) -> Result<Self> {
        let table: toml::Table = toml::from_str(text).context("parsing the definition")?;
        match table.get("format").and_then(|f| f.as_integer()) {
            Some(2) => {}
            Some(1) => bail!(
                "format 1 is no longer read: a definition lists features now (format 2, docs/FEATURES.md)"
            ),
            Some(n) => bail!("format {n} is not supported; this edel understands format {FORMAT}"),
            None => bail!("format is missing"),
        }
        let file: DefFile = toml::Value::Table(table).try_into()?;

        let known: Vec<String> = found.keys().cloned().collect();
        let mut listed: Vec<Listed> = Vec::new();
        for name in &file.features {
            if listed.iter().any(|l| &l.name == name) {
                bail!("feature {name} is listed twice");
            }
            let Some(feature) = found.remove(name) else {
                bail!(
                    "unknown feature {name}; the features are {}",
                    known.join(", ")
                );
            };
            listed.push(feature);
        }
        for name in &file.off {
            match listed.iter().find(|l| &l.name == name) {
                None => bail!("off lists {name}, which is not in features"),
                Some(l) if !l.feature.switchable => {
                    bail!("off lists {name}, which is not switchable")
                }
                Some(_) => {}
            }
        }

        let mut packages = Vec::new();
        let mut services = Services::default();
        let mut runlevel: BTreeMap<&str, (&str, &str)> = BTreeMap::new();
        let mut modules = Vec::new();
        let mut initramfs = Vec::new();
        let mut health = Vec::new();
        let mut programs = Vec::new();
        let mut shipped: BTreeMap<PathBuf, &str> = BTreeMap::new();
        for l in &listed {
            let f = &l.feature;
            add_new(&mut packages, &f.packages);
            for (level, service) in f.services.entries() {
                match runlevel.get(service) {
                    Some((other_level, other)) if *other_level != level => bail!(
                        "service {service} is in {other_level} in {other} and in {level} in {}; a service has one runlevel",
                        l.name
                    ),
                    _ => {
                        runlevel.insert(service, (level, &l.name));
                    }
                }
                if !file.off.contains(&l.name) {
                    services.add(level, service);
                }
            }
            add_new(&mut modules, &f.modules);
            add_new(&mut initramfs, &f.initramfs);
            add_new(&mut health, &f.health);
            add_new(&mut programs, &f.programs);
            if let Some(dir) = &l.files {
                for path in files_below(dir)? {
                    if let Some(other) = shipped.insert(path.clone(), &l.name) {
                        bail!(
                            "{} is shipped by both {other} and {}; a file belongs to one feature",
                            path.display(),
                            l.name
                        );
                    }
                }
            }
        }
        initramfs.sort();

        let def = ImageDef {
            name: file.name,
            variant: file.variant,
            arch: file.arch,
            hostname: file.hostname,
            alpine: file.alpine,
            vm: file.vm,
            image: file.image,
            release: file.release,
            features: listed,
            off: file.off,
            packages,
            services,
            modules,
            initramfs,
            health,
            programs,
        };
        def.validate()?;
        Ok(def)
    }

    fn validate(&self) -> Result<()> {
        if !is_simple_name(&self.name) {
            bail!("name {:?} must use only a-z, 0-9 and '-'", self.name);
        }
        if !is_simple_name(&self.arch.replace('_', "-")) {
            bail!("arch {:?} is not a valid architecture name", self.arch);
        }
        if let Some(hostname) = &self.hostname {
            if !is_simple_name(hostname) {
                bail!("hostname {hostname:?} must use only a-z, 0-9 and '-'");
            }
        }
        if self.alpine.repositories.is_empty() {
            bail!("alpine.repositories must list at least one repository");
        }
        if self.packages.is_empty() {
            bail!("the features install no package");
        }
        if let Some(name) = self.health.iter().find(|n| !guard::is_health_name(n)) {
            bail!("health {name:?} is not a known health name");
        }
        match (self.variant, &self.vm) {
            (Variant::Vm, None) => bail!("a vm image needs a [vm] section"),
            (Variant::Container, Some(_)) => {
                bail!("a container image must not have a [vm] section")
            }
            (Variant::Container, None) => {
                if !self.health.is_empty() {
                    bail!("a container has no boot guard, so no feature of it may list health");
                }
            }
            (Variant::Vm, Some(vm)) => {
                if self.health.is_empty() {
                    bail!(
                        "a vm image needs a feature with health, the files the boot guard waits for"
                    );
                }
                if self.modules.is_empty() || self.initramfs.is_empty() {
                    bail!(
                        "a vm image needs features with modules and initramfs, or it cannot find its root"
                    );
                }
                if self.image.health_timeout < 10 {
                    bail!("image.health_timeout must be at least 10 seconds");
                }
                let kernel_pkg = format!("linux-{}", vm.kernel);
                if !self.packages.contains(&kernel_pkg) {
                    bail!(
                        "vm.kernel is {:?} but no feature installs {kernel_pkg}",
                        vm.kernel
                    );
                }
                if vm.slot_mib < 64 {
                    bail!("vm.slot_mib must be at least 64");
                }
                if vm.data_mib < 16 {
                    bail!("vm.data_mib must be at least 16");
                }
                if vm
                    .cmdline
                    .split_whitespace()
                    .any(|a| a.starts_with("modules="))
                {
                    bail!(
                        "vm.cmdline must not set modules=; edel image build writes it from the features"
                    );
                }
                if !boot::is_safe_cmdline(&vm.cmdline) {
                    bail!(
                        "vm.cmdline {:?} may only hold plain kernel arguments",
                        vm.cmdline
                    );
                }
                boot::efi_target(&self.arch)?;
            }
        }
        Ok(())
    }

    /// Repository URLs, one per line, as Alpine's `/etc/apk/repositories`
    /// expects them.
    pub fn repositories_file(&self) -> String {
        let mirror = self.alpine.mirror.trim_end_matches('/');
        self.alpine
            .repositories
            .iter()
            .map(|repo| format!("{mirror}/{}/{repo}\n", self.alpine.branch))
            .collect()
    }

    /// File name stem shared by every output of this image.
    pub fn stem(&self) -> String {
        format!("{}-{}", self.name, self.arch)
    }
}

fn is_simple_name(s: &str) -> bool {
    features::is_name(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    const VM: &str = r#"
        format = 2
        name = "edel-vm"
        variant = "vm"
        arch = "x86_64"
        hostname = "edel"
        features = ["base", "boot", "ssh"]

        [alpine]
        branch = "v3.24"
        mirror = "https://dl-cdn.alpinelinux.org/alpine/"
        repositories = ["main", "community"]

        [vm]
        kernel = "virt"
        slot_mib = 1024
        cmdline = "console=ttyS0"
    "#;

    const FEATURES: [(&str, &str); 4] = [
        (
            "base",
            r#"format = 1
            summary = "base"
            why = "w"
            packages = ["alpine-base"]
            modules = ["ext4", "overlay"]
            [services]
            sysinit = ["devfs"]
            "#,
        ),
        (
            "boot",
            r#"format = 1
            summary = "boot"
            why = "w"
            packages = ["linux-virt", "alpine-base"]
            modules = ["softdog", "ext4"]
            initramfs = ["virtio", "base"]
            health = ["default-runlevel"]
            [services]
            boot = ["hostname"]
            "#,
        ),
        (
            "ssh",
            r#"format = 1
            summary = "ssh"
            why = "w"
            packages = ["openssh-server"]
            switchable = true
            [services]
            default = ["sshd"]
            "#,
        ),
        (
            "dbus-user",
            r#"format = 1
            summary = "another feature that needs dbus"
            why = "w"
            packages = ["dbus"]
            [services]
            default = ["dbus"]
            "#,
        ),
    ];

    fn listed(name: &str, text: &str) -> Listed {
        Listed {
            name: name.into(),
            path: PathBuf::from(format!("features/{name}.toml")),
            text: text.into(),
            files: None,
            feature: features::check(name, text).unwrap(),
        }
    }

    fn found() -> BTreeMap<String, Listed> {
        FEATURES
            .iter()
            .map(|(name, text)| (name.to_string(), listed(name, text)))
            .collect()
    }

    fn parse(text: &str) -> Result<ImageDef> {
        ImageDef::merge(text, found())
    }

    #[test]
    fn merges_the_features_in_order() {
        let def = parse(VM).unwrap();
        assert_eq!(def.variant, Variant::Vm);
        assert_eq!(def.stem(), "edel-vm-x86_64");
        assert_eq!(
            def.packages,
            ["alpine-base", "linux-virt", "openssh-server"]
        );
        assert_eq!(
            def.services.entries(),
            [
                ("sysinit", "devfs"),
                ("boot", "hostname"),
                ("default", "sshd")
            ]
        );
        assert_eq!(def.modules, ["ext4", "overlay", "softdog"]);
        assert_eq!(def.initramfs, ["base", "virtio"]);
        assert_eq!(def.health, ["default-runlevel"]);
        let order = parse(&VM.replace(r#"["base", "boot", "ssh"]"#, r#"["boot", "base", "ssh"]"#));
        assert_eq!(order.unwrap().modules, ["softdog", "ext4", "overlay"]);
    }

    #[test]
    fn a_service_two_features_list_is_enabled_once() {
        let text = FEATURES[3]
            .1
            .replace("summary = \"another", "summary = \"a second");
        let mut found = found();
        found.insert("dbus-too".into(), listed("dbus-too", &text));
        let def = ImageDef::merge(
            &VM.replace(r#""ssh"]"#, r#""ssh", "dbus-user", "dbus-too"]"#),
            found,
        )
        .unwrap();
        assert_eq!(def.services.default, ["sshd", "dbus"]);
        assert_eq!(def.packages.iter().filter(|p| *p == "dbus").count(), 1);
    }

    #[test]
    fn off_keeps_the_packages_and_drops_the_services() {
        let def = parse(&VM.replace("[alpine]", "off = [\"ssh\"]\n[alpine]")).unwrap();
        assert!(def.packages.contains(&"openssh-server".to_string()));
        assert!(def.services.default.is_empty());
        assert_eq!(def.off, ["ssh"]);
    }

    #[test]
    fn refuses_what_the_merge_cannot_settle() {
        let cases = [
            (
                VM.replace(r#""ssh"]"#, r#""ssh", "bluetooth"]"#),
                "unknown feature bluetooth; the features are base, boot, dbus-user, ssh",
            ),
            (VM.replace(r#""ssh"]"#, r#""ssh", "ssh"]"#), "listed twice"),
            (
                VM.replace("[alpine]", "off = [\"dbus-user\"]\n[alpine]"),
                "off lists dbus-user, which is not in features",
            ),
            (
                VM.replace("[alpine]", "off = [\"base\"]\n[alpine]"),
                "off lists base, which is not switchable",
            ),
        ];
        for (text, message) in cases {
            let err = parse(&text).unwrap_err();
            assert!(format!("{err:#}").contains(message), "{err:#}");
        }
    }

    #[test]
    fn refuses_one_service_in_two_runlevels() {
        let mut found = found();
        let text = FEATURES[3]
            .1
            .replace("default = [\"dbus\"]", "boot = [\"sshd\"]");
        found.insert("dbus-user".into(), listed("dbus-user", &text));
        let err =
            ImageDef::merge(&VM.replace(r#""ssh"]"#, r#""ssh", "dbus-user"]"#), found).unwrap_err();
        assert!(
            err.to_string()
                .contains("service sshd is in default in ssh and in boot in dbus-user"),
            "{err}"
        );
    }

    #[test]
    fn refuses_a_file_two_features_ship() {
        let dir = std::env::temp_dir().join(format!("edel-def-files-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        for name in ["base", "ssh"] {
            fs::create_dir_all(dir.join(name).join("etc")).unwrap();
            fs::write(dir.join(name).join("etc/motd"), name).unwrap();
        }
        let mut found = found();
        for name in ["base", "ssh"] {
            found.get_mut(name).unwrap().files = Some(dir.join(name));
        }
        let err = ImageDef::merge(VM, found).unwrap_err();
        fs::remove_dir_all(&dir).unwrap();
        assert!(
            err.to_string()
                .contains("etc/motd is shipped by both base and ssh"),
            "{err}"
        );
    }

    #[test]
    fn finds_features_beside_the_definition_and_above_it() {
        let dir = std::env::temp_dir().join(format!("edel-def-dirs-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let repo = dir.join("repo");
        fs::create_dir_all(repo.join("features/ssh")).unwrap();
        fs::create_dir_all(repo.join("images")).unwrap();
        fs::create_dir_all(repo.join("ci/test/features")).unwrap();
        // A features/ above the definition that is not the repository's
        // is not looked in.
        fs::create_dir_all(repo.join("ci/features")).unwrap();
        fs::write(repo.join("features/ssh.toml"), FEATURES[2].1).unwrap();
        fs::write(repo.join("ci/test/features/base.toml"), FEATURES[0].1).unwrap();
        let dirs = feature_dirs(&repo.join("ci/test")).unwrap();
        assert_eq!(dirs, [repo.join("ci/test/features"), repo.join("features")]);
        let found = find_features(&dirs).unwrap();
        assert_eq!(found.keys().collect::<Vec<_>>(), ["base", "ssh"]);
        assert_eq!(found["ssh"].files, Some(repo.join("features/ssh")));
        assert_eq!(found["base"].files, None);

        fs::write(repo.join("ci/test/features/ssh.toml"), FEATURES[2].1).unwrap();
        let err = find_features(&dirs).unwrap_err();
        assert!(err.to_string().contains("feature ssh is both"), "{err}");
        fs::remove_file(repo.join("ci/test/features/ssh.toml")).unwrap();

        fs::create_dir_all(repo.join("features/gone")).unwrap();
        let err = find_features(&dirs).unwrap_err();
        assert!(
            err.to_string().contains("has no gone.toml beside it"),
            "{err}"
        );
        fs::remove_dir(repo.join("features/gone")).unwrap();

        fs::write(
            repo.join("features/ssh.toml"),
            format!("shiny = 1\n{}", FEATURES[2].1),
        )
        .unwrap();
        let err = find_features(&dirs).unwrap_err();
        assert!(
            format!("{err:#}").contains("unknown field shiny"),
            "{err:#}"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn writes_repositories_without_double_slashes() {
        let def = parse(VM).unwrap();
        assert_eq!(
            def.repositories_file(),
            "https://dl-cdn.alpinelinux.org/alpine/v3.24/main\n\
             https://dl-cdn.alpinelinux.org/alpine/v3.24/community\n"
        );
    }

    #[test]
    fn refuses_format_1_and_unknown_formats() {
        let err = parse(&VM.replace("format = 2", "format = 1")).unwrap_err();
        assert!(err.to_string().contains("format 1 is no longer read"));
        let err = parse(&VM.replace("format = 2", "format = 3")).unwrap_err();
        assert!(err.to_string().contains("format 3 is not supported"));
    }

    #[test]
    fn a_vm_needs_health_and_its_kernel_from_its_features() {
        let err = parse(&VM.replace(r#""boot", "#, "")).unwrap_err();
        assert!(
            err.to_string().contains("needs a feature with health"),
            "{err}"
        );
        let err = parse(&VM.replace(r#"kernel = "virt""#, r#"kernel = "lts""#)).unwrap_err();
        assert!(
            err.to_string().contains("no feature installs linux-lts"),
            "{err}"
        );
        let mut found = found();
        let unknown = FEATURES[1]
            .1
            .replace("\"default-runlevel\"", "\"desktop-ish\"");
        found.insert("boot".into(), listed("boot", &unknown));
        assert!(ImageDef::merge(VM, found).is_err());
    }

    #[test]
    fn leaves_the_module_list_to_the_builder() {
        let err = parse(&VM.replace("console=ttyS0", "console=ttyS0 modules=ext4")).unwrap_err();
        assert!(err.to_string().contains("must not set modules="));
    }

    #[test]
    fn rejects_kernel_arguments_grub_would_expand() {
        let err = parse(&VM.replace("console=ttyS0", "init=$evil")).unwrap_err();
        assert!(err.to_string().contains("plain kernel arguments"));
    }

    #[test]
    fn rejects_a_vm_for_an_arch_without_efi_boot() {
        assert!(parse(&VM.replace(r#"arch = "x86_64""#, r#"arch = "s390x""#)).is_err());
    }

    #[test]
    fn rejects_unknown_fields() {
        assert!(parse(&format!("{VM}\nsurprise = true")).is_err());
        let packages = VM.replace("[alpine]", "[packages]\ninstall = [\"x\"]\n[alpine]");
        assert!(parse(&packages).is_err());
    }

    #[test]
    fn rejects_unsafe_names() {
        assert!(parse(&VM.replace(r#"name = "edel-vm""#, r#"name = "../x""#)).is_err());
        assert!(parse(&VM.replace(r#"hostname = "edel""#, r#"hostname = "a b""#)).is_err());
    }

    /// The repository's definitions, merged from the repository's
    /// features.
    fn repo_def(name: &str) -> ImageDef {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../images/{name}.toml"));
        ImageDef::load(&path).unwrap()
    }

    fn sorted<T: Ord + Clone>(items: &[T]) -> Vec<T> {
        let mut items = items.to_vec();
        items.sort();
        items
    }

    /// Roadmap M4.0: each image made of features installs and enables what
    /// main's format-1 definition did (the laptop without sshd, its ssh
    /// shipped off), and boots with the same modules and initramfs.
    #[test]
    fn features_rebuild_the_format_1_images() {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/format1");
        for name in ["container", "vm", "laptop"] {
            let text = fs::read_to_string(fixtures.join(format!("{name}.toml"))).unwrap();
            let old: toml::Table = toml::from_str(&text).unwrap();
            let install: Vec<String> = old["packages"]["install"].clone().try_into().unwrap();
            let services: Services = old
                .get("services")
                .map(|s| s.clone().try_into().unwrap())
                .unwrap_or_default();
            let mut old_services: Vec<(&str, &str)> = services.entries();
            if name == "laptop" {
                old_services.retain(|(_, s)| *s != "sshd");
            }
            let new = repo_def(name);
            assert_eq!(sorted(&new.packages), sorted(&install), "{name}: packages");
            assert_eq!(
                sorted(&new.services.entries()),
                sorted(&old_services),
                "{name}: services"
            );
            if name != "container" {
                // Since the M2 and M3 review, i6300esb loads by hardware ID
                // like a laptop's watchdog, and the laptop adds mmc.
                assert_eq!(
                    new.modules.join(","),
                    "ext4,overlay,softdog,virtio_pci,virtio_blk,nvme,ahci,sd_mod,usb-storage,uas,xhci_pci",
                    "{name}: modules="
                );
                let mmc = if name == "laptop" { " mmc" } else { "" };
                assert_eq!(
                    new.initramfs.join(" "),
                    format!("ata base ext4 kms{mmc} nvme scsi usb virtio"),
                    "{name}: mkinitfs features"
                );
                assert_eq!(new.health, ["default-runlevel"], "{name}: EDEL_HEALTH");
            }
        }
        assert_eq!(repo_def("laptop").off, ["ssh"]);
    }

    /// Roadmap M4.0: dropping a feature from a definition drops exactly
    /// what it brought.
    #[test]
    fn dropping_ssh_from_the_vm_drops_sshd_and_openssh_server() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../images/vm.toml");
        let text = fs::read_to_string(&path).unwrap();
        let without = text.replace(r#""ab-boot", "ssh", "vm""#, r#""ab-boot", "vm""#);
        assert_ne!(text, without);
        let dirs = feature_dirs(path.parent().unwrap()).unwrap();
        let with = ImageDef::merge(&text, find_features(&dirs).unwrap()).unwrap();
        let without = ImageDef::merge(&without, find_features(&dirs).unwrap()).unwrap();
        let lost: Vec<&String> = with
            .packages
            .iter()
            .filter(|p| !without.packages.contains(p))
            .collect();
        assert_eq!(lost, ["openssh-server"]);
        assert_eq!(without.packages.len(), with.packages.len() - 1);
        let lost: Vec<(&str, &str)> = with
            .services
            .entries()
            .into_iter()
            .filter(|e| !without.services.entries().contains(e))
            .collect();
        assert_eq!(lost, [("default", "sshd")]);
        assert_eq!(
            without.services.entries().len(),
            with.services.entries().len() - 1
        );
    }
}
