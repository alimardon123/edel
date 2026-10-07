//! The bootable A/B disk (ADR-006).
//!
//! Every VM image is a GPT disk with four partitions:
//!
//! | # | Name       | What                                            |
//! |---|------------|-------------------------------------------------|
//! | 1 | `EDEL-ESP` | EFI system partition: GRUB, its config and env  |
//! | 2 | `edel-a`   | Root slot A                                     |
//! | 3 | `edel-b`   | Root slot B, empty until the first update       |
//! | 4 | `edel-data`| Data that outlives every update; grows on first boot |
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
//! attempt. Once the system is up, `edel boot mark-good` resets the count.
//! A slot that fails [`TRIES`] times in a row is passed over, and the next
//! slot boots instead.

use anyhow::{Result, bail};

use crate::grubenv::Env;

/// File system label of the EFI system partition.
pub const ESP_LABEL: &str = "EDEL-ESP";
/// GPT name and file system label of the data partition, partition 4.
pub const DATA_LABEL: &str = "edel-data";
/// The data partition's GPT partition number.
pub const DATA_PARTITION: u32 = 4;
/// Size of the EFI system partition. Holds GRUB (about 1 MiB) with room to
/// grow; 64 MiB is close to the smallest FAT32 that firmware accepts.
pub const ESP_MIB: u64 = 64;
/// Where GRUB looks for its config and env, on the EFI system partition.
pub const GRUB_PREFIX: &str = edel::places::ESP_DIR;
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
    pub data_mib: u64,
}

impl Layout {
    pub fn esp_start_mib(&self) -> u64 {
        1
    }

    /// Start of slot `index` (0 for A, 1 for B).
    pub fn slot_start_mib(&self, index: u64) -> u64 {
        1 + ESP_MIB + index * self.slot_mib
    }

    /// Start of the data partition, right after slot B, so the slots stay
    /// partitions 2 and 3 and the data partition can grow to the end.
    pub fn data_start_mib(&self) -> u64 {
        self.slot_start_mib(SLOTS.len() as u64)
    }

    pub fn disk_mib(&self) -> u64 {
        self.data_start_mib() + self.data_mib + 1
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
        script += &format!(
            "start={}, size={}, type={LINUX_TYPE}, name=\"{DATA_LABEL}\"\n",
            self.data_start_mib() * SECTORS_PER_MIB,
            self.data_mib * SECTORS_PER_MIB,
        );
        script
    }
}

/// Watchdog timeouts the guard relies on (roadmap M1.5): QEMU's, softdog,
/// and the hardware watchdogs of Intel and AMD laptops.
pub const WATCHDOG_ARGS: &str =
    "i6300esb.heartbeat=15 softdog.soft_margin=15 iTCO_wdt.heartbeat=15 sp5100_tco.heartbeat=15";

/// CPU microcode a slot may carry in `/boot` (intel-ucode, amd-ucode). GRUB
/// loads each one present before the initramfs, so the kernel applies it
/// at its very start.
pub const MICROCODE: &[&str] = &["intel-ucode.img", "amd-ucode.img"];

