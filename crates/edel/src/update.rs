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
use crate::release::os_release_value;
use edel::i18n::{tr, trf};

const MIB: u64 = 1 << 20;
/// Where edel keeps its runtime files; a tmpfs, so they vanish at reboot.
const RUN_DIR: &str = edel::places::RUN_DIR;

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
        let mountinfo = fs::read_to_string("/proc/self/mountinfo")
            .context(tr("could not read /proc/self/mountinfo"))?;
        find_disk(&mountinfo, Path::new("/sys"))
    }

    fn part(&self, number: u32) -> Result<&Part> {
        self.parts
            .iter()
            .find(|p| p.number == number)
            .with_context(|| {
                trf(
                    "the system disk has no partition {number}",
                    &[("number", &number.to_string())],
                )
            })
    }

    pub(crate) fn device(&self, number: u32) -> Result<PathBuf> {
        Ok(Path::new("/dev").join(&self.part(number)?.name))
    }

    /// The partition the running system is on.
    pub(crate) fn running_device(&self) -> Result<PathBuf> {
        self.device(self.running.partition())
    }

    /// The other slot's partition, refusing the one the system runs from.
    fn install_target(&self) -> Result<&Part> {
        let target = self.running.other();
        let part = self.part(target.partition())?;
        if part.dev == self.root_dev {
            bail!(
                "{}",
                trf(
                    "slot {slot} holds the running system; not writing to it",
                    &[("slot", &target.to_string())]
                )
            );
        }
        Ok(part)
    }
}

/// `major:minor` of the device mounted at `/` (`edel::install`'s reader,
/// which Settings shares).
fn root_device(mountinfo: &str) -> Result<&str> {
    edel::install::root_device(mountinfo).context(tr("/proc/self/mountinfo has no entry for /"))
}

fn read_trimmed(path: &Path) -> Result<String> {
    Ok(fs::read_to_string(path)
        .with_context(|| {
            trf(
                "could not read {path}",
                &[("path", &path.display().to_string())],
            )
        })?
        .trim()
        .to_string())
}

/// Finds the root partition in sysfs (`sys` is `/sys`, or a test tree),
/// the disk it belongs to and that disk's partitions.
fn find_disk(mountinfo: &str, sys: &Path) -> Result<Disk> {
    let root_dev = root_device(mountinfo)?.to_string();
    let link = sys.join("dev/block").join(&root_dev);
    let root = fs::canonicalize(&link).with_context(|| {
        trf(
            "could not resolve {path}",
            &[("path", &link.display().to_string())],
        )
    })?;
    if !root.join("partition").exists() {
        bail!(
            "{}",
            tr(
                "this system does not run from an Edel OS disk, so it has no slots to update or roll back: / is not on a disk partition"
            )
        );
    }
    let number: u32 = read_trimmed(&root.join("partition"))?.parse()?;
    let running = Slot::from_partition(number)
        .context(tr(
            "this system does not run from an Edel OS disk, so it has no slots to update or roll back: / is not on slot A (partition 2) or slot B (partition 3)",
        ))?;
    let disk_dir = root
        .parent()
        .context(tr("the root partition has no disk"))?;
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
    let first = env.order().first() == Some(&other);
    // GRUB counts a try before it starts a slot, so an OK slot first in
    // ORDER that has no try yet was installed or rolled back to during this
    // boot: it is pending, not failed, and keeps its place.
    if first && env.ok(other) && env.tries(other) == 0 {
        env.set_slot(running, true, 0);
        return None;
    }
    // An OK slot first in ORDER that used a try was passed over; one that
    // is off was an install that never finished, not a failed boot.
    let passed_over = first && env.ok(other);
    if passed_over {
        env.set_slot(other, false, env.tries(other));
    }
    env.set_order(running);
    env.set_slot(running, true, 0);
    passed_over.then_some(other)
}

/// Whether the running slot may be the only way back, so the other slot can
/// be written: GRUB counted a try for it and the guard has not confirmed it
/// yet, so the other slot is still the version that last worked (the
/// review of 2026-10-25). A safe start, when no slot had a try left, is
/// settled: there is no better way back to keep, and an update may be the
/// fix.
fn running_is_settled(env: &Env, running: Slot, safe: bool) -> bool {
    safe || env.tries(running) == 0
}

