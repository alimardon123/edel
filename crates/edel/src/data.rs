//! `edel boot mount-data`: the data partition, partition 4, which holds
//! what outlives every update (roadmap M1.2). The `edel-data` service runs
//! it in sysinit, before the boot runlevel writes to `/var`. On the first
//! boot of a disk larger than the image, the partition grows to the end of
//! the disk. Then `/etc` gets an overlay whose upper layer is on it
//! (M1.4), `/home` and `/var` are bound onto it (M1.3) and `/tmp` becomes a
//! tmpfs, so the slot itself stays read-only while updates and rollbacks
//! keep people's files, settings, logs and service state.

use std::fs;
use std::io::Write;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::Duration;

use anyhow::{Context, Result};

use edel::{addon_state, base, hardware, places, services};

use crate::boot::DATA_PARTITION;
use crate::update::{Disk, run};

const MOUNT_POINT: &str = edel::places::DATA_MOUNT;
/// Leave the partition alone when less than this lies after it, in
/// 512-byte sectors (2 MiB): sfdisk aligns to 1 MiB, and GPT keeps its
/// backup copy in the last sectors of the disk.
const GROW_SLACK_SECTORS: u64 = 4096;

/// Whether the partition at `start` with `size` sectors should grow on a
/// disk of `disk_sectors` sectors.
fn should_grow(disk_sectors: u64, start: u64, size: u64) -> bool {
    disk_sectors.saturating_sub(start + size) > GROW_SLACK_SECTORS
}

fn sectors(path: &Path) -> Result<u64> {
    let text =
        fs::read_to_string(path).with_context(|| format!("could not read {}", path.display()))?;
    Ok(text.trim().parse()?)
}

pub(crate) fn is_mounted(dir: &str) -> Result<bool> {
    let mountinfo = fs::read_to_string("/proc/self/mountinfo")?;
    Ok(mountinfo
        .lines()
        .any(|l| l.split_whitespace().nth(4) == Some(dir)))
}

/// Moves GPT's backup copy to the new end of the disk, then lets the data
/// partition take all the space after it. The root is on this disk, so
/// sfdisk must not reread it; partx tells the kernel about the one
/// partition that changed.
fn grow(disk: &Path) -> Result<()> {
    run(Command::new("sfdisk")
        .args(["--no-reread", "--relocate", "gpt-bak-std"])
        .arg(disk))?;
    let mut sfdisk = Command::new("sfdisk")
        .args(["--no-reread", "-N", &DATA_PARTITION.to_string()])
        .arg(disk)
        .stdin(Stdio::piped())
        .spawn()
        .context("could not start sfdisk")?;
    sfdisk
        .stdin
        .take()
        .context("no stdin")?
        .write_all(b", +\n")?;
    if !sfdisk.wait()?.success() {
        anyhow::bail!("sfdisk could not grow partition {DATA_PARTITION}");
    }
    run(Command::new("partx")
        .args(["--update", "--nr", &DATA_PARTITION.to_string()])
        .arg(disk))
}

fn names(dir: &Path) -> Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("could not read {}", dir.display()))? {
        names.push(entry?.file_name().to_string_lossy().into_owned());
    }
    names.sort();
    Ok(names)
}

fn present(path: &Path) -> bool {
    path.symlink_metadata().is_ok()
}

fn is_real_dir(path: &Path) -> bool {
    path.symlink_metadata().is_ok_and(|m| m.is_dir())
}

/// Entries of the slot's `/var` that the data partition lacks, as paths
/// relative to `/var`: top-level names, and names inside `lib` and `log`,
/// where packages add their own directories. Copying them before the bind
/// means an update that adds a directory under `/var` boots with it.
fn missing_var_entries(slot: &Path, data: &Path) -> Result<Vec<PathBuf>> {
    let mut missing = Vec::new();
    for name in names(slot)? {
        if !present(&data.join(&name)) {
            missing.push(PathBuf::from(&name));
        } else if (name == "lib" || name == "log") && is_real_dir(&slot.join(&name)) {
            for child in names(&slot.join(&name))? {
                if !present(&data.join(&name).join(&child)) {
                    missing.push(Path::new(&name).join(child));
                }
            }
        }
    }
    Ok(missing)
}

