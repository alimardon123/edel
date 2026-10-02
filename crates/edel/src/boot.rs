//! The bootable A/B disk (ADR-006).
//!
//! Every VM image is a GPT disk with three partitions:
//!
//! | # | Name       | What                                            |
//! |---|------------|-------------------------------------------------|
//! | 1 | `EDEL-ESP` | EFI system partition: GRUB, its config and env  |
//! | 2 | `edel-a`   | Root slot A                                     |
//! | 3 | `edel-b`   | Root slot B, empty until the first update       |
//!
//! Each slot is a complete root filesystem, kernel included. An update is
//! written to the slot that is not running; the running system is never
//! changed in place.
//!
//! GRUB keeps three kinds of variables in `EFI/edel/grubenv`: `ORDER`, the
//! slots in the order to try them, and for each slot `<slot>_OK` (1 when it
//! holds a complete system) and `<slot>_TRY` (boot attempts not confirmed
//! yet). These are the names RAUC's GRUB backend uses, so RAUC could manage
//! the slots later without changing the disk. On every boot GRUB picks the
//! first slot in `ORDER` that is OK and has tries left, and counts the
//! attempt. Once the system is up, `edel update mark-good` resets the count.
//! A slot that fails [`TRIES`] times in a row is passed over, and the next
//! slot boots instead.

use anyhow::{Result, bail};

use crate::grubenv::Env;

/// File system label of the EFI system partition.
pub const ESP_LABEL: &str = "EDEL-ESP";
/// Size of the EFI system partition. Holds GRUB (about 1 MiB) with room to
/// grow; 64 MiB is close to the smallest FAT32 that firmware accepts.
pub const ESP_MIB: u64 = 64;
/// Where GRUB looks for its config and env, on the EFI system partition.
pub const GRUB_PREFIX: &str = "/EFI/edel";
/// Boot attempts a slot gets before GRUB moves on to the next one. More
/// than one, so a single power cut during boot does not undo an update.
pub const TRIES: u32 = 3;

/// GRUB modules built into the EFI binary. Nothing is loaded at boot time,
/// so the EFI system partition holds only three files.
pub const GRUB_MODULES: &[&str] = &[
    "part_gpt", "fat", "ext2", "normal", "linux", "loadenv", "test", "echo", "regexp", "probe",
    "serial", "terminal", "efi_gop",
];

/// The A/B slots: name and GPT partition number.
const SLOTS: [(&str, u32); 2] = [("A", 2), ("B", 3)];

const ESP_TYPE: &str = "C12A7328-F81F-11D2-BA4B-00A0C93EC93B";
const LINUX_TYPE: &str = "0FC63DAF-8483-4772-8E79-3D69D8477DE4";
const SECTORS_PER_MIB: u64 = 2048;

/// GRUB's EFI target and the file name firmware looks for on removable
/// disks, for each CPU architecture we build.
pub fn efi_target(arch: &str) -> Result<(&'static str, &'static str)> {
    match arch {
        "x86_64" => Ok(("x86_64-efi", "BOOTX64.EFI")),
        "aarch64" => Ok(("arm64-efi", "BOOTAA64.EFI")),
        _ => bail!("no EFI boot support for arch {arch:?} yet"),
    }
}

/// Where each partition goes on the disk, in MiB. The first MiB holds the
/// partition table, and one MiB at the end holds its backup copy.
#[derive(Debug, Clone, Copy)]
pub struct Layout {
    pub slot_mib: u64,
}

impl Layout {
    pub fn esp_start_mib(&self) -> u64 {
        1
    }

    /// Start of slot `index` (0 for A, 1 for B).
    pub fn slot_start_mib(&self, index: u64) -> u64 {
        1 + ESP_MIB + index * self.slot_mib
    }

    pub fn disk_mib(&self) -> u64 {
        self.slot_start_mib(SLOTS.len() as u64) + 1
    }

    /// The partition table, as a script for `sfdisk`.
    pub fn sfdisk_script(&self) -> String {
        let mut script = String::from("label: gpt\nunit: sectors\n\n");
        script += &format!(
            "start={}, size={}, type={ESP_TYPE}, name=\"{ESP_LABEL}\"\n",
            self.esp_start_mib() * SECTORS_PER_MIB,
            ESP_MIB * SECTORS_PER_MIB,
        );
        for (index, (name, _)) in SLOTS.iter().enumerate() {
            script += &format!(
                "start={}, size={}, type={LINUX_TYPE}, name=\"edel-{}\"\n",
                self.slot_start_mib(index as u64) * SECTORS_PER_MIB,
                self.slot_mib * SECTORS_PER_MIB,
                name.to_lowercase(),
            );
        }
        script
    }
}