/// Makes the other slot start next; refused when it is switched off.
fn roll_back(env: &mut Env, running: Slot) -> Result<Slot> {
    let target = running.other();
    if !env.ok(target) {
        bail!(
            "{}",
            trf(
                "slot {slot} is switched off, so there is nothing to roll back to: it holds no update, or the update failed to start; the running system is unchanged",
                &[("slot", &target.to_string())]
            )
        );
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
    /// Mounts the EFI system partition on a directory of this process's
    /// own, so a status that ends never unmounts it under a confirmation
    /// still writing (the kernel shares one FAT superblock between mounts).
    fn mount(disk: &Disk) -> Result<Esp> {
        let dir = Path::new(RUN_DIR).join(format!("esp.{}", std::process::id()));
        fs::create_dir_all(&dir)?;
        let dev = disk.device(crate::boot::ESP_PARTITION)?;
        // Name the character set: Alpine's virt kernel defaults to utf8 for
        // FAT but does not ship that module. dirsync writes every directory
        // change at once, so a rename in the loader swap never leaves an
        // entry pointing at freed clusters after a power cut (M1.8).
        run(Command::new("mount")
            .args(["-t", "vfat", "-o", "noatime,dirsync,iocharset=iso8859-1"])
            .arg(&dev)
            .arg(&dir))
        .with_context(|| {
            trf(
                "could not open the start-up partition {device}, which says which version starts next; nothing was changed; if this repeats, run edel report and send the file it writes",
                &[("device", &dev.display().to_string())],
            )
        })?;
        let esp = Esp {
            dir,
            mounted_here: true,
        };
        if !esp.env_path().is_file() {
            bail!(
                "{}",
                trf(
                    "the EFI system partition has no {file}, so the slots cannot be read; this disk was not made by edel install, or it is damaged",
                    &[(
                        "file",
                        &edel::places::on_esp(Path::new("/"), edel::places::GRUBENV)
                            .display()
                            .to_string(),
                    )]
                )
            );
        }
        Ok(esp)
    }

    fn env_path(&self) -> PathBuf {
        edel::places::on_esp(&self.dir, edel::places::GRUBENV)
    }

    fn load(&self) -> Result<Env> {
        Env::parse(
            &fs::read_to_string(self.env_path())
                .context(tr("could not read which version starts next from the start-up partition; nothing was changed; if this repeats, run edel report and send the file it writes"))?,
        )
    }

    /// Rewrites the block in place, as GRUB's `save_env` does, so it keeps
    /// its size and its place on the partition.
    fn save(&self, env: &Env) -> Result<()> {
        let file = OpenOptions::new()
            .write(true)
            .open(self.env_path())
            .context(tr("could not open the start-up partition's record of which version starts next; nothing was changed; if this repeats, run edel report and send the file it writes"))?;
        file.write_all_at(env.render()?.as_bytes(), 0)?;
        file.sync_all()?;
        Ok(())
    }
}

/// Writes `text` as `name` beside GRUB's environment block on the running
/// disk's EFI system partition, through a new file and a rename, and
/// returns where it is on the partition. The desktop stick leaves its
/// `edel report` there, readable on any computer (M3.4).
pub(crate) fn write_beside_grubenv(name: &str, text: &str) -> Result<String> {
    let _lock = Lock::take("update", "edel report --esp")?;
    let esp = Esp::mount(&Disk::find()?)?;
    let env = esp.env_path();
    let dir = env
        .parent()
        .context(tr("GRUB's environment block has no directory"))?;
    // In place, not through a rename: on FAT a rename over an existing
    // file is two directory writes, and a power cut between them can
    // cross-link clusters in the directory that holds grub.cfg and
    // grubenv. A torn report is harmless; the next boot writes it again.
    let mut file = File::create(dir.join(name))?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    let shown = dir.strip_prefix(&esp.dir).unwrap_or(dir).join(name);
    Ok(format!("/{}", shown.display()))
}

impl Drop for Esp {
    fn drop(&mut self) {
        if self.mounted_here {
            let _ = Command::new("umount").arg(&self.dir).status();
            let _ = fs::remove_dir(&self.dir);
        }
    }
}

/// One writer at a time: two installs would write the same slot, two
/// applies the same account files. `name` names the lock file; `what`, the
/// command taking it, is written beside the pid, so a refused command says
/// who holds the lock. The lock is the kernel's (`flock`) on the open
/// file, held for the life of `Lock` and let go when the process ends,
/// however it ends, so a killed updater leaves no lock and two commands
/// can never both hold it; the file itself stays, and only its words
/// change.
pub(crate) struct Lock {
    _held: File,
}

impl Lock {
    pub(crate) fn take(name: &str, what: &str) -> Result<Lock> {
        fs::create_dir_all(RUN_DIR)
            .map_err(|err| io_failure(err, &trf("could not write {path}", &[("path", RUN_DIR)])))?;
        Lock::take_at(&Path::new(RUN_DIR).join(format!("{name}.lock")), what)
    }

    fn take_at(path: &Path, what: &str) -> Result<Lock> {
        let failed = |err| {
            io_failure(
                err,
                &trf(
                    "could not take the lock {path}",
                    &[("path", &path.display().to_string())],
                ),
            )
        };
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(failed)?;
        if !try_flock(&file).map_err(failed)? {
            let mut held = String::new();
            let _ = file.read_to_string(&mut held);
            bail!(
                "{}",
                trf(
                    "{command} has to wait: {holder}; try again when it is done",
                    &[("command", what), ("holder", &holder(&held))]
                )
            );
        }
        file.set_len(0)?;
        file.write_all_at(format!("{} {what}\n", std::process::id()).as_bytes(), 0)?;
        Ok(Lock { _held: file })
    }
}

/// Takes the kernel's exclusive lock on `file` without waiting: whether
/// it was free.
fn try_flock(file: &File) -> std::io::Result<bool> {
    use std::os::fd::AsRawFd;
    // SAFETY: flock only reads the descriptor, which `file` keeps open.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
        return Ok(true);
    }
    let err = std::io::Error::last_os_error();
    if err.raw_os_error() == Some(libc::EWOULDBLOCK) {
        Ok(false)
    } else {
        Err(err)
    }
}

