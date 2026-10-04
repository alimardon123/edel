//! `edel install DISK --system FILE` (roadmap M2.4): makes a blank or old
//! disk an Edel OS machine. It lays out the disk as images are laid out
//! (`boot.rs`) with slots of `SLOT_MIB` (M3.3b), copies the running slot
//! into slot A, grows its file system to the slot and gives it a fresh UUID,
//! writes the EFI system partition from the slot's own boot loader with an
//! initial environment block, and creates the data partition holding the
//! system file. It never writes the disk it runs from. The plan it shows is
//! `edel::install::Plan`.

use std::fs::{self, File};
use std::io::{self, BufRead, IsTerminal, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread::sleep;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use edel::install::{Disk, Partition, Plan, SLOT_MIB, parse_blkid};
use edel::system;

use crate::boot::{self, DATA_LABEL, ESP_LABEL, GRUB_PREFIX, Layout};
use crate::loader::SLOT_DIR;
use crate::update::{self, Lock, run};

/// The external tools a real install runs, and the package each comes
/// from; busybox's `mount`, `umount` and `blkid` come with the base. A test
/// holds `images/vm.toml` to this list.
pub const TOOLS: &[(&str, &str)] = &[
    ("sfdisk", "sfdisk"),
    ("mkfs.vfat", "dosfstools"),
    ("mkfs.ext4", "e2fsprogs"),
    ("e2fsck", "e2fsprogs"),
    ("tune2fs", "e2fsprogs"),
    ("resize2fs", "e2fsprogs-extra"),
];

const MIB: u64 = 1024 * 1024;
/// The first block of a slot: it holds the ext4 superblock and its UUID.
const HEAD: u64 = 4096;
/// Exit code when the plan was shown but nobody could confirm it.
const NOT_CONFIRMED: i32 = 3;

/// The tools in `TOOLS` this system lacks, so install can stop before it
/// erases anything.
fn missing_tools() -> Vec<String> {
    let path = std::env::var("PATH").unwrap_or_else(|_| "/usr/sbin:/usr/bin:/sbin:/bin".into());
    TOOLS
        .iter()
        .filter(|(tool, _)| {
            !path
                .split(':')
                .any(|dir| Path::new(dir).join(tool).exists())
        })
        .map(|(tool, package)| format!("{tool} (package {package})"))
        .collect()
}

/// `/dev/sda` and `sda` both name the disk `sda`.
fn disk_name(disk: &str) -> &str {
    disk.trim_start_matches("/dev/")
}

/// The kernel name of partition `number` of `disk`: `sda1`, `nvme0n1p1`.
fn partition_name(disk: &str, number: u32) -> String {
    if disk.ends_with(|c: char| c.is_ascii_digit()) {
        format!("{disk}p{number}")
    } else {
        format!("{disk}{number}")
    }
}

fn read_number(path: &Path) -> Result<u64> {
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(text.trim().parse()?)
}

/// The target disk as sysfs and blkid see it.
fn describe(name: &str) -> Result<Disk> {
    let sys = Path::new("/sys/block").join(name);
    if !sys.exists() {
        bail!("/dev/{name} is not a whole disk; name a disk such as /dev/sda, not a partition");
    }
    let model = fs::read_to_string(sys.join("device/model"))
        .ok()
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty());
    let mut partitions = Vec::new();
    for entry in fs::read_dir(&sys)? {
        let path = entry?.path();
        if !path.join("partition").exists() {
            continue;
        }
        let part = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let out = Command::new("blkid")
            .arg(Path::new("/dev").join(&part))
            .output();
        let line = out
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default();
        let (mut label, mut fs) = parse_blkid(line.trim());
        // blkid says nothing about a partition it may not open, which is
        // not the same as an empty one (a dry run needs no root).
        if line.trim().is_empty() && File::open(Path::new("/dev").join(&part)).is_err() {
            label = Some("unknown".into());
            fs = Some("run as root to see".into());
        }
        partitions.push(Partition {
            name: part,
            label,
            fs,
            bytes: read_number(&path.join("size"))? * 512,
        });
    }
    partitions.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Disk {
        name: name.to_string(),
        model,
        bytes: read_number(&sys.join("size"))? * 512,
        partitions,
    })
}

