//! Image definitions: the TOML files under `images/` that describe what goes
//! into each Edel OS image.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::boot;
use crate::guard;

/// The only definition format this version of `edel` understands. Bump it on
/// any breaking change and teach `edel` to migrate older files.
pub const FORMAT: u32 = 1;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageDef {
    pub format: u32,
    pub name: String,
    pub variant: Variant,
    pub arch: String,
    pub hostname: Option<String>,
    pub alpine: Alpine,
    pub packages: Packages,
    #[serde(default)]
    pub services: Services,
    /// Directories copied over the root filesystem after packages are
    /// installed, in order. Relative to the definition file.
    #[serde(default)]
    pub files: Vec<PathBuf>,
    pub vm: Option<Vm>,
    /// Facts about the image written into its os-release.
    #[serde(default)]
    pub image: ImageFacts,
    #[serde(default)]
    pub release: ReleaseKeys,
}

/// `[release]`: public key files (relative to the definition) that may sign
/// this image's updates, copied to `/usr/share/edel/keys/` (M1.6).
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseKeys {
    #[serde(default)]
    pub public_keys: Vec<PathBuf>,
}

/// `[image]`: what `edel boot guard` waits for before it confirms a slot
/// (M1.5), written as `EDEL_HEALTH` and `EDEL_HEALTH_TIMEOUT`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageFacts {
    #[serde(default)]
    pub health: Vec<String>,
    #[serde(default = "default_health_timeout")]
    pub health_timeout: u64,
}

impl Default for ImageFacts {
    fn default() -> Self {
        ImageFacts {
            health: Vec::new(),
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
pub struct Packages {
    pub install: Vec<String>,
}

/// OpenRC services to enable, by runlevel.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
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
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Vm {
    /// Alpine kernel flavor; the `linux-<flavor>` package must be installed.
    pub kernel: String,
    /// Size of each of the two root slots on the disk (ADR-006).
    pub slot_mib: u64,
    /// Kernel arguments added to the ones the A/B boot needs, for example
    /// the console to use. `modules=` is not one of them: `edel image
    /// build` owns that list (`boot::MODULES`).
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

impl ImageDef {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading image definition {}", path.display()))?;
        let def: ImageDef = toml::from_str(&text)
            .with_context(|| format!("parsing image definition {}", path.display()))?;
        def.validate()
            .with_context(|| format!("checking image definition {}", path.display()))?;
        Ok(def)
    }

    pub fn validate(&self) -> Result<()> {
        if self.format != FORMAT {
            bail!(
                "format {} is not supported; this edel understands format {FORMAT}",
                self.format
            );
        }
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
        if self.packages.install.is_empty() {
            bail!("packages.install must list at least one package");
        }
        match (self.variant, &self.vm) {
            (Variant::Vm, None) => bail!("a vm image needs a [vm] section"),
            (Variant::Container, Some(_)) => {
                bail!("a container image must not have a [vm] section")
            }
            (Variant::Vm, Some(vm)) => {
                if self.image.health.is_empty() {
                    bail!("a vm image needs [image] health, the files the boot guard waits for");
                }
                if let Some(name) = self.image.health.iter().find(|n| !guard::is_health_name(n)) {
                    bail!("image.health {name:?} is not a known health name");
                }
                if self.image.health_timeout < 10 {
                    bail!("image.health_timeout must be at least 10 seconds");
                }
                let kernel_pkg = format!("linux-{}", vm.kernel);
                if !self.packages.install.contains(&kernel_pkg) {
                    bail!(
                        "vm.kernel is {:?} but {kernel_pkg} is not in packages.install",
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
                        "vm.cmdline must not set modules=; edel image build adds the one list every image uses"
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
            (Variant::Container, None) => {}
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
    !s.is_empty()
        && !s.starts_with('-')
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    const VM: &str = r#"
        format = 1
        name = "edel-vm"
        variant = "vm"
        arch = "x86_64"
        hostname = "edel"

        [alpine]
        branch = "v3.24"
        mirror = "https://dl-cdn.alpinelinux.org/alpine/"
        repositories = ["main", "community"]

        [packages]
        install = ["alpine-base", "linux-virt"]

        [services]
        sysinit = ["devfs"]
        boot = ["hostname"]
        default = ["sshd"]

        [image]
        health = ["default-runlevel"]

        [vm]
        kernel = "virt"
        slot_mib = 1024
        cmdline = "console=ttyS0"
    "#;

    fn parse(text: &str) -> Result<ImageDef> {
        let def: ImageDef = toml::from_str(text)?;
        def.validate()?;
        Ok(def)
    }

    #[test]
    fn parses_a_vm_definition() {
        let def = parse(VM).unwrap();
        assert_eq!(def.variant, Variant::Vm);
        assert_eq!(def.stem(), "edel-vm-x86_64");
        assert_eq!(
            def.services.entries(),
            vec![
                ("sysinit", "devfs"),
                ("boot", "hostname"),
                ("default", "sshd")
            ]
        );
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
    fn rejects_an_unknown_format() {
        let err = parse(&VM.replace("format = 1", "format = 2")).unwrap_err();
        assert!(err.to_string().contains("format 2 is not supported"));
    }

    #[test]
    fn rejects_unknown_or_missing_health() {
        let unknown = VM.replace("\"default-runlevel\"", "\"desktop-ish\"");
        let def: ImageDef = toml::from_str(&unknown).unwrap();
        assert!(def.validate().is_err());
        let none = VM.replace("health = [\"default-runlevel\"]", "");
        let def: ImageDef = toml::from_str(&none).unwrap();
        assert!(def.validate().is_err());
    }

    #[test]
    fn rejects_a_vm_without_its_kernel_package() {
        let err = parse(&VM.replace(r#""linux-virt""#, r#""linux-lts""#)).unwrap_err();
        assert!(
            err.to_string()
                .contains("linux-virt is not in packages.install")
        );
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
    }

    #[test]
    fn rejects_unsafe_names() {
        assert!(parse(&VM.replace(r#"name = "edel-vm""#, r#"name = "../x""#)).is_err());
        assert!(parse(&VM.replace(r#"hostname = "edel""#, r#"hostname = "a b""#)).is_err());
    }
}