/// Lines of the slot's account file (`slot`) whose name, the first field,
/// is missing from the machine's copy (`machine`): users and groups a
/// newer slot adds, which the machine's own copy would otherwise hide.
/// Returns the machine's file with them appended, or `None` when nothing
/// is missing.
fn merge_accounts(slot: &str, machine: &str) -> Option<String> {
    let name = |line: &str| line.split(':').next().unwrap_or_default().to_string();
    // passwd (7 fields) and group (4) give the id in field 3; in shadow
    // (9) that field is a date.
    let id = |line: &str| {
        let fields: Vec<&str> = line.split(':').collect();
        matches!(fields.len(), 4 | 7)
            .then(|| fields[2])
            .filter(|f| !f.is_empty() && f.bytes().all(|b| b.is_ascii_digit()))
            .map(String::from)
    };
    let known: std::collections::HashSet<String> = machine.lines().map(name).collect();
    let taken: std::collections::HashSet<String> = machine.lines().filter_map(id).collect();
    let missing: Vec<&str> = slot
        .lines()
        .filter(|l| !l.is_empty() && !known.contains(&name(l)))
        .filter(|l| {
            // Builds pin every system account's id (account_ids.rs), so
            // a slot's new account has an id no build gave another; a
            // machine made before the pins, or an account made by hand,
            // may still hold it, and adding the line would make two
            // names share one id.
            let clash = id(l).is_some_and(|i| taken.contains(&i));
            if clash {
                eprintln!(
                    "warning: edel-data: not adding {:?}: its id is already used here",
                    name(l)
                );
            }
            !clash
        })
        .collect();
    if missing.is_empty() {
        return None;
    }
    let mut merged = machine.to_string();
    if !merged.is_empty() && !merged.ends_with('\n') {
        merged.push('\n');
    }
    for line in missing {
        merged.push_str(line);
        merged.push('\n');
    }
    Some(merged)
}

/// Replaces an account file through a new file and a rename in the same
/// directory, keeping its mode and owner (`shadow` stays 0640
/// root:shadow), so a power cut leaves the old or the new file.
fn replace_keeping_mode(path: &Path, text: &str) -> Result<()> {
    let meta = fs::metadata(path)?;
    let new = PathBuf::from(format!("{}.edel-new", path.display()));
    fs::write(&new, text)?;
    fs::set_permissions(&new, meta.permissions())?;
    std::os::unix::fs::chown(&new, Some(meta.uid()), Some(meta.gid()))?;
    fs::File::open(&new)?.sync_all()?;
    fs::rename(&new, path)?;
    if let Some(dir) = path.parent() {
        fs::File::open(dir)?.sync_all()?;
    }
    Ok(())
}

