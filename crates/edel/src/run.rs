//! Running external tools, with a dry-run mode that only prints them.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use anyhow::{Context, Result, bail};

pub struct Runner {
    pub dry_run: bool,
}

impl Runner {
    /// Prints the command, then runs it unless this is a dry run.
    pub fn run(&self, cmd: &mut Command) -> Result<()> {
        println!("+ {}", describe(cmd));
        if self.dry_run {
            return Ok(());
        }
        let status = cmd
            .status()
            .with_context(|| format!("starting {}", describe(cmd)))?;
        check(cmd, status)
    }

    /// Like [`Runner::run`], with `input` fed to the command's standard input.
    pub fn run_with_input(&self, cmd: &mut Command, input: &str) -> Result<()> {
        println!("+ {} <<'EOF'\n{input}EOF", describe(cmd));
        if self.dry_run {
            return Ok(());
        }
        let mut child = cmd
            .stdin(Stdio::piped())
            .spawn()
            .with_context(|| format!("starting {}", describe(cmd)))?;
        // Taking stdin out of the child closes it once written, so the
        // command sees the end of its input.
        child
            .stdin
            .take()
            .context("no stdin")?
            .write_all(input.as_bytes())
            .with_context(|| format!("writing to {}", describe(cmd)))?;
        let status = child.wait()?;
        check(cmd, status)
    }

    /// Prints a step that edel performs itself rather than through a tool.
    pub fn step(&self, what: &str) {
        println!("= {what}");
    }

    /// Mounts the kernel filesystems package scripts need (proc, sys, dev)
    /// inside `root`. They are unmounted when the guard is dropped, even if
    /// the build fails.
    pub fn mount_kernel_fs(&self, root: &Path) -> Result<MountGuard<'_>> {
        let mut guard = MountGuard {
            runner: self,
            mounted: Vec::new(),
        };
        let mounts: [(&str, &[&str]); 3] = [
            ("proc", &["-t", "proc", "proc"]),
            ("sys", &["-o", "bind", "/sys"]),
            ("dev", &["-o", "bind", "/dev"]),
        ];
        for (dir, args) in mounts {
            let target = root.join(dir);
            if !self.dry_run {
                std::fs::create_dir_all(&target)?;
            }
            self.run(Command::new("mount").args(args).arg(&target))?;
            guard.mounted.push(target);
        }
        Ok(guard)
    }
}

pub struct MountGuard<'a> {
    runner: &'a Runner,
    mounted: Vec<PathBuf>,
}

impl Drop for MountGuard<'_> {
    fn drop(&mut self) {
        while let Some(target) = self.mounted.pop() {
            if let Err(err) = self.runner.run(Command::new("umount").arg(&target)) {
                eprintln!("warning: could not unmount {}: {err:#}", target.display());
            }
        }
    }
}

fn check(cmd: &Command, status: ExitStatus) -> Result<()> {
    if !status.success() {
        bail!("{} failed with {status}", describe(cmd));
    }
    Ok(())
}

/// Fails if anything is still mounted at or below `dir`. Deleting a directory
/// with a live bind mount of /dev inside it would delete the host's devices.
pub fn ensure_nothing_mounted_under(dir: &Path) -> Result<()> {
    let mounts =
        std::fs::read_to_string("/proc/self/mounts").context("reading /proc/self/mounts")?;
    let dir = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    if let Some(target) = mounted_under(&mounts, &dir) {
        bail!(
            "{} is still mounted inside {}; unmount it before rebuilding",
            target.display(),
            dir.display()
        );
    }
    Ok(())
}

fn mounted_under(mounts: &str, dir: &Path) -> Option<PathBuf> {
    mounts
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1))
        .map(unescape_mount_path)
        .find(|target| target.starts_with(dir))
}

/// /proc/self/mounts escapes spaces and a few other bytes as octal (`\040`).
fn unescape_mount_path(field: &str) -> PathBuf {
    let mut out = String::with_capacity(field.len());
    let mut chars = field.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            let code: String = chars.by_ref().take(3).collect();
            match u8::from_str_radix(&code, 8) {
                Ok(byte) => out.push(byte as char),
                Err(_) => {
                    out.push('\\');
                    out.push_str(&code);
                }
            }
        } else {
            out.push(c);
        }
    }
    PathBuf::from(out)
}

fn describe(cmd: &Command) -> String {
    std::iter::once(cmd.get_program())
        .chain(cmd.get_args())
        .map(|part| part.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOUNTS: &str = "\
proc /proc proc rw 0 0
proc /build/out/work/edel-vm-x86_64/rootfs/proc proc rw 0 0
/dev/vda /build/my\\040dir ext4 rw 0 0
";

    #[test]
    fn finds_a_mount_inside_the_work_dir() {
        let found = mounted_under(MOUNTS, Path::new("/build/out/work"));
        assert_eq!(
            found,
            Some(PathBuf::from("/build/out/work/edel-vm-x86_64/rootfs/proc"))
        );
    }

    #[test]
    fn ignores_mounts_elsewhere_and_lookalike_prefixes() {
        assert_eq!(mounted_under(MOUNTS, Path::new("/build/out/wo")), None);
        assert_eq!(mounted_under(MOUNTS, Path::new("/elsewhere")), None);
    }

    #[test]
    fn unescapes_spaces_in_mount_paths() {
        assert_eq!(
            mounted_under(MOUNTS, Path::new("/build/my dir")),
            Some(PathBuf::from("/build/my dir"))
        );
    }

    #[test]
    fn dry_run_prints_without_running() {
        let runner = Runner { dry_run: true };
        runner
            .run(&mut Command::new("definitely-not-a-real-tool"))
            .unwrap();
    }
}
