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

use anyhow::{Context, Result};

use crate::boot::DATA_PARTITION;
use crate::update::{Disk, run};

const MOUNT_POINT: &str = "/data";
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
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
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
        .context("starting sfdisk")?;
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
    for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
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

/// Account files a machine copies into its own `/etc` once a person or a
/// package adds a user.
const ACCOUNT_FILES: [&str; 3] = ["passwd", "group", "shadow"];

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
            // Alpine hands out system ids per build, so a slot's new
            // account may carry an id this machine already gave someone
            // else; adding it would make two names share one id.
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
    for file in ACCOUNT_FILES {
        let machine_path = upper.join(file);
        let Ok(machine) = fs::read_to_string(&machine_path) else {
            continue;
        };
        let slot = fs::read_to_string(Path::new("/etc").join(file)).unwrap_or_default();
        if let Some(merged) = merge_accounts(&slot, &machine) {
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
    Ok(())
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
        for rel in missing_var_entries(Path::new("/var"), &data.join("var"))? {
            let dest = data.join("var").join(&rel);
            run(Command::new("cp")
                .arg("-a")
                .arg(Path::new("/var").join(&rel))
                .arg(dest.parent().unwrap_or(data)))?;
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
        run(Command::new("mount")
            .args(["-t", "ext4", "-o", "noatime"])
            .arg(&dev)
            .arg(MOUNT_POINT))
        .with_context(|| format!("cannot mount {} at {MOUNT_POINT}", dev.display()))?;
    }
    // Online resize: does nothing when the file system already fills it.
    if let Err(err) = run(Command::new("resize2fs")
        .arg(&dev)
        .stdout(Stdio::null())
        .stderr(Stdio::null()))
    {
        eprintln!("warning: could not grow the data file system: {err:#}");
    }
    let _ = Command::new("df").args(["-m", MOUNT_POINT]).status();
    println!("edel-data: mounted {MOUNT_POINT}");
    // Each step runs even when one before it failed: a failed /etc overlay
    // must not also leave /home, /var and /tmp off /data.
    let mut first_error = None;
    for (step, result) in [
        ("/etc", overlay_etc()),
        ("/home, /var and /root", bind_home_and_var()),
        ("/tmp", tmp_on_tmpfs()),
    ] {
        if let Err(err) = result {
            eprintln!("edel-data: {step}: {err:#}");
            first_error.get_or_insert(err);
        }
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