/// GRUB's config: picks a slot as described at the top of this module, and
/// boots `kernel` (an Alpine kernel flavor) from it with `cmdline` added.
pub fn grub_cfg(kernel: &str, cmdline: &str) -> String {
    let mut cfg = String::from(
        r#"# Edel OS A/B boot, written by `edel image build`. GRUB starts the
# first slot in ORDER that is OK and has tries left, and counts the try;
# `edel update mark-good` resets the count once the system is up.

serial --unit=0 --speed=115200
terminal_input console serial
terminal_output console serial
set timeout_style=hidden
set timeout=1

# The slots are on the same disk as this partition.
regexp --set=1:disk '^(.*),' "$root"
export disk

set ORDER="A B"
set A_OK=0
set A_TRY=0
set B_OK=0
set B_TRY=0
load_env -f "$prefix/grubenv" ORDER A_OK A_TRY B_OK B_TRY

set slot=""
for s in $ORDER; do
	if [ -z "$slot" ]; then
		set ok=0
		set try=""
		if [ "$s" = "A" ]; then set ok="$A_OK"; set try="$A_TRY"; fi
		if [ "$s" = "B" ]; then set ok="$B_OK"; set try="$B_TRY"; fi
		set next=""
		if [ "$ok" = "1" ]; then
"#,
    );
    for tried in 0..TRIES {
        cfg += &format!(
            "\t\t\tif [ \"$try\" = \"{tried}\" ]; then set next=\"{}\"; fi\n",
            tried + 1
        );
    }
    cfg += r#"		fi
		if [ -n "$next" ]; then
			set slot="$s"
			if [ "$s" = "A" ]; then set A_TRY="$next"; fi
			if [ "$s" = "B" ]; then set B_TRY="$next"; fi
		fi
	fi
done
if [ -z "$slot" ]; then
	# Every slot used its tries. Starting something beats stopping here.
	for s in $ORDER; do
		if [ -z "$slot" ]; then set slot="$s"; fi
	done
	if [ -z "$slot" ]; then set slot="A"; fi
	echo "Edel OS: no slot has tries left; starting slot $slot anyway"
fi
save_env -f "$prefix/grubenv" A_TRY B_TRY
echo "Edel OS: starting slot $slot (A ok=$A_OK try=$A_TRY, B ok=$B_OK try=$B_TRY)"

# Menu entries by number: GRUB 2.12 ignores an entry id in fallback. If the
# slot's kernel cannot even be loaded, the other slot starts right away.
if [ "$slot" = "B" ]; then
	set default=1
	set fallback=0
else
	set default=0
	set fallback=1
fi
"#;
    let cmdline = format!("root=UUID=$uuid rootfstype=ext4 panic=10 {cmdline}");
    let cmdline = cmdline.trim_end();
    for (name, partition) in SLOTS {
        cfg += &format!(
            r#"
menuentry "Edel OS, slot {name}" {{
	set root="$disk,gpt{partition}"
	set uuid=""
	probe --set=uuid --fs-uuid "($disk,gpt{partition})"
	linux /boot/vmlinuz-{kernel} {cmdline}
	initrd /boot/initramfs-{kernel}
}}
"#
        );
    }
    cfg
}

/// GRUB's environment block for a new disk: slot A boots, slot B is empty.
pub fn initial_grubenv() -> String {
    Env::initial()
        .render()
        .expect("the initial variables fit in the block")
}

/// Whether `cmdline` can go into the GRUB config as it is: printable ASCII
/// and nothing GRUB's script would expand or end early.
pub fn is_safe_cmdline(cmdline: &str) -> bool {
    cmdline
        .chars()
        .all(|c| (c.is_ascii_graphic() || c == ' ') && !"\"'\\$;{}#`".contains(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lays_out_the_disk_without_overlaps() {
        let layout = Layout { slot_mib: 1024 };
        assert_eq!(layout.esp_start_mib(), 1);
        assert_eq!(layout.slot_start_mib(0), 65);
        assert_eq!(layout.slot_start_mib(1), 1089);
        assert_eq!(layout.disk_mib(), 2114);
    }

    #[test]
    fn writes_the_partition_table() {
        let script = Layout { slot_mib: 512 }.sfdisk_script();
        assert_eq!(
            script,
            "label: gpt\nunit: sectors\n\n\
             start=2048, size=131072, type=C12A7328-F81F-11D2-BA4B-00A0C93EC93B, name=\"EDEL-ESP\"\n\
             start=133120, size=1048576, type=0FC63DAF-8483-4772-8E79-3D69D8477DE4, name=\"edel-a\"\n\
             start=1181696, size=1048576, type=0FC63DAF-8483-4772-8E79-3D69D8477DE4, name=\"edel-b\"\n"
        );
    }

    #[test]
    fn grub_counts_every_try() {
        let cfg = grub_cfg("virt", "console=ttyS0");
        for (tried, next) in [(0, 1), (1, 2), (2, 3)] {
            assert!(cfg.contains(&format!(
                "if [ \"$try\" = \"{tried}\" ]; then set next=\"{next}\"; fi"
            )));
        }
        assert!(!cfg.contains("\"$try\" = \"3\""));
        assert!(cfg.contains("save_env -f \"$prefix/grubenv\" A_TRY B_TRY"));
    }

    #[test]
    fn grub_boots_each_slot_by_its_own_uuid() {
        let cfg = grub_cfg("virt", "console=ttyS0");
        assert!(cfg.contains("menuentry \"Edel OS, slot B\" {"));
        assert!(cfg.contains("probe --set=uuid --fs-uuid \"($disk,gpt3)\""));
        assert!(cfg.contains(
            "linux /boot/vmlinuz-virt root=UUID=$uuid rootfstype=ext4 panic=10 console=ttyS0\n"
        ));
        assert!(cfg.contains("initrd /boot/initramfs-virt\n"));
    }

    #[test]
    fn knows_the_efi_file_names() {
        assert_eq!(efi_target("x86_64").unwrap(), ("x86_64-efi", "BOOTX64.EFI"));
        assert!(efi_target("riscv64").is_err());
    }

    #[test]
    fn accepts_only_plain_kernel_arguments() {
        assert!(is_safe_cmdline(
            "console=ttyS0 modules=ext4,virtio_blk quiet"
        ));
        assert!(is_safe_cmdline(""));
        assert!(!is_safe_cmdline("init=$x"));
        assert!(!is_safe_cmdline("a\" b"));
        assert!(!is_safe_cmdline("a\nb"));
        assert!(!is_safe_cmdline("a; reboot"));
    }
}