/// Mounts an overlay on `/etc`: the slot's `/etc` below, this machine's
/// changes above on `/data`, so the upper directory is exactly what this
/// machine changed (stateless /etc, ADR-006). Before that, adds the system
/// users and groups the slot has but the machine's copies lack.
fn overlay_etc() -> Result<()> {
    if is_mounted("/etc")? {
        return Ok(());
    }
    let data = Path::new(MOUNT_POINT);
    let (upper, work) = (data.join("etc/upper"), data.join("etc/work"));
    fs::create_dir_all(&upper)?;
    fs::create_dir_all(&work)?;
    for file in crate::addons::ACCOUNT_FILES {
        let file = file.strip_prefix("etc/").unwrap_or(file);
        let machine_path = upper.join(file);
        // The mounted add-ons' users and groups (M7.2a) join the slot's;
        // a machine without its own copy gets one, from the slot's.
        let addons = crate::addons::account_lines(file);
        // And the members they add to the slot's groups (M7.2b).
        let members = if file == "group" {
            crate::addons::member_lines()
        } else {
            String::new()
        };
        if (!addons.is_empty() || !members.is_empty()) && !machine_path.exists() {
            run(Command::new("cp")
                .arg("-a")
                .arg(Path::new("/etc").join(file))
                .arg(&machine_path))?;
        }
        let Ok(machine) = fs::read_to_string(&machine_path) else {
            continue;
        };
        let slot = fs::read_to_string(Path::new("/etc").join(file)).unwrap_or_default() + &addons;
        let merged = merge_accounts(&slot, &machine);
        let text = merged.as_deref().unwrap_or(&machine);
        if let Some(joined) = crate::addons::add_members(text, &members) {
            replace_keeping_mode(&machine_path, &joined)?;
            println!("edel-data: added the slot's and the add-ons' new entries to /etc/{file}");
        } else if let Some(merged) = merged {
            replace_keeping_mode(&machine_path, &merged)?;
            println!("edel-data: added the slot's new entries to /etc/{file}");
        }
    }
    let _ = Command::new("modprobe").arg("overlay").status();
    run(Command::new("mount")
        .args(["-t", "overlay", "overlay", "-o"])
        .arg(format!(
            "lowerdir=/etc,upperdir={},workdir={}",
            upper.display(),
            work.display()
        ))
        .arg("/etc"))
    .context("cannot mount the /etc overlay")?;
    println!("edel-data: /etc changes are kept on {MOUNT_POINT}");
    // OpenRC read the services before this layer was there, so a service
    // whose script the layer links in, an add-on's (M7.2a), or one added
    // in developer mode's layer (M7.1, which mounts before this), would
    // never start without this. It costs a moment, only at a boot with
    // such an add-on or with developer mode on.
    let developer_on =
        fs::read_to_string(places::DEVELOPER_NOW).is_ok_and(|now| now.trim() == "on");
    if crate::addons::has_services() || developer_on {
        if let Err(err) = run(&mut base::reread_services()) {
            eprintln!(
                "warning: edel-data: OpenRC could not read the services of the add-ons and developer mode again, so they may not start: {err:#}"
            );
        }
    }
    Ok(())
}

/// Developer mode's layer over `/usr` (ADR-007 level 3, M7.1): this
/// machine's changes above the slot's `/usr`, kept on `/data`, so OS files
/// can be edited and `apk add` works (the images are `/usr`-merged). A
/// reset asked for is done first, while nothing is mounted over the layer
/// or `/etc`. A start with no slot left with a try (`boot::SAFE_ARG` on
/// the command line) holds the layer back, and so does every start after
/// it until developer mode is reset or switched off, so a change that
/// stops the machine starting is never mounted again by itself. Seam
/// (ADR-007, M7.2a): add-ons become more lower directories of this one
/// mount, held back the same way.
fn developer_layer() -> Result<()> {
    let (layer, work) = (
        Path::new(places::DEVELOPER_LAYER),
        Path::new(places::DEVELOPER_WORK),
    );
    let held = Path::new(places::DEVELOPER_HELD);
    let reset = Path::new(places::DEVELOPER_RESET);
    if reset.exists() {
        for dir in [layer, work] {
            if dir.exists() {
                fs::remove_dir_all(dir)
                    .with_context(|| format!("cannot delete {}", dir.display()))?;
            }
        }
        // apk's list of wanted packages, so it is the slot's again.
        let world = Path::new(places::ETC_UPPER).join("apk/world");
        for mark in [held, &world, reset] {
            if mark.exists() {
                fs::remove_file(mark)?;
            }
        }
        println!("edel-data: deleted developer mode's changes under /usr, as reset asked");
    }
    if !Path::new(places::DEVELOPER_FLAG).exists() {
        // Switching off ends a hold.
        if held.exists() {
            fs::remove_file(held)?;
        }
        return Ok(());
    }
    let now = Path::new(places::DEVELOPER_NOW);
    fs::create_dir_all(places::RUN_DIR)?;
    let safe = is_safe_start(&fs::read_to_string("/proc/cmdline").unwrap_or_default());
    if safe || held.exists() {
        fs::create_dir_all(held.parent().unwrap_or(Path::new(MOUNT_POINT)))?;
        fs::write(held, "")?;
        fs::write(now, "held\n")?;
        let why = if safe {
            "this start is a safe one, as no slot had a try left"
        } else {
            "a start found no slot with a try left"
        };
        println!(
            "edel-data: developer mode's layer over /usr is held back, as {why}; switch developer mode off and on again, or reset it, to end this"
        );
        return Ok(());
    }
    // Mounted once a start: `/run` is new each start, and an add-ons'
    // overlay may already be on /usr below the layer.
    if fs::read_to_string(now).unwrap_or_default().trim() != "on" {
        fs::create_dir_all(layer)?;
        fs::create_dir_all(work)?;
        let _ = Command::new("modprobe").arg("overlay").status();
        run(Command::new("mount")
            .args(["-t", "overlay", "overlay", "-o"])
            .arg(format!(
                "lowerdir=/usr,upperdir={},workdir={}",
                layer.display(),
                work.display()
            ))
            .arg("/usr"))
        .context("cannot mount developer mode's layer over /usr")?;
    }
    fs::write(now, "on\n")?;
    println!(
        "edel-data: developer mode is on: /usr is writable, its changes kept on {MOUNT_POINT}"
    );
    Ok(())
}

