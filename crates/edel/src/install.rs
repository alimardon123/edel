//! The install plan (roadmap M2.4): what `edel install` erases and what it
//! writes, built once and shown three ways (ADR-008): `--plan` prints
//! it, a terminal run shows it before the person types the disk's name,
//! and a run with neither a terminal nor `--yes` prints it and exits 3.
//! Settings' installer page shows the same plan (M6.6a), so it lives in the
//! library.

use std::fmt;

use anyhow::{Result, bail};

use crate::i18n::{tr, trf};

const MIB: u64 = 1024 * 1024;
/// The EFI system partition, as `boot.rs` lays it out.
pub const ESP_MIB: u64 = 64;
/// The smallest data partition worth installing with.
pub const MIN_DATA_MIB: u64 = 256;
/// Each slot of an installed machine, whatever the medium it was installed
/// from: a machine installed from a preview must take every later update,
/// so slots are sized for the base's growth, not today's (roadmap M3.3b,
/// `docs/FORMATS.md`).
pub const SLOT_MIB: u64 = 4096;

/// A partition already on the target disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Partition {
    /// Kernel name, such as `sda1`
    pub name: String,
    /// The file system's label, if it has one
    pub label: Option<String>,
    /// The file system's type as blkid names it, such as `ntfs`
    pub fs: Option<String>,
    pub bytes: u64,
}

/// The target disk as it is now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Disk {
    /// Kernel name, such as `sda`
    pub name: String,
    pub model: Option<String>,
    pub bytes: u64,
    pub partitions: Vec<Partition>,
}

/// What `edel install` will do to one disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub disk: Disk,
    /// Each slot's size, `SLOT_MIB`
    pub slot_mib: u64,
    /// The settings file the new machine starts with
    pub system_file: String,
}

impl Plan {
    /// The data partition fills the rest, less 1 MiB in front and 1 MiB
    /// for the backup partition table at the end.
    pub fn data_mib(&self) -> u64 {
        (self.disk.bytes / MIB).saturating_sub(1 + ESP_MIB + 2 * self.slot_mib + 1)
    }

    /// Refuses a disk too small for two slots and some data.
    pub fn check(&self) -> Result<()> {
        if self.data_mib() < MIN_DATA_MIB {
            let need = 2 + ESP_MIB + 2 * self.slot_mib + MIN_DATA_MIB;
            bail!(
                "{}",
                trf(
                    "/dev/{disk} has {has}, and Edel OS needs at least {needs}; use a larger disk",
                    &[
                        ("disk", &self.disk.name),
                        ("has", &size(self.disk.bytes)),
                        ("needs", &size(need * MIB))
                    ]
                )
            );
        }
        Ok(())
    }
}

/// Decimal sizes, as disks are sold: `420 GB`, `3.2 GB`, `512 MB`.
pub fn size(bytes: u64) -> String {
    let gb = bytes as f64 / 1e9;
    if gb >= 10.0 {
        format!("{gb:.0} GB")
    } else if gb >= 1.0 {
        format!("{gb:.1} GB")
    } else {
        format!("{:.0} MB", bytes as f64 / 1e6)
    }
}

/// `ntfs` as people know it: `NTFS`.
fn fs_name(fs: &str) -> String {
    match fs {
        "vfat" => "FAT".into(),
        "ntfs" | "exfat" | "btrfs" | "xfs" | "zfs_member" => {
            fs.trim_end_matches("_member").to_uppercase()
        }
        "crypto_LUKS" => "encrypted".into(),
        "swap" => "swap".into(),
        other => other.to_string(),
    }
}

impl fmt::Display for Partition {
    /// `sda3: Windows (NTFS, 420 GB)`
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = self.label.as_deref().unwrap_or(tr("no name"));
        match &self.fs {
            Some(fs) => write!(
                f,
                "{}",
                trf(
                    "{name}: {label} ({fs}, {size})",
                    &[
                        ("name", &self.name),
                        ("label", label),
                        ("fs", &fs_name(fs)),
                        ("size", &size(self.bytes))
                    ]
                )
            ),
            None => write!(
                f,
                "{}",
                trf(
                    "{name}: {label} (no file system, {size})",
                    &[
                        ("name", &self.name),
                        ("label", label),
                        ("size", &size(self.bytes))
                    ]
                )
            ),
        }
    }
}

