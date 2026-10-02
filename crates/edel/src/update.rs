//! `edel update`: installs a new system into the A/B slot that is not
//! running, confirms the running slot once it has started, and rolls back
//! by hand (ADR-006). GRUB counts boot tries with the variables described
//! in `grubenv.rs`; `boot.rs` writes the disk layout and `grub.cfg`.
//!
//! These commands change real disks, so they have no dry run: they call
//! tools with `std::process::Command` and check every exit status. The
//! decisions are small pure functions over text and numbers, tested
//! without a disk.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

use crate::grubenv::{Env, Slot};

/// Where edel keeps its runtime files; a tmpfs, so they vanish at reboot.
const RUN_DIR: &str = "/run/edel";
/// The environment block, relative to the EFI system partition.
const ENV_FILE: &str = "EFI/edel/grubenv";

/// A partition of the disk the system runs from.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Part {
    number: u32,
    /// Kernel name, such as `vda2`; the device is `/dev/<name>`.
    name: String,
    /// `major:minor`.
    dev: String,
}

/// The disk the running system is on, found from the mounted root rather
/// than the kernel command line: the slot we write must never be the one
/// in use.
#[derive(Debug)]
pub(crate) struct Disk {
    /// Kernel name of the whole disk, such as `vda`.
    pub(crate) name: String,
    running: Slot,
    root_dev: String,
    parts: Vec<Part>,
}

impl Disk {
    pub(crate) fn find() -> Result<Disk> {
        let mountinfo =
            fs::read_to_string("/proc/self/mountinfo").context("reading /proc/self/mountinfo")?;
        find_disk(&mountinfo, Path::new("/sys"))
    }

    fn part(&self, number: u32) -> Result<&Part> {
        self.parts
            .iter()
            .find(|p| p.number == number)
            .with_context(|| format!("the system disk has no partition {number}"))
    }

    pub(crate) fn device(&self, number: u32) -> Result<PathBuf> {
        Ok(Path::new("/dev").join(&self.part(number)?.name))
    }

    /// The other slot's partition, refusing the one the system runs from.
    fn install_target(&self) -> Result<&Part> {
        let target = self.running.other();
        let part = self.part(target.partition())?;
        if part.dev == self.root_dev {
            bail!("slot {target} holds the running system; not writing to it");
        }
        Ok(part)
    }
}

/// `major:minor` of the device mounted at `/`. The last matching line
/// wins, since a later mount hides an earlier one.
fn root_device(mountinfo: &str) -> Result<&str> {
    mountinfo
        .lines()
        .rev()
        .find_map(|line| {
            let mut fields = line.split_whitespace();
            let dev = fields.nth(2)?;
            (fields.nth(1)? == "/").then_some(dev)
        })
        .context("/proc/self/mountinfo has no entry for /")
}

fn read_trimmed(path: &Path) -> Result<String> {
    Ok(fs::read_to_string(path)
        .with_context(|| format!("reading {}", path.display()))?
        .trim()
        .to_string())
}

/// Finds the root partition in sysfs (`sys` is `/sys`, or a test tree),
/// the disk it belongs to and that disk's partitions.
fn find_disk(mountinfo: &str, sys: &Path) -> Result<Disk> {
    let root_dev = root_device(mountinfo)?.to_string();
    let link = sys.join("dev/block").join(&root_dev);
    let root = fs::canonicalize(&link).with_context(|| format!("resolving {}", link.display()))?;
    if !root.join("partition").exists() {
        bail!("/ is not on a disk partition");
    }
    let number: u32 = read_trimmed(&root.join("partition"))?.parse()?;
    let running = Slot::from_partition(number)
        .context("/ is not on slot A (partition 2) or slot B (partition 3)")?;
    let disk_dir = root.parent().context("the root partition has no disk")?;
    let mut parts = Vec::new();
    for entry in fs::read_dir(disk_dir)? {
        let path = entry?.path();
        if path.join("partition").exists() {
            parts.push(Part {
                number: read_trimmed(&path.join("partition"))?.parse()?,
                name: path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                dev: read_trimmed(&path.join("dev"))?,
            });
        }
    }
    parts.sort_by_key(|p| p.number);
    Ok(Disk {
        name: disk_dir
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        running,
        root_dev,
        parts,
    })
}