/// Whether the kernel's command line marks a safe start (M7.1).
fn is_safe_start(cmdline: &str) -> bool {
    cmdline
        .split_whitespace()
        .any(|word| word == crate::boot::SAFE_ARG)
}

/// What `edel status` says of developer mode, from whether the flag is
/// there now and what this boot's `places::DEVELOPER_NOW` says; nothing
/// while it is off and stays off (M7.1).
pub fn developer_state(flag: bool, now: Option<&str>, reset: bool) -> Option<&'static str> {
    if reset {
        return Some("its changes under /usr are deleted when the computer next starts");
    }
    match (flag, now.map(str::trim)) {
        (true, Some("on")) => Some("on"),
        (true, Some("held")) => Some(
            "on, but its layer over /usr is held back since a start found no slot with a try left; switch it off and on again to keep its changes, or reset it with edel settings reset system.developer_mode",
        ),
        (true, _) => Some("on after a restart"),
        (false, Some(_)) => Some("off after a restart"),
        (false, None) => None,
    }
}

/// `/tmp` in memory: the root is read-only, and Alpine does not do this by
/// default.
fn tmp_on_tmpfs() -> Result<()> {
    if is_mounted("/tmp")? {
        return Ok(());
    }
    run(Command::new("mount").args([
        "-t",
        "tmpfs",
        "-o",
        "mode=1777,nosuid,nodev",
        "tmpfs",
        "/tmp",
    ]))
}

/// Tops up `/data/var` from the slot, then binds `/data/home` over `/home`
/// and `/data/var` over `/var`.
fn bind_home_and_var() -> Result<()> {
    let data = Path::new(MOUNT_POINT);
    // root's home is on the read-only slot too; its first copy on /data
    // starts from the slot's, private to root.
    if !data.join("root").exists() {
        fs::create_dir_all(data.join("root"))?;
        fs::set_permissions(data.join("root"), fs::Permissions::from_mode(0o700))?;
        run(Command::new("cp")
            .args(["-a", "/root/."])
            .arg(data.join("root")))?;
    }
    for dir in ["home", "var", "edel"] {
        fs::create_dir_all(data.join(dir))?;
    }
    if !is_mounted("/var")? {
        // The slot's /var, then what the mounted add-ons keep there
        // (M7.2a), wherever this machine lacks it; nothing of the
        // machine's is replaced. Entry by entry, as busybox's `cp -n`
        // skips a folder that exists without looking inside it.
        let sources = std::iter::once(PathBuf::from("/var")).chain(crate::addons::var_dirs());
        for source in sources {
            for rel in missing_var_entries(&source, &data.join("var"))? {
                let dest = data.join("var").join(&rel);
                run(Command::new("cp")
                    .arg("-a")
                    .arg(source.join(&rel))
                    .arg(dest.parent().unwrap_or(data)))?;
            }
        }
        // Flatpak hands apps the first fonts' cache folder that exists, and
        // this one would be stale after an update, so the slot's cache
        // (M6.2a) replaces it. It is only a cache: a failure is said, not
        // fatal.
        let old_cache = data
            .join("var")
            .join(edel::places::VAR_FONT_CACHE.trim_start_matches("/var/"));
        if Path::new(edel::places::FONT_CACHE_DIR).is_dir() && old_cache.is_dir() {
            match fs::remove_dir_all(&old_cache) {
                Ok(()) => println!(
                    "edel-data: took the old fonts' cache out of {}, as the slot carries its own",
                    edel::places::VAR_FONT_CACHE
                ),
                Err(err) => eprintln!(
                    "warning: edel-data: could not take the old fonts' cache out of {}: {err}",
                    edel::places::VAR_FONT_CACHE
                ),
            }
        }
    }
    for dir in ["/home", "/var", "/root"] {
        if !is_mounted(dir)? {
            let from = data.join(dir.trim_start_matches('/'));
            run(Command::new("mount").arg("--bind").arg(&from).arg(dir))?;
        }
    }
    println!("edel-data: /home, /var and /root are on {MOUNT_POINT}");
    Ok(())
}

