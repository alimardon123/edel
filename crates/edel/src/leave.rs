//! `edel boot leave`: at shutdown, after the login screen and ssh have
//! stopped and before the data partition is unmounted, ends what people's
//! sessions left running and unmounts their FUSE file systems (2026-10-24).
//!
//! A session's processes outlive the service that started them: greetd and
//! sshd stop, but a person's portals, Flatpak sandboxes and rootless
//! containers go on, in mount namespaces of their own where `fuser` cannot
//! see them, so the data partition stayed "in use" at unmount. Worse,
//! OpenRC's killprocs stops every process at once before it signals them,
//! the document portal's FUSE server too, and a process waiting on that
//! file system then never returns: the desktop's poweroff hung at
//! "Terminating remaining processes". Ending people's processes first,
//! each with time to finish, then lazily unmounting FUSE, leaves killprocs
//! and the unmount nothing to wait for.

use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::Result;

use edel::users::{FIRST_UID, NOBODY};

/// How long people's processes get to end after the polite signal, and
/// then after the kill.
const POLITE: Duration = Duration::from_secs(3);
const FIRM: Duration = Duration::from_secs(2);

/// The real user id in a `/proc/PID/status` text.
pub fn uid_in(status: &str) -> Option<u32> {
    status
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))
        .and_then(|ids| ids.split_whitespace().next())
        .and_then(|uid| uid.parse().ok())
}

/// Whether `uid` is a person's, or the stick's own account `live`.
pub fn a_persons(uid: u32) -> bool {
    (FIRST_UID..NOBODY).contains(&uid) || uid == edel::places::LIVE_UID
}

/// The FUSE mount points in a `/proc/self/mounts` text, deepest first, so
/// one inside another goes before it. Spaces in a path are `\040`.
pub fn fuse_mounts(mounts: &str) -> Vec<String> {
    let mut found: Vec<String> = mounts
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let (point, kind) = (fields.get(1)?, fields.get(2)?);
            (*kind == "fuse" || kind.starts_with("fuse.")).then(|| point.replace("\\040", " "))
        })
        .collect();
    found.sort_by_key(|point| std::cmp::Reverse(point.matches('/').count()));
    found
}

/// The processes of people now running.
fn people() -> Vec<i32> {
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    dir.filter_map(|entry| {
        let pid: i32 = entry.ok()?.file_name().to_str()?.parse().ok()?;
        let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
        a_persons(uid_in(&status)?).then_some(pid)
    })
    .collect()
}

/// Sends `signal` to each of `pids`, then waits up to `time` for every
/// person's process to be gone; gives those left.
fn end(pids: &[i32], signal: i32, time: Duration) -> Vec<i32> {
    for pid in pids {
        // SAFETY: kill only sends a signal; a process already gone is ESRCH.
        unsafe {
            libc::kill(*pid, signal);
        }
    }
    let until = Instant::now() + time;
    loop {
        let left = people();
        if left.is_empty() || Instant::now() >= until {
            return left;
        }
        sleep(Duration::from_millis(100));
    }
}

pub fn leave() -> Result<()> {
    let pids = people();
    if !pids.is_empty() {
        let left = end(&pids, libc::SIGTERM, POLITE);
        let killed = left.len();
        let left = if left.is_empty() {
            left
        } else {
            end(&left, libc::SIGKILL, FIRM)
        };
        println!(
            "edel boot leave: ended {} processes of people's sessions, {killed} of them with a kill{}",
            pids.len(),
            if left.is_empty() {
                String::new()
            } else {
                format!("; {} would not end", left.len())
            }
        );
    }
    let mounts = std::fs::read_to_string("/proc/self/mounts").unwrap_or_default();
    for point in fuse_mounts(&mounts) {
        let Ok(at) = std::ffi::CString::new(point.as_bytes()) else {
            continue;
        };
        // SAFETY: a NUL-terminated path; MNT_DETACH takes it out of the
        // tree at once and lets the kernel finish when nothing uses it.
        let done = unsafe { libc::umount2(at.as_ptr(), libc::MNT_DETACH) };
        if done == 0 {
            println!("edel boot leave: unmounted {point}");
        } else if Path::new(&point).exists() {
            eprintln!(
                "warning: edel boot leave: could not unmount {point}: {}",
                std::io::Error::last_os_error()
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_real_user_id_is_the_first_of_the_uid_line() {
        let status = "Name:\tmpv\nState:\tS (sleeping)\nUid:\t1000\t1000\t1000\t1000\nGid:\t1000\n";
        assert_eq!(uid_in(status), Some(1000));
        assert_eq!(uid_in("Name:\tx\n"), None);
    }

    #[test]
    fn people_and_live_are_ended_and_the_system_is_not() {
        assert!(a_persons(1000));
        assert!(a_persons(edel::places::LIVE_UID));
        assert!(!a_persons(0));
        assert!(!a_persons(101));
        assert!(!a_persons(NOBODY));
    }

    #[test]
    fn fuse_mounts_come_deepest_first_and_others_not_at_all() {
        let mounts = "/dev/vda4 /data ext4 rw 0 0\n\
                      portal /run/user/1000/doc fuse.portal rw 0 0\n\
                      gvfsd-fuse /run/user/1000/gvfs fuse.gvfsd-fuse rw 0 0\n\
                      sshfs /home/ci/far\\040away fuse.sshfs rw 0 0\n\
                      inner /home/ci/far\\040away/in fuse rw 0 0\n";
        assert_eq!(
            fuse_mounts(mounts),
            [
                "/run/user/1000/doc",
                "/run/user/1000/gvfs",
                "/home/ci/far away/in",
                "/home/ci/far away"
            ]
        );
    }
}
