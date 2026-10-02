//! `edel boot mount-data`: the data partition, partition 4, which holds
//! what outlives every update (roadmap M1.2). The `edel-data` service runs
//! it in sysinit, before the boot runlevel writes to `/var`. On the first
//! boot of a disk larger than the image, the partition grows to the end of
//! the disk.

use std::fs;
use std::io::Write;
use std::path::Path;
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

fn is_mounted(dir: &str) -> Result<bool> {
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grows_only_when_the_disk_has_room() {
        // The image as built: the partition ends 1 MiB before the end.
        assert!(!should_grow(4_460_544, 4_327_424, 131_072 - 2048));
        assert!(!should_grow(4_460_544, 4_327_424, 131_072));
        // The same image on a disk 2 GiB larger.
        assert!(should_grow(4_460_544 + 4_194_304, 4_327_424, 131_072));
    }
}