/// `edel install DISK --system FILE`.
pub fn install(disk: &str, system_file: &Path, dry_run: bool, yes: bool) -> Result<()> {
    let name = disk_name(disk);
    let running = update::Disk::find()?;
    if name == running.name {
        bail!("/dev/{name} is the disk this system runs from; install to another disk");
    }
    let mounts = fs::read_to_string("/proc/mounts")?;
    if mounts
        .lines()
        .any(|l| l.starts_with(&format!("/dev/{name}")))
    {
        bail!("/dev/{name} has a mounted file system; unmount it first");
    }
    let text = fs::read_to_string(system_file)
        .with_context(|| format!("reading {}", system_file.display()))?;
    let problems =
        system::check(&text).with_context(|| format!("checking {}", system_file.display()))?;
    if !problems.is_empty() {
        bail!(
            "{} has problems, so nothing was changed:\n{}",
            system_file.display(),
            problems.join("\n")
        );
    }
    let slot = running.running_device()?;
    let slot_name = slot
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let running_mib =
        read_number(&Path::new("/sys/class/block").join(&slot_name).join("size"))? * 512 / MIB;
    let plan = Plan {
        disk: describe(name)?,
        slot_mib: SLOT_MIB.max(running_mib),
        system_file: system_file.display().to_string(),
    };
    plan.check()?;
    print!("{plan}");
    if dry_run {
        println!("This was only the plan; nothing was changed.");
        return Ok(());
    }
    if !yes {
        if !io::stdin().is_terminal() {
            println!("No terminal to confirm on and no --yes, so nothing was changed.");
            io::stdout().flush()?;
            std::process::exit(NOT_CONFIRMED);
        }
        print!("Type {name} to erase it and install, or anything else to stop: ");
        io::stdout().flush()?;
        let mut answer = String::new();
        io::stdin().lock().read_line(&mut answer)?;
        if answer.trim() != name {
            bail!("stopped; nothing was changed");
        }
    }
    let missing = missing_tools();
    if !missing.is_empty() {
        bail!(
            "this system lacks {}, so nothing was changed",
            missing.join(", ")
        );
    }
    let _lock = Lock::take("install", "edel install")?;
    write(&plan, &slot, running_mib, system_file)
}

/// Waits for the kernel's device node of a new partition; after a second
/// without one, asks mdev to scan, in case nothing handles hotplug events.
fn wait_for(device: &Path) -> Result<()> {
    for tick in 0..100 {
        if device.exists() {
            return Ok(());
        }
        if tick == 10 {
            let _ = Command::new("mdev").arg("-s").status();
        }
        sleep(Duration::from_millis(100));
    }
    bail!("{} did not appear after partitioning", device.display())
}

/// Mounts `device` at `dir` for as long as the guard lives.
struct Mounted(PathBuf);

impl Mounted {
    fn new(device: &Path, dir: &Path, args: &[&str]) -> Result<Mounted> {
        fs::create_dir_all(dir)?;
        run(Command::new("mount").args(args).arg(device).arg(dir))?;
        Ok(Mounted(dir.to_path_buf()))
    }
}

impl Drop for Mounted {
    fn drop(&mut self) {
        let _ = Command::new("umount").arg(&self.0).status();
    }
}

