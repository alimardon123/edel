//! The install plan (roadmap M2.4): what `edel install` erases and what it
//! writes, built once and shown three ways (ADR-008): `--dry-run` prints
//! it, a terminal run shows it before the person types the disk's name,
//! and a run with neither a terminal nor `--yes` prints it and exits 3.
//! Settings' installer page shows the same plan (M6.6a), so it lives in the
//! library.

use std::fmt;

use anyhow::{Result, bail};

const MIB: u64 = 1024 * 1024;
/// The EFI system partition, as `boot.rs` lays it out.
pub const ESP_MIB: u64 = 64;
/// The smallest data partition worth installing with.
pub const MIN_DATA_MIB: u64 = 256;

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
    /// Each slot's size: the running slot's
    pub slot_mib: u64,
    /// The system file the new machine starts with
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
                "/dev/{} has {}, and Edel OS needs at least {}",
                self.disk.name,
                size(self.disk.bytes),
                size(need * MIB)
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
        let label = self.label.as_deref().unwrap_or("no name");
        match &self.fs {
            Some(fs) => write!(
                f,
                "{}: {label} ({}, {})",
                self.name,
                fs_name(fs),
                size(self.bytes)
            ),
            None => write!(
                f,
                "{}: {label} (no file system, {})",
                self.name,
                size(self.bytes)
            ),
        }
    }
}

impl fmt::Display for Plan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let disk = &self.disk;
        let model = disk.model.as_deref().unwrap_or("no model name");
        writeln!(
            f,
            "Install Edel OS on /dev/{} ({model}, {})",
            disk.name,
            size(disk.bytes)
        )?;
        if disk.partitions.is_empty() {
            writeln!(f, "The disk has no partitions.")?;
        } else {
            writeln!(f, "This erases everything on it:")?;
            for partition in &disk.partitions {
                writeln!(f, "  {partition}")?;
            }
        }
        writeln!(f, "and writes:")?;
        writeln!(f, "  1: EFI system partition, {ESP_MIB} MiB")?;
        writeln!(
            f,
            "  2: slot A, {} MiB, a copy of the running system",
            self.slot_mib
        )?;
        writeln!(
            f,
            "  3: slot B, {} MiB, empty until the first update",
            self.slot_mib
        )?;
        writeln!(
            f,
            "  4: data, {} MiB, starting from the system file {}",
            self.data_mib(),
            self.system_file
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
            slot_mib: 4096,
            system_file: "/run/media/system.toml".into(),
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