/// Makes sure the node `dev` exists. Right after partx resized partition 4
/// its `/dev` node has been missing for seconds (CI, 2026-10-02: resize2fs
/// said "No such file or directory while opening /dev/vda4"), so this
/// waits a second, asks mdev to scan, waits two more and then makes the
/// node itself from the numbers sysfs gives, readable by root only.
fn ensure_node(dev: &Path) {
    for tick in 0..30 {
        if dev.exists() {
            return;
        }
        if tick == 10 {
            let _ = base::scan_devices().status();
        }
        sleep(Duration::from_millis(100));
    }
    let numbers = dev.file_name().and_then(|name| {
        fs::read_to_string(Path::new("/sys/class/block").join(name).join("dev")).ok()
    });
    if let Some((major, minor)) = numbers.as_deref().and_then(|n| n.trim().split_once(':')) {
        eprintln!("edel-data: {} was missing; making it", dev.display());
        let _ = Command::new("mknod")
            .args(["-m", "600"])
            .arg(dev)
            .args(["b", major, minor])
            .status();
    }
}

/// Grows the mounted file system on `dev` to fill its partition, online; it
/// does nothing when the file system already fills it, and every boot runs
/// it, so a growth that failed is finished on the next boot. It tries three
/// times, a second apart, each time after `ensure_node`, and prints
/// resize2fs's own error if all fail.
fn grow_file_system(dev: &Path) {
    let mut why = String::new();
    for attempt in 1..=3 {
        ensure_node(dev);
        match Command::new("resize2fs").arg(dev).output() {
            Ok(out) if out.status.success() => return,
            Ok(out) => {
                why = String::from_utf8_lossy(&out.stderr)
                    .trim()
                    .replace('\n', "; ")
            }
            Err(err) => why = err.to_string(),
        }
        if attempt < 3 {
            sleep(Duration::from_secs(1));
        }
    }
    eprintln!("warning: could not grow the data file system: {why}");
}

/// The hardware packs (M7.11, ADR-012), after the add-ons are mounted: when
/// a mounted pack holds a device's firmware (`probe`), each device it
/// serves that has no driver is started again, since its driver may have
/// asked for the firmware before the pack came; then the packs this
/// machine's devices match and it has not chosen are written to
/// `places::PACKS_FILE` for `edel status`, Settings and shell-ui, as an
/// empty file when there are none. Never fails the boot.
fn hardware_packs(probe: bool) {
    let mounted = addon_state::mounted();
    if probe {
        let patterns: Vec<String> = services::installed()
            .iter()
            .filter(|(name, feature)| feature.addon && mounted.contains(name))
            .flat_map(|(_, feature)| feature.hardware.clone())
            .collect();
        for line in hardware::start_again_at(Path::new(hardware::BUS), &patterns) {
            println!("edel-data: {line}");
        }
    }
    let devices = hardware::devices();
    let offered = addon_state::offered_features_in(Path::new(places::OFFERED_ADDONS_DIR));
    let facts = addon_state::Facts::read();
    let packs =
        addon_state::unchosen_packs(&offered, &devices, |name| addon_state::state(name, &facts));
    let written = fs::create_dir_all(places::RUN_DIR)
        .and_then(|()| fs::write(places::PACKS_FILE, hardware::packs_text(&packs)));
    if let Err(err) = written {
        eprintln!(
            "warning: edel-data: could not write {}: {err}",
            places::PACKS_FILE
        );
    }
}