/// Before writing `target`: switch it off, so a half-written slot never
/// starts.
fn before_install(env: &mut Env, target: Slot) {
    env.set_slot(target, false, 0);
}

/// After `target` is written and checked: it starts next, with all tries.
fn after_install(env: &mut Env, target: Slot) {
    env.set_order(target);
    env.set_slot(target, true, 0);
}

/// Confirms the running slot. When `ORDER` starts with the other slot,
/// GRUB passed over it because it failed to start: that slot is switched
/// off and returned.
fn confirm(env: &mut Env, running: Slot) -> Option<Slot> {
    let other = running.other();
    let passed_over = env.order().first() == Some(&other);
    if passed_over {
        env.set_slot(other, false, env.tries(other));
    }
    env.set_order(running);
    env.set_slot(running, true, 0);
    passed_over.then_some(other)
}

/// Makes the other slot start next; refused when it is switched off.
fn roll_back(env: &mut Env, running: Slot) -> Result<Slot> {
    let target = running.other();
    if !env.ok(target) {
        bail!("slot {target} is switched off, so there is nothing to roll back to");
    }
    env.set_order(target);
    env.set_slot(target, true, 0);
    Ok(target)
}

/// The EFI system partition, mounted only while edel uses it: FAT keeps
/// data safest when nobody has it open.
struct Esp {
    dir: PathBuf,
    mounted_here: bool,
}

impl Esp {
    fn mount(disk: &Disk) -> Result<Esp> {
        let dir = Path::new(RUN_DIR).join("esp");
        fs::create_dir_all(&dir)?;
        let mountinfo = fs::read_to_string("/proc/self/mountinfo")?;
        let already = mountinfo
            .lines()
            .any(|l| l.split_whitespace().nth(4) == dir.to_str());
        if !already {
            let dev = disk.device(1)?;
            // Name the character set: Alpine's virt kernel defaults to utf8
            // for FAT but does not ship that module.
            run(Command::new("mount")
                .args(["-t", "vfat", "-o", "noatime,iocharset=iso8859-1"])
                .arg(&dev)
                .arg(&dir))
            .with_context(|| format!("cannot mount the EFI system partition {}", dev.display()))?;
        }
        let esp = Esp {
            dir,
            mounted_here: !already,
        };
        if !esp.env_path().is_file() {
            bail!("the EFI system partition has no {ENV_FILE}");
        }
        Ok(esp)
    }

    fn env_path(&self) -> PathBuf {
        self.dir.join(ENV_FILE)
    }

    fn load(&self) -> Result<Env> {
        Env::parse(&fs::read_to_string(self.env_path()).context("reading the GRUB environment")?)
    }

    /// Rewrites the block in place, as GRUB's `save_env` does, so it keeps
    /// its size and its place on the partition.
    fn save(&self, env: &Env) -> Result<()> {
        let file = OpenOptions::new()
            .write(true)
            .open(self.env_path())
            .context("opening the GRUB environment")?;
        file.write_all_at(env.render()?.as_bytes(), 0)?;
        file.sync_all()?;
        Ok(())
    }
}

impl Drop for Esp {
    fn drop(&mut self) {
        if self.mounted_here {
            let _ = Command::new("umount").arg(&self.dir).status();
        }
    }
}

/// One updater at a time: two installs would write the same slot.
struct Lock(PathBuf);

impl Lock {
    fn take() -> Result<Lock> {
        fs::create_dir_all(RUN_DIR)?;
        let path = Path::new(RUN_DIR).join("update.lock");
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                writeln!(file, "{}", std::process::id())?;
                Ok(Lock(path))
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => bail!(
                "another edel update is running; if none is, delete {}",
                path.display()
            ),
            Err(err) => Err(err.into()),
        }
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub(crate) fn run(cmd: &mut Command) -> Result<()> {
    let status = cmd
        .stdin(Stdio::null())
        .status()
        .with_context(|| format!("starting {:?}", cmd.get_program()))?;
    if !status.success() {
        bail!("{:?} failed with {status}", cmd.get_program());
    }
    Ok(())
}

/// Size in bytes of a file or a block device (file metadata says 0 for a
/// device, seeking to its end does not).
fn size_of(path: &Path) -> Result<u64> {
    let mut file = File::open(path).with_context(|| format!("cannot read {}", path.display()))?;
    Ok(file.seek(SeekFrom::End(0))?)
}

/// SHA-256 of the first `len` bytes of `path`.
fn hash_prefix(path: &Path, len: u64) -> Result<Vec<u8>> {
    let mut reader = File::open(path)?.take(len);
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().to_vec())
}