impl fmt::Display for Plan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let disk = &self.disk;
        let model = disk.model.as_deref().unwrap_or(tr("no model name"));
        writeln!(
            f,
            "{}",
            trf(
                "Install Edel OS on /dev/{disk} ({model}, {size})",
                &[
                    ("disk", &disk.name),
                    ("model", model),
                    ("size", &size(disk.bytes))
                ]
            )
        )?;
        if disk.partitions.is_empty() {
            writeln!(f, "{}", tr("The disk has no partitions."))?;
        } else {
            writeln!(f, "{}", tr("This erases everything on it:"))?;
            for partition in &disk.partitions {
                writeln!(f, "  {partition}")?;
            }
        }
        writeln!(f, "{}", tr("and writes:"))?;
        writeln!(
            f,
            "{}",
            trf(
                "  1: EFI system partition, {mib} MiB",
                &[("mib", &ESP_MIB.to_string())]
            )
        )?;
        writeln!(
            f,
            "{}",
            trf(
                "  2: slot A, {mib} MiB, a copy of the running system",
                &[("mib", &self.slot_mib.to_string())]
            )
        )?;
        writeln!(
            f,
            "{}",
            trf(
                "  3: slot B, {mib} MiB, empty until the first update",
                &[("mib", &self.slot_mib.to_string())]
            )
        )?;
        writeln!(
            f,
            "{}",
            trf(
                "  4: data, {mib} MiB, starting from the settings file {file}",
                &[
                    ("mib", &self.data_mib().to_string()),
                    ("file", &self.system_file)
                ]
            )
        )
    }
}

/// Reads busybox or util-linux `blkid DEVICE` output: `LABEL` and `TYPE`.
pub fn parse_blkid(line: &str) -> (Option<String>, Option<String>) {
    let field = |name: &str| {
        let start = line.find(&format!(" {name}=\""))? + name.len() + 3;
        let len = line[start..].find('"')?;
        Some(line[start..start + len].to_string()).filter(|v| !v.is_empty())
    };
    (field("LABEL"), field("TYPE"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(partitions: Vec<Partition>) -> Plan {
        Plan {
            disk: Disk {
                name: "sda".into(),
                model: Some("Samsung SSD 870".into()),
                bytes: 500_107_862_016,
                partitions,
            },
            slot_mib: SLOT_MIB,
            system_file: "/run/media/seed.toml".into(),
        }
    }

    #[test]
    fn shows_what_it_erases_as_people_know_it() {
        let shown = plan(vec![
            Partition {
                name: "sda1".into(),
                label: None,
                fs: Some("vfat".into()),
                bytes: 104_857_600,
            },
            Partition {
                name: "sda3".into(),
                label: Some("Windows".into()),
                fs: Some("ntfs".into()),
                bytes: 420_000_000_000,
            },
        ])
        .to_string();
        assert!(shown.starts_with(
            "Install Edel OS on /dev/sda (Samsung SSD 870, 500 GB)\nThis erases everything on it:\n"
        ));
        assert!(shown.contains("  sda1: no name (FAT, 105 MB)\n"));
        assert!(shown.contains("  sda3: Windows (NTFS, 420 GB)\n"));
        assert!(shown.contains("  2: slot A, 4096 MiB, a copy of the running system\n"));
    }

    #[test]
    fn the_data_partition_takes_the_rest() {
        let mut plan = plan(Vec::new());
        assert_eq!(plan.data_mib(), 476_940 - 1 - 64 - 8192 - 1);
        assert!(plan.check().is_ok());
        plan.disk.bytes = 8 * 1024 * MIB;
        let error = plan.check().unwrap_err().to_string();
        assert!(error.contains("needs at least 8.9 GB"), "{error}");
    }

    #[test]
    fn reads_blkid_lines() {
        assert_eq!(
            parse_blkid("/dev/vdb3: LABEL=\"Windows\" UUID=\"1234\" TYPE=\"ntfs\""),
            (Some("Windows".into()), Some("ntfs".into()))
        );
        assert_eq!(
            parse_blkid("/dev/vdb1: UUID=\"ab\" TYPE=\"vfat\""),
            (None, Some("vfat".into()))
        );
        assert_eq!(parse_blkid(""), (None, None));
    }
}