/// Whether a command holds the lock file at `path` now, by trying the
/// kernel's lock on it (let go again at once).
pub(crate) fn is_held(path: &Path) -> bool {
    File::open(path)
        .and_then(|file| try_flock(&file))
        .is_ok_and(|free| !free)
}

/// `err` as the message for what failed (`doing`): a refusal by the system
/// says to run the command as root, which is what a first try as an
/// ordinary user needs to read (docs/MESSAGES.md).
pub(crate) fn io_failure(err: std::io::Error, doing: &str) -> anyhow::Error {
    if err.kind() == std::io::ErrorKind::PermissionDenied {
        anyhow::Error::new(edel::exit::Coded::new(
            edel::exit::REFUSED,
            refused(doing, &typed()),
        ))
    } else {
        anyhow::Error::new(err).context(doing.to_string())
    }
}

/// The refusal of `command` for what failed (`doing`), naming the command
/// to run instead (the messages audit's row 33).
fn refused(doing: &str, command: &str) -> String {
    trf(
        "{doing}: permission denied; {command} changes the system, so it needs an administrator: doas {command}",
        &[("doing", doing), ("command", command)],
    )
}

/// The command the person ran, `edel update --check`, as they typed it.
fn typed() -> String {
    let words: Vec<String> = std::env::args().skip(1).collect();
    format!("edel {}", words.join(" "))
}

/// Who holds a lock, from the lock file's `PID COMMAND`.
fn holder(held: &str) -> String {
    match held.trim().split_once(' ') {
        Some((pid, what)) => trf(
            "{command} (pid {pid}) is running",
            &[("command", what), ("pid", pid)],
        ),
        None if !held.trim().is_empty() => trf("pid {pid} is running", &[("pid", held.trim())]),
        None => tr("another command is starting").into(),
    }
}

/// Whether a process with this pid runs, from `/proc` (the lock files and
/// the compositor's state file, M7.7c).
pub(crate) fn alive(pid: u32) -> bool {
    Path::new("/proc").join(pid.to_string()).exists()
}

pub(crate) fn run(cmd: &mut Command) -> Result<()> {
    let status = cmd.stdin(Stdio::null()).status().with_context(|| {
        trf(
            "could not start {program}",
            &[("program", &cmd.get_program().to_string_lossy())],
        )
    })?;
    if !status.success() {
        bail!(
            "{}",
            trf(
                "{program} stopped with code {code}",
                &[
                    ("program", &cmd.get_program().to_string_lossy()),
                    (
                        "code",
                        &status.code().map_or("none".into(), |c| c.to_string())
                    ),
                ]
            )
        );
    }
    Ok(())
}

/// Size in bytes of a file or a block device (file metadata says 0 for a
/// device, seeking to its end does not).
fn size_of(path: &Path) -> Result<u64> {
    let mut file = File::open(path).with_context(|| {
        trf(
            "cannot read {path}",
            &[("path", &path.display().to_string())],
        )
    })?;
    Ok(file.seek(SeekFrom::End(0))?)
}