/// Prints both slots and which one is running.
pub fn status() -> Result<()> {
    let disk = Disk::find()?;
    let env = Esp::mount(&disk)?.load()?;
    println!("running: {}", disk.running);
    println!("order: {}", env.get("ORDER").unwrap_or_default());
    for slot in [Slot::A, Slot::B] {
        println!(
            "{slot}: ok={} try={} {}",
            u8::from(env.ok(slot)),
            env.tries(slot),
            disk.device(slot.partition())?.display()
        );
    }
    Ok(())
}

/// Installs the signed release whose `release.toml` is at `location`, a
/// path or an http(s) URL (or, with `unsigned`, the slot image or block
/// device at that path), into the slot that is not running, and makes it
/// start next. The image streams into the slot, decompressed on the way
/// when it is gzipped, and grows to fill the slot.
pub fn install(location: &str, allow_downgrade: bool, unsigned: bool) -> Result<()> {
    let _lock = Lock::take()?;
    let disk = Disk::find()?;
    let slot = disk.running.other();
    let target = Path::new("/dev").join(&disk.install_target()?.name);
    let (mut source, expected, size): (Box<dyn Read>, Option<Vec<u8>>, u64) = if unsigned {
        eprintln!("warning: installing an unsigned image; nothing checked where it came from");
        let path = Path::new(location);
        (Box::new(File::open(path)?), None, size_of(path)?)
    } else {
        let release = crate::release::open_checked(location, allow_downgrade)?;
        (release.reader, Some(release.sha256), release.size)
    };
    let room = size_of(&target)?;
    if size > room {
        bail!("the image ({size} bytes) does not fit in slot {slot} ({room} bytes)");
    }

    let esp = Esp::mount(&disk)?;
    let mut env = esp.load()?;
    before_install(&mut env, slot);
    esp.save(&env)?;

    println!("writing {location} to slot {slot} ({})", target.display());
    let mut dst = OpenOptions::new()
        .write(true)
        .open(&target)
        .with_context(|| format!("opening {}", target.display()))?;
    let (written, streamed) = copy_hashing(&mut source, &mut dst)?;
    dst.sync_all()?;
    drop(dst);
    if expected.as_ref().is_some_and(|e| *e != streamed) || written != size {
        bail!("refused: sha256: the image does not match release.toml; slot {slot} stays off");
    }
    // Drop cached blocks, so the check reads what is on the disk.
    run(Command::new("blockdev").arg("--flushbufs").arg(&target))?;
    if hash_prefix(&target, written)? != streamed {
        bail!("slot {slot} does not match the image after writing; it stays off");
    }

    // e2fsck -p exits 1 when it fixed something, which is fine.
    let fsck = Command::new("e2fsck")
        .arg("-fp")
        .arg(&target)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .status()
        .context("starting e2fsck")?;
    if !matches!(fsck.code(), Some(0 | 1)) {
        bail!("slot {slot} has filesystem errors; it stays off");
    }
    // Images are shipped shrunk; the file system grows to fill the slot.
    run(Command::new("resize2fs")
        .arg(&target)
        .stdout(Stdio::null())
        .stderr(Stdio::null()))?;
    // GRUB hands the kernel root=UUID=..., so the new slot needs a UUID of
    // its own rather than the one of the image it came from.
    run(Command::new("tune2fs")
        .args(["-U", "random"])
        .arg(&target)
        .stdout(Stdio::null()))?;

    after_install(&mut env, slot);
    esp.save(&env)?;
    println!(
        "slot {slot} is ready and starts next time. If it fails to start, \
         Edel OS goes back to slot {} on its own.",
        disk.running
    );
    Ok(())
}

/// Copies `src` into `dst`, returning the bytes written and their SHA-256.
fn copy_hashing(src: &mut dyn Read, dst: &mut dyn Write) -> Result<(u64, Vec<u8>)> {
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut total = 0u64;
    loop {
        let n = src.read(&mut buf).context("reading the image")?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        dst.write_all(&buf[..n]).context("writing the slot")?;
        total += n as u64;
    }
    Ok((total, hasher.finalize().to_vec()))
}