/// Grows the data partition if the disk has room, mounts it at `/data` and
/// grows its file system to fill it.
pub fn mount_data() -> Result<()> {
    let disk = Disk::find()?;
    let dev = disk
        .device(DATA_PARTITION)
        .context("this disk has no data partition; it predates M1.2")?;
    let part = dev.file_name().unwrap_or_default();
    let sys = Path::new("/sys/class/block");
    let disk_sectors = sectors(&sys.join(&disk.name).join("size"))?;
    let start = sectors(&sys.join(part).join("start"))?;
    let size = sectors(&sys.join(part).join("size"))?;
    if should_grow(disk_sectors, start, size) {
        println!("edel-data: growing the data partition to the end of the disk");
        // A data partition that cannot grow is still worth mounting.
        if let Err(err) = grow(&Path::new("/dev").join(&disk.name)) {
            eprintln!("warning: could not grow the data partition: {err:#}");
        }
    }
    if !is_mounted(MOUNT_POINT)? {
        ensure_node(&dev);
        run(Command::new("mount")
            .args(["-t", "ext4", "-o", "noatime"])
            .arg(&dev)
            .arg(MOUNT_POINT))
        .with_context(|| format!("cannot mount {} at {MOUNT_POINT}", dev.display()))?;
    }
    grow_file_system(&dev);
    let _ = Command::new("df").args(["-m", MOUNT_POINT]).status();
    println!("edel-data: mounted {MOUNT_POINT}");
    // Each step runs even when one before it failed: a failed /etc overlay
    // must not also leave /home, /var and /tmp off /data.
    let mut first_error = None;
    // A safe start (M7.1) leaves the add-ons off as it does developer
    // mode's layer: they live on /data, which both slots share.
    let safe = is_safe_start(&fs::read_to_string("/proc/cmdline").unwrap_or_default());
    let addons = if safe {
        println!(
            "edel-data: add-ons are left off, as this start is a safe one: no slot had a try left"
        );
        Ok(())
    } else {
        crate::addons::mount_all()
    };
    for (step, result) in [
        ("add-ons", addons),
        // After the add-ons, so the layer goes over their overlay too.
        ("developer mode", developer_layer()),
        ("/etc", overlay_etc()),
        ("/home, /var and /root", bind_home_and_var()),
        ("/tmp", tmp_on_tmpfs()),
    ] {
        if let Err(err) = result {
            eprintln!("edel-data: {step}: {err:#}");
            first_error.get_or_insert(err);
        }
    }
    // After /etc, so the rules' groups are known (M8.17c).
    crate::addons::take_device_rules();
    hardware_packs(!safe);
    // This device's identity key (M6.12): made once, at the first boot; a
    // failure is said and never stops the boot.
    match crate::device_key::ensure(
        Path::new(edel::places::DEVICE_KEY),
        Path::new(edel::places::DEVICE_KEY_PUB),
    ) {
        Ok(true) => println!(
            "edel-data: made this device's identity key, {}",
            edel::places::DEVICE_KEY_PUB
        ),
        Ok(false) => {}
        Err(err) => eprintln!("warning: could not make this device's identity key: {err:#}"),
    }
    first_error.map_or(Ok(()), Err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tops_up_only_what_the_data_partition_lacks() {
        let dir = std::env::temp_dir().join(format!("edel-var-top-up-{}", std::process::id()));
        let (slot, data) = (dir.join("slot"), dir.join("data"));
        for d in ["cache", "lib/apk", "lib/ci-new", "log/nginx", "empty"] {
            fs::create_dir_all(slot.join(d)).unwrap();
        }
        std::os::unix::fs::symlink("/run", slot.join("run")).unwrap();
        for d in ["cache", "lib/apk", "log"] {
            fs::create_dir_all(data.join(d)).unwrap();
        }
        fs::write(data.join("lib/apk/changed-by-the-machine"), "kept").unwrap();
        let missing = missing_var_entries(&slot, &data).unwrap();
        let missing: Vec<_> = missing.iter().map(|p| p.to_str().unwrap()).collect();
        assert_eq!(missing, ["empty", "lib/ci-new", "log/nginx", "run"]);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn adds_users_a_newer_slot_brings() {
        let slot = "root:x:0:0::/root:/bin/sh\nsshd:x:22:22::/dev/null:/sbin/nologin\nnew:x:90:90::/:/sbin/nologin\n";
        let machine = "root:x:0:0::/root:/bin/sh\nsshd:x:22:22::/dev/null:/sbin/nologin\nali:x:1000:1000::/home/ali:/bin/sh";
        let merged = merge_accounts(slot, machine).unwrap();
        assert_eq!(merged, format!("{machine}\nnew:x:90:90::/:/sbin/nologin\n"));
        assert_eq!(merge_accounts(slot, &merged), None);
    }

    #[test]
    fn only_the_word_edel_safe_marks_a_safe_start() {
        assert!(is_safe_start("root=PARTUUID=x quiet edel.safe\n"));
        assert!(!is_safe_start("root=PARTUUID=x quiet edel.safer"));
        assert!(!is_safe_start("edel.tag=edel.safe"));
    }

    #[test]
    fn status_names_developer_mode_only_when_it_is_or_will_be_on() {
        assert_eq!(developer_state(false, None, false), None);
        assert_eq!(developer_state(true, Some("on\n"), false), Some("on"));
        let held = developer_state(true, Some("held"), false).unwrap();
        assert!(held.starts_with("on, but"), "{held}");
        assert_eq!(
            developer_state(true, None, false),
            Some("on after a restart")
        );
        let off = developer_state(false, Some("on"), false);
        assert_eq!(off, Some("off after a restart"));
        let reset = developer_state(false, None, true).unwrap();
        assert!(reset.contains("deleted"), "{reset}");
    }

    #[test]
    fn grows_only_when_the_disk_has_room() {
        // The image as built: the partition ends 1 MiB before the end.
        assert!(!should_grow(4_460_544, 4_327_424, 131_072 - 2048));
        assert!(!should_grow(4_460_544, 4_327_424, 131_072));
        // The same image on a disk 2 GiB larger.
        assert!(should_grow(4_460_544 + 4_194_304, 4_327_424, 131_072));
    }

    #[test]
    fn never_adds_an_account_whose_id_is_taken() {
        let machine = "root:x:0:0::/root:/bin/sh\nali:x:1000:1000::/home/ali:/bin/sh\nsshd:x:22:22::/dev/null:/sbin/nologin\n";
        let slot = "root:x:0:0::/root:/bin/sh\nsshd:x:22:22::/dev/null:/sbin/nologin\nnew:x:22:22::/:/sbin/nologin\nalso:x:23:23::/:/sbin/nologin\n";
        let merged = merge_accounts(slot, machine).unwrap();
        assert!(!merged.contains("new:x:22"));
        assert!(merged.ends_with("also:x:23:23::/:/sbin/nologin\n"));
        // shadow lines carry a date there, which many accounts share.
        let shadow = merge_accounts(
            "a:*:19000:0:99999:7:::\nb:!:19000:0:99999:7:::\n",
            "a:*:19000:0:99999:7:::\n",
        );
        assert_eq!(
            shadow.unwrap(),
            "a:*:19000:0:99999:7:::\nb:!:19000:0:99999:7:::\n"
        );
    }
}