/// GRUB's config: picks a slot as described at the top of this module, and
/// boots `kernel` (an Alpine kernel flavor) from it with `modules=modules`
/// (the image's features' modules, which the initramfs loads before it
/// looks for the root), `WATCHDOG_ARGS` and `cmdline` added, loading the
/// `microcode` files (names in `/boot`, from `MICROCODE`) before the
/// initramfs.
pub fn grub_cfg(kernel: &str, modules: &str, cmdline: &str, microcode: &[&str]) -> String {
    let mut cfg = String::from(
        r#"# Edel OS A/B boot, written by `edel image build`. GRUB starts the
# first slot in ORDER that is OK and has tries left, and counts the try;
# `edel boot mark-good` resets the count once the system is up.

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
	# Every slot used its tries. Starting something beats stopping here:
	# a slot that is OK first, then the first in ORDER.
	for s in $ORDER; do
		if [ -z "$slot" ]; then
			if [ "$s" = "A" ]; then if [ "$A_OK" = "1" ]; then set slot="A"; fi; fi
			if [ "$s" = "B" ]; then if [ "$B_OK" = "1" ]; then set slot="B"; fi; fi
		fi
	done
	for s in $ORDER; do
		if [ -z "$slot" ]; then set slot="$s"; fi
	done
	if [ -z "$slot" ]; then set slot="A"; fi
	echo "Edel OS: no slot has tries left; starting slot $slot anyway"
fi
save_env -f "$prefix/grubenv" A_TRY B_TRY
echo "Edel OS: starting slot $slot (A ok=$A_OK try=$A_TRY, B ok=$B_OK try=$B_TRY)"

# Menu entries by number: GRUB 2.12 ignores an entry id in fallback. If the
# slot's kernel cannot even be loaded, the other slot starts right away,
# but only when it is OK: a switched-off slot may be half written.
if [ "$slot" = "B" ]; then
	set default=1
	if [ "$A_OK" = "1" ]; then set fallback=0; fi
else
	set default=0
	if [ "$B_OK" = "1" ]; then set fallback=1; fi
fi
"#;
    let cmdline = format!(
        "root=PARTUUID=$partuuid rootfstype=ext4 panic=10 modules={modules} {WATCHDOG_ARGS} {cmdline}"
    );
    let cmdline = cmdline.trim_end();
    let early: String = microcode.iter().map(|f| format!("/boot/{f} ")).collect();
    for (name, partition) in SLOTS {
        cfg += &format!(
            r#"
menuentry "Edel OS, slot {name}" {{
	set root="$disk,gpt{partition}"
	set partuuid=""
	probe --set=partuuid --part-uuid "($disk,gpt{partition})"
	linux /boot/vmlinuz-{kernel} {cmdline}
	initrd {early}/boot/initramfs-{kernel}
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
        let layout = Layout {
            slot_mib: 1024,
            data_mib: 64,
        };
        assert_eq!(layout.esp_start_mib(), 1);
        assert_eq!(layout.slot_start_mib(0), 65);
        assert_eq!(layout.slot_start_mib(1), 1089);
        assert_eq!(layout.data_start_mib(), 2113);
        assert_eq!(layout.disk_mib(), 2178);
    }

    #[test]
    fn writes_the_partition_table() {
        let script = Layout {
            slot_mib: 512,
            data_mib: 64,
        }
        .sfdisk_script();
        assert_eq!(
            script,
            "label: gpt\nunit: sectors\n\n\
             start=2048, size=131072, type=C12A7328-F81F-11D2-BA4B-00A0C93EC93B, name=\"EDEL-ESP\"\n\
             start=133120, size=1048576, type=0FC63DAF-8483-4772-8E79-3D69D8477DE4, name=\"edel-a\"\n\
             start=1181696, size=1048576, type=0FC63DAF-8483-4772-8E79-3D69D8477DE4, name=\"edel-b\"\n\
             start=2230272, size=131072, type=0FC63DAF-8483-4772-8E79-3D69D8477DE4, name=\"edel-data\"\n"
        );
        // Some phone bootloaders break on GPT names over 24 characters.
        for line in script.lines().filter(|l| l.contains("name=")) {
            let name = line.rsplit("name=").next().unwrap().trim_matches('"');
            assert!(name.len() <= 24, "{name} is too long");
        }
    }

    #[test]
    fn grub_counts_every_try() {
        let cfg = grub_cfg("virt", "ext4,overlay", "console=ttyS0", &[]);
        for (tried, next) in [(0, 1), (1, 2), (2, 3)] {
            assert!(cfg.contains(&format!(
                "if [ \"$try\" = \"{tried}\" ]; then set next=\"{next}\"; fi"
            )));
        }
        assert!(!cfg.contains("\"$try\" = \"3\""));
        assert!(cfg.contains("save_env -f \"$prefix/grubenv\" A_TRY B_TRY"));
        // GRUB falls back only to a slot that is OK, and when every slot
        // used its tries it starts one that is OK first.
        assert!(cfg.contains("if [ \"$A_OK\" = \"1\" ]; then set fallback=0; fi"));
        assert!(cfg.contains("if [ \"$B_OK\" = \"1\" ]; then set fallback=1; fi"));
        assert!(cfg.contains(
            "if [ \"$s\" = \"A\" ]; then if [ \"$A_OK\" = \"1\" ]; then set slot=\"A\"; fi; fi"
        ));
    }

    #[test]
    fn grub_boots_each_slot_by_its_own_partition() {
        let cfg = grub_cfg("virt", "ext4,overlay", "console=ttyS0", &[]);
        assert!(cfg.contains("menuentry \"Edel OS, slot B\" {"));
        // The partition's GPT id, never the file system's UUID, which a
        // slot keeps from the image it was written from (M1.12).
        assert!(cfg.contains("probe --set=partuuid --part-uuid \"($disk,gpt3)\""));
        assert!(!cfg.contains("UUID=$uuid"));
        assert!(cfg.contains(
            "linux /boot/vmlinuz-virt root=PARTUUID=$partuuid rootfstype=ext4 panic=10 modules=ext4,overlay i6300esb."
        ));
        assert!(cfg.contains(" sp5100_tco.heartbeat=15 console=ttyS0\n"));
        assert!(cfg.contains("initrd /boot/initramfs-virt\n"));
        let laptop = grub_cfg("lts", "ext4", "", MICROCODE);
        assert!(
            laptop
                .contains("initrd /boot/intel-ucode.img /boot/amd-ucode.img /boot/initramfs-lts\n")
        );
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