/// Confirms that the running slot works, so GRUB keeps starting it.
pub fn mark_good() -> Result<()> {
    for line in confirm_running()? {
        println!("{line}");
    }
    Ok(())
}

/// Confirms the running slot and returns what happened, one line each.
pub(crate) fn confirm_running() -> Result<Vec<String>> {
    let mut lines = Vec::new();
    let _lock = Lock::take()?;
    let disk = Disk::find()?;
    let esp = Esp::mount(&disk)?;
    let mut env = esp.load()?;
    if let Some(failed) = confirm(&mut env, disk.running) {
        let msg = format!(
            "slot {failed} did not start, so slot {} is running instead; switching slot {failed} off",
            disk.running
        );
        lines.push(format!("edel update: {msg}"));
        let _ = Command::new("logger").args(["-t", "edel", &msg]).status();
        if let Err(err) = record_fallback(failed, disk.running) {
            eprintln!("warning: could not record the fallback: {err:#}");
        }
    }
    esp.save(&env)?;
    lines.push(format!("edel update: slot {} confirmed", disk.running));
    Ok(lines)
}

/// The fallback record shell-ui shows once (M5.9).
fn fallback_record(from: Slot, to: Slot, date: &str) -> String {
    format!("format = 1\nfrom = \"{from}\"\nto = \"{to}\"\ndate = {date}\n")
}

/// Writes `/data/edel/last-fallback.toml`, when the data partition is
/// mounted (M1.3).
fn record_fallback(from: Slot, to: Slot) -> Result<()> {
    if !crate::data::is_mounted("/data")? {
        return Ok(());
    }
    let date = Command::new("date")
        .arg("-u")
        .arg("+%Y-%m-%dT%H:%M:%SZ")
        .output()
        .context("starting date")?;
    let date = String::from_utf8_lossy(&date.stdout).trim().to_string();
    fs::create_dir_all("/data/edel")?;
    fs::write(
        "/data/edel/last-fallback.toml",
        fallback_record(from, to, &date),
    )?;
    Ok(())
}