/// SHA-256 of the first `len` bytes of `path`.
pub(crate) fn hash_prefix(path: &Path, len: u64) -> Result<Vec<u8>> {
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

/// GRUB's environment, read from the EFI system partition, for the guard
/// (M6.17).
pub(crate) fn boot_env() -> Result<Env> {
    let disk = Disk::find()?;
    Esp::mount(&disk)?.load()
}

/// `edel platform status`: this system's platform level and what it gives
/// apps (M8.8), read from os-release, the packages file and the kernel.
pub fn platform_status() -> Result<()> {
    let level = fs::read_to_string("/usr/lib/os-release")
        .ok()
        .and_then(|text| os_release_value(&text, edel::platform::OS_RELEASE_KEY))
        .and_then(|value| value.parse::<u32>().ok());
    let packages = fs::read_to_string(edel::places::PACKAGES_FILE).unwrap_or_default();
    let kernel = fs::read_to_string("/proc/sys/kernel/osrelease").ok();
    for line in edel::platform::status(level, &packages, kernel.as_deref().map(str::trim)) {
        println!("{line}");
    }
    Ok(())
}

/// Prints both slots and which one is running.
pub fn status() -> Result<()> {
    // The three facts a person reads first (M8.14), before anything that
    // needs root, so a person without it sees them too.
    for line in edel::security::now().lines() {
        println!("{line}");
    }
    // The versions installed need the start-up partition, which only root
    // reads; a person sees the facts above and how to see the rest (the
    // messages audit's row 39), and Settings' Updates page shows them.
    if edel::users::current() != Some(0) {
        eprintln!(
            "warning: {}",
            tr("the versions installed are shown to administrators only: doas edel status")
        );
        return Ok(());
    }
    let disk = Disk::find()?;
    let env = Esp::mount(&disk)?.load()?;
    println!("running: {}", disk.running);
    println!("order: {}", env.get("ORDER").unwrap_or_default());
    for slot in [Slot::A, Slot::B] {
        let device = disk.device(slot.partition())?;
        let os_release = if slot == disk.running {
            fs::read_to_string("/usr/lib/os-release").ok()
        } else {
            slot_os_release(&device)
        };
        let version = os_release
            .and_then(|text| os_release_value(&text, "VERSION_ID"))
            .unwrap_or_else(|| "empty".into());
        println!(
            "{slot}: ok={} try={} {} {version}",
            u8::from(env.ok(slot)),
            env.tries(slot),
            device.display()
        );
    }
    // The guard kept an unhealthy slot running, as no slot had a try left
    // (M6.17).
    if let Some(note) = fs::read_to_string(edel::places::NO_HEALTHY_SLOT)
        .ok()
        .and_then(|note| crate::guard::stood_down_note(&note))
    {
        println!("health: {note}");
    }
    // Developer mode, while it is on or changes at the next start (M7.1).
    if let Some(state) = crate::data::developer_state(
        Path::new(edel::places::DEVELOPER_FLAG).exists(),
        fs::read_to_string(edel::places::DEVELOPER_NOW)
            .ok()
            .as_deref(),
        Path::new(edel::places::DEVELOPER_RESET).exists(),
    ) {
        println!("developer mode: {state}");
    }
    // The desktop's effect tier (M5.11), while a session runs.
    if let Some(tier) = crate::shell::tier_now() {
        println!("effects: {tier}");
    }
    // Where to look when a boot or a session went wrong (M5.28a).
    for line in crate::report::pointers_now() {
        println!("{line}");
    }
    Ok(())
}

/// The os-release of the slot on `device`, read with debugfs, so the slot
/// that is not running is never mounted (M3.1).
fn slot_os_release(device: &Path) -> Option<String> {
    let out = Command::new("debugfs")
        .args(["-R", "cat /usr/lib/os-release"])
        .arg(device)
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    (out.status.success() && text.contains("VERSION_ID=")).then_some(text)
}

/// Installs the signed release whose `release.toml` is at `location`, a
/// path or an http(s) URL (or, with `unsigned`, the slot image or block
/// device at that path), into the slot that is not running, and makes it
/// start next. The image streams into the slot, decompressed on the way
/// when it is gzipped, and stays as it was signed: never grown, its UUID
/// its own (M1.12).
pub fn install(location: &str, accept: &crate::release::Accept, unsigned: bool) -> Result<()> {
    let _lock = Lock::take("update", "edel update")?;
    // A game gets the whole machine, so the update waits for it (M7.7c).
    // The wait is here, after the lock and before the disk: a second update
    // names its holder at once. App updates (`default_apps::follow`) run
    // only after this install, so they wait with it.
    crate::shell::wait_for_game("edel update");
    let disk = Disk::find()?;
    let slot = disk.running.other();
    let target = Path::new("/dev").join(&disk.install_target()?.name);
    let mut manifest = None;
    let (mut source, expected, size): (Box<dyn Read>, Option<Vec<u8>>, u64) = if unsigned {
        eprintln!(
            "warning: {}",
            tr("installing an unsigned image; nothing checked where it came from")
        );
        let path = Path::new(location);
        (Box::new(File::open(path)?), None, size_of(path)?)
    } else {
        let release = crate::release::open_checked(location, accept).inspect_err(|err| {
            // Up to date is a check that worked: the security status says so.
            if let Some(up) = err.downcast_ref::<crate::release::UpToDate>() {
                crate::release::record_check(&up.version);
            }
        })?;
        crate::release::record_check(&release.manifest.version);
        manifest = Some(release.manifest);
        (release.reader, Some(release.sha256), release.size)
    };
    let room = size_of(&target)?;
    if size > room {
        bail!("{}", too_big(slot, size, room));
    }

    // The EFI system partition is mounted only while it is written: here,
    // and again once the slot is checked. The lock keeps others out between.
    {
        let esp = Esp::mount(&disk)?;
        let mut env = esp.load()?;
        let safe =
            crate::data::is_safe_start(&fs::read_to_string("/proc/cmdline").unwrap_or_default());
        if !running_is_settled(&env, disk.running, safe) {
            bail!(
                "{}",
                trf(
                    "slot {running} has not finished starting yet, so slot {slot}, the version before it, is kept as the way back; run edel update again once Edel OS has started (edel status says when)",
                    &[
                        ("running", &disk.running.to_string()),
                        ("slot", &slot.to_string())
                    ]
                )
            );
        }
        before_install(&mut env, slot);
        esp.save(&env)?;
    }
    // The slot is off now, so no update waits for a restart until this one
    // is written; mark_restart_pending says so again once it is.
    let _ = fs::remove_file(edel::places::RESTART_PENDING);

    println!(
        "edel update: {}",
        trf(
            "writing {location} to slot {slot} ({device})",
            &[
                ("location", location),
                ("slot", &slot.to_string()),
                ("device", &target.display().to_string())
            ]
        )
    );
    let mut dst = OpenOptions::new()
        .write(true)
        .open(&target)
        .with_context(|| {
            trf(
                "could not open {path}",
                &[("path", &target.display().to_string())],
            )
        })?;
    let (written, streamed) = copy_hashing(&mut source, &mut dst)?;
    dst.sync_all()?;
    drop(dst);
    if expected.as_ref().is_some_and(|e| *e != streamed) || written != size {
        bail!("{}", wrong_checksum(slot));
    }
    // Drop cached blocks, so the check reads what is on the disk.
    run(Command::new("blockdev").arg("--flushbufs").arg(&target))?;
    if hash_prefix(&target, written)? != streamed {
        bail!("{}", not_read_back(slot));
    }

    // The slot keeps the signed bytes for its whole life (M1.12): checked
    // without changes (-n), never grown, and with the image's own UUID, as
    // GRUB finds a slot by its partition.
    let fsck = Command::new("e2fsck")
        .arg("-fn")
        .arg(&target)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context(tr("could not start e2fsck"))?;
    if fsck.code() != Some(0) {
        bail!("{}", file_system_errors(slot));
    }
    // The add-ons the machine names arrive beside the slot, checked as it
    // is, before it is switched on (M7.2a).
    if let Some(manifest) = &manifest {
        crate::addons::fetch(location, manifest, accept.allow_http, &target)?;
    }

    let esp = Esp::mount(&disk)?;
    let mut env = esp.load()?;
    after_install(&mut env, slot);
    esp.save(&env)?;
    mark_restart_pending(&slot.to_string());
    if let Some(manifest) = &manifest {
        crate::addons::remember_source(location, &manifest.version, accept);
    }
    println!(
        "edel update: {}",
        trf(
            "slot {slot} written and checked; restart to use it (edel restart); if it fails to start, Edel OS goes back to slot {running} on its own",
            &[
                ("slot", &slot.to_string()),
                ("running", &disk.running.to_string())
            ]
        )
    );
    Ok(())
}

/// Leaves `places::RESTART_PENDING` saying which slot starts next, for
/// the lock screen's power menu to mark Restart (M5.10c). `/run` is memory,
/// so it goes with the next boot. The slot is already chosen when this runs,
/// so a file that cannot be written changes nothing but the mark.
fn mark_restart_pending(slot: &str) {
    let path = Path::new(edel::places::RESTART_PENDING);
    if let Err(e) = fs::write(path, format!("{slot}\n")) {
        eprintln!(
            "warning: {}",
            trf(
                "could not write {path} ({why}), so the power menu does not mark Restart; the update still starts at the next restart",
                &[
                    ("path", &path.display().to_string()),
                    ("why", &e.to_string())
                ]
            )
        );
    }
}

/// The update image does not fit the slot it was to be written to.
fn too_big(slot: Slot, size: u64, room: u64) -> String {
    trf(
        "the update image is {size} MiB and slot {slot} holds {room} MiB, so nothing was written; the running system is unchanged",
        &[
            ("size", &size.div_ceil(MIB).to_string()),
            ("slot", &slot.to_string()),
            ("room", &(room / MIB).to_string()),
        ],
    )
}

/// The written image is not the one release.toml lists.
fn wrong_checksum(slot: Slot) -> String {
    trf(
        "refused: sha256: the image does not match the checksum in release.toml, so it is damaged or was changed on the way; slot {slot} stays switched off and the running system is unchanged; run edel update again",
        &[("slot", &slot.to_string())],
    )
}

/// The disk gave back something other than what was written.
fn not_read_back(slot: Slot) -> String {
    trf(
        "slot {slot} does not read back as written, so the disk may be failing; it stays switched off and the running system is unchanged; run edel update again",
        &[("slot", &slot.to_string())],
    )
}

/// `e2fsck` found errors in the written slot.
fn file_system_errors(slot: Slot) -> String {
    trf(
        "slot {slot} has file system errors after writing; it stays switched off and the running system is unchanged; run edel update again",
        &[("slot", &slot.to_string())],
    )
}

/// Copies `src` into `dst`, returning the bytes written and their SHA-256.
pub(crate) fn copy_hashing(src: &mut dyn Read, dst: &mut dyn Write) -> Result<(u64, Vec<u8>)> {
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut total = 0u64;
    loop {
        let n = src.read(&mut buf).context(tr("could not read the image"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        dst.write_all(&buf[..n])
            .context(tr("could not write the slot"))?;
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
    let _lock = Lock::take("update", "edel update")?;
    let disk = Disk::find()?;
    let esp = Esp::mount(&disk)?;
    let mut env = esp.load()?;
    if let Some(failed) = confirm(&mut env, disk.running) {
        let msg = trf(
            "slot {failed} did not start, so Edel OS went back to slot {running}; your files and settings are unchanged, and the automatic updates pass that release over until a newer one comes (edel update installs it again by hand)",
            &[
                ("failed", &failed.to_string()),
                ("running", &disk.running.to_string()),
            ],
        );
        lines.push(format!("edel update: {msg}"));
        let _ = Command::new("logger").args(["-t", "edel", &msg]).status();
        if let Err(err) = record_fallback(failed, disk.running) {
            eprintln!(
                "warning: {}",
                trf(
                    "the fallback happened but could not be recorded ({why}), so Settings will not mention it",
                    &[("why", &format!("{err:#}"))]
                )
            );
        }
    }
    esp.save(&env)?;
    lines.push(format!("edel update: slot {} confirmed", disk.running));
    // The add-ons this boot tried for the first time stay (M7.2b).
    lines.extend(crate::addons::promote());
    // Only a confirmed slot may change the boot loader (M1.8).
    match crate::boot::efi_target(std::env::consts::ARCH).and_then(|(_, efi)| {
        crate::loader::update_esp(&esp.dir, Path::new(crate::loader::SLOT_DIR), efi)
    }) {
        Ok(Some(line)) => lines.push(line),
        Ok(None) => {}
        Err(err) => lines.push(format!(
            "warning: {}",
            trf(
                "the boot loader was not updated ({why}); the slot is confirmed and the boot loader before it stays",
                &[("why", &format!("{err:#}"))]
            )
        )),
    }
    Ok(lines)
}

/// The fallback record shell-ui shows once (M5.9).
fn fallback_record(from: Slot, to: Slot, date: &str) -> String {
    format!("format = 1\nfrom = \"{from}\"\nto = \"{to}\"\ndate = {date}\n")
}

/// The record of add-ons whose first boot did not finish (M7.2b), in the
/// same file, which shell-ui shows once as an add-on's notice.
fn addons_record(names: &[String], date: &str) -> String {
    let list: Vec<String> = names.iter().map(|n| format!("{n:?}")).collect();
    format!(
        "format = 1\naddons = [{}]\ndate = {date}\n",
        list.join(", ")
    )
}

/// Writes `/data/edel/last-fallback.toml`, when the data partition is
/// mounted (M1.3).
fn record_fallback(from: Slot, to: Slot) -> Result<()> {
    write_record(|date| fallback_record(from, to, date))
}

/// Records that the add-ons `names` did not start (M7.2b).
pub(crate) fn record_failed_addons(names: &[String]) -> Result<()> {
    write_record(|date| addons_record(names, date))
}

/// Writes the record `text` gives for the time now, when the data
/// partition is mounted.
fn write_record(text: impl Fn(&str) -> String) -> Result<()> {
    if !crate::data::is_mounted(edel::places::DATA_MOUNT)? {
        return Ok(());
    }
    let date = Command::new("date")
        .arg("-u")
        .arg("+%Y-%m-%dT%H:%M:%SZ")
        .output()
        .context("could not start date")?;
    let date = String::from_utf8_lossy(&date.stdout).trim().to_string();
    fs::create_dir_all(edel::places::DATA_DIR)?;
    fs::write(edel::places::LAST_FALLBACK, text(&date))?;
    Ok(())
}

/// Makes the other slot start at the next boot.
pub fn rollback() -> Result<()> {
    let _lock = Lock::take("update", "edel rollback")?;
    let disk = Disk::find()?;
    let esp = Esp::mount(&disk)?;
    let mut env = esp.load()?;
    let target = roll_back(&mut env, disk.running)?;
    esp.save(&env)?;
    mark_restart_pending(&target.to_string());
    println!(
        "edel rollback: {}",
        trf(
            "slot {slot} starts after the next restart (edel restart); slot {running} stays installed, so edel rollback again undoes this",
            &[
                ("slot", &target.to_string()),
                ("running", &disk.running.to_string())
            ]
        )
    );
    Ok(())
}

/// What `updates.automatic` asks for (M7.5); absent, or a value this
/// release does not know, is off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Automatic {
    Off,
    Check,
    Install,
    InstallAndRestart,
}

impl Automatic {
    fn from_value(value: Option<&str>) -> Automatic {
        match value {
            Some("check") => Automatic::Check,
            Some("install") => Automatic::Install,
            Some("install-and-restart") => Automatic::InstallAndRestart,
            _ => Automatic::Off,
        }
    }
}

/// What an automatic run does about restarting once an update waits in
/// the other slot: a machine with a screen only says so, as a person may
/// be at it; one without restarts inside `updates.restart_window`, or at
/// any hour without one.
#[derive(Debug, PartialEq, Eq)]
enum Restart {
    Now,
    Wait,
    Screen,
}

fn restart_now(screen: bool, window: Option<(u16, u16)>, minute: u16) -> Restart {
    if screen {
        return Restart::Screen;
    }
    match window {
        Some(span) if !edel::settings::in_hours(span, minute) => Restart::Wait,
        _ => Restart::Now,
    }
}

/// `edel update --automatic` (M7.5), which the `updates` feature's timer
/// runs once an hour: as `updates.automatic` says, nothing, a check, an
/// install from `location`, or an install and then a restart. Each thing
/// it does is one line on its output and in the system log.
pub fn automatic(location: Option<String>, accept: &crate::release::Accept) -> Result<()> {
    crate::default_apps::follow_languages();
    let chosen = edel::settings::on_machine;
    let mode = Automatic::from_value(chosen("updates.automatic").as_deref());
    if mode == Automatic::Off {
        return Ok(());
    }
    let pending = Path::new(edel::places::RESTART_PENDING).exists();
    if !pending {
        let location = match location {
            Some(location) => location,
            None => crate::release::default_location(accept)?,
        };
        let manifest = crate::release::verified(&location, accept)?;
        let os_release = fs::read_to_string("/usr/lib/os-release").unwrap_or_default();
        let running = os_release_value(&os_release, "VERSION_ID").unwrap_or_default();
        if crate::release::compare_versions(&manifest.version, &running)
            != std::cmp::Ordering::Greater
        {
            return Ok(());
        }
        if mode == Automatic::Check {
            say(&trf(
                "{version} is available; edel update installs it",
                &[("version", &manifest.version)],
            ));
            return Ok(());
        }
        // An update that fell back stays in the other slot, switched off;
        // installing it again every hour would fall back every time.
        if fell_back(&manifest.version)? {
            say(&trf(
                "{version} did not start here before, so it is not installed again; a newer release will be",
                &[("version", &manifest.version)],
            ));
            return Ok(());
        }
        install(&location, accept, false)?;
        if let Err(err) = crate::default_apps::follow() {
            eprintln!("warning: {err:#}");
        }
    }
    if mode != Automatic::InstallAndRestart {
        return Ok(());
    }
    let window = chosen("updates.restart_window").and_then(|w| edel::settings::hours(&w));
    match restart_now(edel::services::screen_now(), window, local_minute()?) {
        Restart::Now => {
            say(tr("restarting to start the update"));
            crate::system_power::run(crate::system_power::Action::Restart)
        }
        Restart::Wait => Ok(()),
        Restart::Screen => {
            say(tr("the update starts at the next restart"));
            Ok(())
        }
    }
}

/// Whether the other slot holds `version` and did not start, so the guard
/// went back from it.
fn fell_back(version: &str) -> Result<bool> {
    let disk = Disk::find()?;
    let env = Esp::mount(&disk)?.load()?;
    let other = disk.running.other();
    if !failed_to_start(&env, other) {
        return Ok(false);
    }
    let device = disk.device(other.partition())?;
    Ok(slot_os_release(&device)
        .and_then(|text| os_release_value(&text, "VERSION_ID"))
        .is_some_and(|v| v == version))
}

/// Whether `slot` failed to start: switched off after GRUB counted a try
/// on it, as `confirm` leaves a slot it passed over. A slot switched off
/// with no try is an install that never finished, worth trying again.
fn failed_to_start(env: &Env, slot: Slot) -> bool {
    !env.ok(slot) && env.tries(slot) > 0
}

/// The local time now, in minutes after midnight.
fn local_minute() -> Result<u16> {
    let out = Command::new("date")
        .arg("+%H:%M")
        .output()
        .context("could not start date")?;
    let now = String::from_utf8_lossy(&out.stdout).trim().to_string();
    now.split_once(':')
        .and_then(|(h, m)| Some(h.parse::<u16>().ok()? * 60 + m.parse::<u16>().ok()?))
        .with_context(|| format!("date printed {now:?}"))
}

/// One line on the output and in the system log.
fn say(line: &str) {
    println!("edel update: {line}");
    let _ = Command::new("logger")
        .args(["-t", "edel", &format!("edel update: {line}")])
        .status();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_automatic_restart_waits_for_its_window_and_never_comes_with_a_screen() {
        assert_eq!(Automatic::from_value(None), Automatic::Off);
        assert_eq!(Automatic::from_value(Some("later")), Automatic::Off);
        assert_eq!(
            Automatic::from_value(Some("install-and-restart")),
            Automatic::InstallAndRestart
        );
        let window = Some((120, 240));
        assert_eq!(restart_now(false, None, 900), Restart::Now);
        assert_eq!(restart_now(false, window, 150), Restart::Now);
        assert_eq!(restart_now(false, window, 900), Restart::Wait);
        assert_eq!(restart_now(true, None, 150), Restart::Screen);
    }

    #[test]
    fn an_add_on_that_did_not_start_is_recorded_with_a_date() {
        let text = addons_record(&["ci-hang".to_string()], "2026-10-17T10:00:00Z");
        let table: toml::Table = text.parse().unwrap();
        assert_eq!(table["format"].as_integer(), Some(1));
        assert_eq!(
            table["addons"].as_array().unwrap()[0].as_str(),
            Some("ci-hang")
        );
        assert!(table.contains_key("date") && !table.contains_key("from"));
    }

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
        assert!(err.to_string().contains("no slots to update or roll back"));
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
    fn a_slot_that_fell_back_failed_and_a_cut_install_did_not() {
        // After a fallback, confirm keeps the failed slot's tries: it is not
        // tried again. An install cut off before after_install leaves it off
        // with no try: the next automatic update writes it again.
        let mut env = Env::parse("ORDER=B A\nA_OK=1\nA_TRY=1\nB_OK=1\nB_TRY=3\n").unwrap();
        confirm(&mut env, Slot::A);
        assert!(failed_to_start(&env, Slot::B));
        let mut env = Env::initial();
        before_install(&mut env, Slot::B);
        assert!(!failed_to_start(&env, Slot::B));
        after_install(&mut env, Slot::B);
        assert!(!failed_to_start(&env, Slot::B));
    }

    #[test]
    fn an_unconfirmed_slot_keeps_the_version_before() {
        // B was installed and is on its first try; A last worked.
        let env = Env::parse("ORDER=B A\nA_OK=1\nA_TRY=0\nB_OK=1\nB_TRY=1\n").unwrap();
        assert!(!running_is_settled(&env, Slot::B, false));
        // Once the guard confirmed B, A may be written.
        let env = Env::parse("ORDER=B A\nA_OK=1\nA_TRY=0\nB_OK=1\nB_TRY=0\n").unwrap();
        assert!(running_is_settled(&env, Slot::B, false));
        // A safe start has no better way back to keep.
        let env = Env::parse("ORDER=B A\nA_OK=1\nA_TRY=3\nB_OK=1\nB_TRY=3\n").unwrap();
        assert!(running_is_settled(&env, Slot::B, true));
    }

    #[test]
    fn confirming_keeps_a_pending_install() {
        let mut env = Env::initial();
        after_install(&mut env, Slot::B);
        assert_eq!(confirm(&mut env, Slot::A), None);
        assert_eq!(env.order(), [Slot::B, Slot::A]);
        assert!(env.ok(Slot::A) && env.ok(Slot::B));
        // The same holds for a rollback asked for during this boot.
        let mut env = Env::parse("ORDER=A B\nA_OK=1\nA_TRY=0\nB_OK=1\nB_TRY=0\n").unwrap();
        roll_back(&mut env, Slot::A).unwrap();
        assert_eq!(confirm(&mut env, Slot::A), None);
        assert_eq!(env.order(), [Slot::B, Slot::A]);
    }

    #[test]
    fn an_unfinished_install_is_not_a_fallback() {
        let mut env = Env::parse("ORDER=B A\nA_OK=1\nA_TRY=1\nB_OK=0\nB_TRY=0\n").unwrap();
        assert_eq!(confirm(&mut env, Slot::A), None);
        assert_eq!(env.order(), [Slot::A, Slot::B]);
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
        assert_eq!(
            err.to_string(),
            "slot B is switched off, so there is nothing to roll back to: it holds no update, or the update failed to start; the running system is unchanged"
        );
        assert_eq!(env, before);
    }

    /// What `edel update` says when it stops before or after writing a
    /// slot (docs/MESSAGES.md): what failed, why, and what is safe.
    #[test]
    fn an_update_that_stops_says_what_is_safe_and_what_to_do() {
        assert_eq!(
            too_big(Slot::B, 5000 * MIB + 1, 4096 * MIB),
            "the update image is 5001 MiB and slot B holds 4096 MiB, so nothing was written; the running system is unchanged"
        );
        let wrong = wrong_checksum(Slot::B);
        assert!(wrong.starts_with("refused: sha256: "), "{wrong}");
        assert!(wrong.ends_with(
            "slot B stays switched off and the running system is unchanged; run edel update again"
        ));
        for text in [not_read_back(Slot::A), file_system_errors(Slot::A)] {
            assert!(text.contains("slot A "), "{text}");
            assert!(text.ends_with("it stays switched off and the running system is unchanged; run edel update again"), "{text}");
        }
    }

    /// A first try as an ordinary user reads that it needs root, not
    /// `Permission denied (os error 13)`.
    #[test]
    fn a_refused_write_says_to_run_it_with_doas() {
        assert_eq!(
            refused("could not write /run/edel", "edel update --check"),
            "could not write /run/edel: permission denied; edel update --check changes the system, so it needs an administrator: doas edel update --check"
        );
        let denied = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        let text = io_failure(denied, "could not write /run/edel").to_string();
        assert!(text.contains("needs an administrator: doas edel"), "{text}");
        let full = std::io::Error::from(std::io::ErrorKind::StorageFull);
        let text = format!("{:#}", io_failure(full, "could not write /run/edel"));
        assert!(text.starts_with("could not write /run/edel: "), "{text}");
        assert!(!text.contains("permission denied"), "{text}");
    }

    #[test]
    fn a_held_lock_says_who_holds_it() {
        assert_eq!(
            holder("812 edel update\n"),
            "edel update (pid 812) is running"
        );
        assert_eq!(holder(""), "another command is starting");
    }

    #[test]
    fn one_command_at_a_time_holds_a_lock_even_after_a_crash() {
        let dir = std::env::temp_dir().join(format!("edel-lock-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("update.lock");
        // A killed updater leaves its words but not the kernel's lock.
        fs::write(&path, "999999999 edel update\n").unwrap();
        assert!(!is_held(&path));
        let first = Lock::take_at(&path, "edel update").unwrap();
        assert!(is_held(&path));
        // Two takers at once, as the timer's update and the person's: only
        // one may hold it, and the other is told who does.
        let refused = std::thread::scope(|s| {
            let a = s.spawn(|| Lock::take_at(&path, "edel rollback").map(|_| ()));
            let b = s.spawn(|| Lock::take_at(&path, "edel update --automatic").map(|_| ()));
            [a.join().unwrap(), b.join().unwrap()]
        });
        for err in refused {
            let text = err.unwrap_err().to_string();
            assert!(
                text.contains(&format!(
                    "edel update (pid {}) is running",
                    std::process::id()
                )),
                "{text}"
            );
        }
        drop(first);
        assert!(!is_held(&path));
        let second = Lock::take_at(&path, "edel rollback").unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            format!("{} edel rollback\n", std::process::id())
        );
        drop(second);
        fs::remove_dir_all(&dir).unwrap();
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