/// Writes the disk: `running_mib` MiB of the running slot become the start
/// of slot A, whose file system then grows to fill it.
fn write(plan: &Plan, slot: &Path, running_mib: u64, system_file: &Path) -> Result<()> {
    let name = &plan.disk.name;
    let disk = Path::new("/dev").join(name);
    let part = |n: u32| Path::new("/dev").join(partition_name(name, n));
    let layout = Layout {
        slot_mib: plan.slot_mib,
        data_mib: plan.data_mib(),
    };

    println!("edel install: partitioning {}", disk.display());
    let mut sfdisk = Command::new("sfdisk")
        .args(["--quiet", "--wipe", "always", "--wipe-partitions", "always"])
        .arg(&disk)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .context("starting sfdisk")?;
    sfdisk
        .stdin
        .take()
        .context("sfdisk has no input")?
        .write_all(layout.sfdisk_script().as_bytes())?;
    if !sfdisk.wait()?.success() {
        bail!("sfdisk could not partition {}", disk.display());
    }
    for n in 1..=4 {
        wait_for(&part(n))?;
    }

    println!("edel install: copying the running system into slot A");
    let mut half = HalfCopy::new(part(2));
    let mut from = File::open(slot).with_context(|| format!("reading {}", slot.display()))?;
    let mut to = fs::OpenOptions::new().write(true).open(part(2))?;
    // The first block, which holds the file system's UUID, goes last.
    from.seek(SeekFrom::Start(HEAD))?;
    to.seek(SeekFrom::Start(HEAD))?;
    io::copy(
        &mut io::Read::take(&mut from, running_mib * MIB - HEAD),
        &mut to,
    )?;
    to.sync_all()?;
    let mut head = vec![0; HEAD as usize];
    from.seek(SeekFrom::Start(0))?;
    from.read_exact(&mut head)?;
    to.seek(SeekFrom::Start(0))?;
    to.write_all(&head)?;
    to.sync_all()?;
    drop(to);
    let status = Command::new("e2fsck").arg("-fp").arg(part(2)).status()?;
    if !matches!(status.code(), Some(0 | 1)) {
        bail!("e2fsck found errors in the copied slot ({status})");
    }
    if plan.slot_mib > running_mib {
        run(Command::new("resize2fs").arg(part(2)))?;
    }
    run(Command::new("tune2fs").args(["-U", "random"]).arg(part(2)))?;
    half.done();

    println!("edel install: writing the boot loader");
    run(Command::new("mkfs.vfat")
        .args(["-F", "32", "-n", ESP_LABEL])
        .arg(part(1)))?;
    {
        let esp = Mounted::new(
            &part(1),
            Path::new("/run/edel/install/esp"),
            &["-t", "vfat", "-o", "iocharset=iso8859-1"],
        )?;
        let (_, efi_name) = boot::efi_target(std::env::consts::ARCH)?;
        let boot_dir = esp.0.join("EFI/BOOT");
        let edel_dir = esp.0.join(GRUB_PREFIX.trim_start_matches('/'));
        fs::create_dir_all(&boot_dir)?;
        fs::create_dir_all(&edel_dir)?;
        let slot_dir = Path::new(SLOT_DIR);
        fs::copy(slot_dir.join(efi_name), boot_dir.join(efi_name))?;
        for file in ["grub.cfg", "loader.toml"] {
            fs::copy(slot_dir.join(file), edel_dir.join(file))?;
        }
        fs::write(edel_dir.join("grubenv"), boot::initial_grubenv())?;
    }

    println!("edel install: creating the data partition");
    run(Command::new("mkfs.ext4")
        .args(["-q", "-L", DATA_LABEL])
        .arg(part(4)))?;
    {
        let data = Mounted::new(
            &part(4),
            Path::new("/run/edel/install/data"),
            &["-t", "ext4"],
        )?;
        fs::create_dir_all(data.0.join("edel"))?;
        fs::copy(system_file, data.0.join("edel/system.toml"))?;
    }
    run(&mut Command::new("sync"))?;
    println!(
        "edel install: done; restart from {} to use it",
        disk.display()
    );
    Ok(())
}

/// Slot A while the running system is copied into it. Until `done`, after
/// `tune2fs -U random`, it carries the running root's UUID, by which the
/// stick's GRUB and initramfs find their root, so a copy that stops half
/// way could be started instead of the stick (M2 and M3 review). The first
/// block is written last and cleared again when the copy fails, leaving a
/// window of the e2fsck, resize2fs and tune2fs seconds for a power cut.
struct HalfCopy {
    partition: PathBuf,
    done: bool,
}

impl HalfCopy {
    fn new(partition: PathBuf) -> HalfCopy {
        HalfCopy {
            partition,
            done: false,
        }
    }

    fn done(&mut self) {
        self.done = true;
    }
}

impl Drop for HalfCopy {
    fn drop(&mut self) {
        if self.done {
            return;
        }
        if let Ok(mut part) = fs::OpenOptions::new().write(true).open(&self.partition) {
            let _ = part.write_all(&[0; HEAD as usize]);
            let _ = part.sync_all();
        }
        eprintln!(
            "edel install: stopped; cleared the start of {} so it cannot be started",
            self.partition.display()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_partitions_the_way_the_kernel_does() {
        assert_eq!(partition_name("sda", 2), "sda2");
        assert_eq!(partition_name("vdb", 4), "vdb4");
        assert_eq!(partition_name("nvme0n1", 1), "nvme0n1p1");
        assert_eq!(partition_name("mmcblk0", 3), "mmcblk0p3");
        assert_eq!(disk_name("/dev/vdb"), "vdb");
    }

    #[test]
    fn every_bootable_image_has_every_tool_install_runs() {
        let images = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../images");
        for name in ["vm", "laptop"] {
            let def = crate::def::ImageDef::load(&images.join(format!("{name}.toml"))).unwrap();
            for (tool, package) in TOOLS {
                assert!(
                    def.packages.iter().any(|p| p == package),
                    "{tool} needs {package} in a feature of images/{name}.toml"
                );
            }
        }
    }
}