/// Makes the other slot start at the next boot.
pub fn rollback() -> Result<()> {
    let _lock = Lock::take()?;
    let disk = Disk::find()?;
    let esp = Esp::mount(&disk)?;
    let mut env = esp.load()?;
    let target = roll_back(&mut env, disk.running)?;
    esp.save(&env)?;
    println!(
        "slot {target} starts after the next reboot; slot {} stays installed",
        disk.running
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sysfs tree like a VM's: vda with partitions 1 to 3, root on `root`.
    fn sysfs(test: &str, root: u32) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("edel-{test}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let disk = dir.join("devices/virtio1/block/vda");
        for n in 1..=3 {
            let part = disk.join(format!("vda{n}"));
            fs::create_dir_all(&part).unwrap();
            fs::write(part.join("partition"), format!("{n}\n")).unwrap();
            fs::write(part.join("dev"), format!("254:{n}\n")).unwrap();
        }
        fs::create_dir_all(dir.join("dev/block")).unwrap();
        std::os::unix::fs::symlink(
            format!("../../devices/virtio1/block/vda/vda{root}"),
            dir.join(format!("dev/block/254:{root}")),
        )
        .unwrap();
        dir
    }

    fn mountinfo(root: u32) -> String {
        format!(
            "1 0 0:2 / / rw - rootfs rootfs rw\n\
             22 1 254:{root} / / ro,relatime - ext4 /dev/root rw\n\
             23 22 0:5 / /dev rw - devtmpfs devtmpfs rw\n"
        )
    }

    #[test]
    fn finds_the_running_slot_and_its_disk() {
        let sys = sysfs("finds-slot", 3);
        let disk = find_disk(&mountinfo(3), &sys).unwrap();
        assert_eq!(disk.running, Slot::B);
        assert_eq!(disk.name, "vda");
        assert_eq!(disk.root_dev, "254:3");
        assert_eq!(disk.parts.len(), 3);
        assert_eq!(disk.install_target().unwrap().name, "vda2");
        assert_eq!(disk.device(1).unwrap(), Path::new("/dev/vda1"));
        fs::remove_dir_all(&sys).unwrap();
    }

    #[test]
    fn refuses_a_root_outside_the_slots() {
        let sys = sysfs("outside-slots", 1);
        let err = find_disk(&mountinfo(1), &sys).unwrap_err();
        assert!(err.to_string().contains("not on slot A"));
        fs::remove_dir_all(&sys).unwrap();
    }

    #[test]
    fn never_writes_the_running_slot() {
        let sys = sysfs("running-slot", 2);
        let mut disk = find_disk(&mountinfo(2), &sys).unwrap();
        // A disk whose other slot claims the root's device number.
        disk.parts[2].dev = "254:2".into();
        let err = disk.install_target().unwrap_err();
        assert!(err.to_string().contains("holds the running system"));
        fs::remove_dir_all(&sys).unwrap();
    }

    #[test]
    fn install_switches_the_target_off_then_on_and_first() {
        let mut env = Env::initial();
        before_install(&mut env, Slot::B);
        assert!(!env.ok(Slot::B));
        assert_eq!(env.order(), [Slot::A, Slot::B]);
        after_install(&mut env, Slot::B);
        assert_eq!(env.order(), [Slot::B, Slot::A]);
        assert!(env.ok(Slot::B) && env.ok(Slot::A));
        assert_eq!(env.tries(Slot::B), 0);
    }

    #[test]
    fn confirming_after_a_fallback_switches_the_failed_slot_off() {
        let mut env = Env::parse("ORDER=B A\nA_OK=1\nA_TRY=1\nB_OK=1\nB_TRY=3\n").unwrap();
        assert_eq!(confirm(&mut env, Slot::A), Some(Slot::B));
        assert_eq!(env.order(), [Slot::A, Slot::B]);
        assert!(env.ok(Slot::A) && !env.ok(Slot::B));
        assert_eq!((env.tries(Slot::A), env.tries(Slot::B)), (0, 3));
    }

    #[test]
    fn confirming_a_normal_boot_resets_the_count() {
        let mut env = Env::parse("ORDER=B A\nA_OK=1\nA_TRY=0\nB_OK=1\nB_TRY=1\n").unwrap();
        assert_eq!(confirm(&mut env, Slot::B), None);
        assert_eq!(env.order(), [Slot::B, Slot::A]);
        assert!(env.ok(Slot::A) && env.ok(Slot::B));
        assert_eq!(env.tries(Slot::B), 0);
    }

    #[test]
    fn records_a_fallback_as_toml() {
        let record = fallback_record(Slot::B, Slot::A, "2026-10-02T18:00:00Z");
        let value: toml::Value = toml::from_str(&record).unwrap();
        assert_eq!(value["from"].as_str(), Some("B"));
        assert_eq!(value["to"].as_str(), Some("A"));
        assert_eq!(value["format"].as_integer(), Some(1));
    }

    #[test]
    fn rolls_back_to_a_working_slot() {
        let mut env = Env::parse("ORDER=B A\nA_OK=1\nA_TRY=0\nB_OK=1\nB_TRY=0\n").unwrap();
        assert_eq!(roll_back(&mut env, Slot::B).unwrap(), Slot::A);
        assert_eq!(env.order(), [Slot::A, Slot::B]);
        assert!(env.ok(Slot::B));
    }

    #[test]
    fn refuses_to_roll_back_to_a_slot_that_is_off() {
        let mut env = Env::initial();
        let before = env.clone();
        let err = roll_back(&mut env, Slot::A).unwrap_err();
        assert!(err.to_string().contains("slot B is switched off"));
        assert_eq!(env, before);
    }

    #[test]
    fn hashes_only_the_image_length() {
        let dir = std::env::temp_dir().join(format!("edel-hash-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let (image, slot) = (dir.join("image"), dir.join("slot"));
        fs::write(&image, b"system").unwrap();
        fs::write(&slot, b"system and old bytes after it").unwrap();
        assert_eq!(size_of(&image).unwrap(), 6);
        assert_eq!(
            hash_prefix(&image, 6).unwrap(),
            hash_prefix(&slot, 6).unwrap()
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reads_the_root_device_from_the_last_mount() {
        assert_eq!(root_device(&mountinfo(2)).unwrap(), "254:2");
        assert!(root_device("23 22 0:5 / /dev rw - devtmpfs devtmpfs rw\n").is_err());
    }
}
