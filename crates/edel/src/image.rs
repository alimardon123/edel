//! `edel image build`: turns an image definition into a container tarball or
//! a VM root filesystem, using Alpine's own `apk` to install packages.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::def::{ImageDef, Variant};
use crate::run::{Runner, ensure_nothing_mounted_under};

/// Where Alpine keeps its trusted package signing keys. The build host must
/// be Alpine (or the alpine container) so these are present and trusted.
const HOST_KEYS: &str = "/etc/apk/keys";

pub struct Build<'a> {
    pub def: &'a ImageDef,
    /// Directory holding the definition file; `files` paths are relative to it.
    pub def_dir: PathBuf,
    pub out: PathBuf,
    pub runner: Runner,
}

impl Build<'_> {
    pub fn run(&self) -> Result<()> {
        let def = self.def;
        let work = self.out.join("work").join(def.stem());
        let root = work.join("rootfs");

        self.prepare_root(&work, &root)?;
        self.install_packages(&root)?;
        self.enable_services(&root)?;
        self.copy_files(&root)?;
        self.configure(&root)?;

        match def.variant {
            Variant::Container => self.pack_container(&root),
            Variant::Vm => self.pack_vm(&root),
        }
    }

    fn prepare_root(&self, work: &Path, root: &Path) -> Result<()> {
        self.runner.step(&format!(
            "start a fresh root filesystem in {}",
            root.display()
        ));
        if self.runner.dry_run {
            return Ok(());
        }
        if work.exists() {
            ensure_nothing_mounted_under(work)?;
            fs::remove_dir_all(work).with_context(|| format!("removing {}", work.display()))?;
        }
        let apk_dir = root.join("etc/apk");
        fs::create_dir_all(apk_dir.join("keys"))?;
        for key in fs::read_dir(HOST_KEYS).with_context(|| format!("reading {HOST_KEYS}"))? {
            let key = key?;
            fs::copy(key.path(), apk_dir.join("keys").join(key.file_name()))?;
        }
        fs::write(apk_dir.join("repositories"), self.def.repositories_file())?;
        Ok(())
    }

    fn install_packages(&self, root: &Path) -> Result<()> {
        // Package scripts (for example the one that builds the initramfs)
        // run inside the new root and need proc, sys and dev there.
        let _mounts = self.runner.mount_kernel_fs(root)?;
        self.runner.run(
            Command::new("apk")
                .arg("--root")
                .arg(root)
                .args(["--arch", &self.def.arch])
                .args(["--initdb", "--update-cache", "--no-progress", "add"])
                .args(&self.def.packages.install),
        )
    }

    fn enable_services(&self, root: &Path) -> Result<()> {
        for (level, service) in self.def.services.entries() {
            self.runner
                .step(&format!("enable {service} in runlevel {level}"));
            if self.runner.dry_run {
                continue;
            }
            if !root.join("etc/init.d").join(service).exists() {
                bail!("service {service} has no /etc/init.d/{service}; is its package installed?");
            }
            let dir = root.join("etc/runlevels").join(level);
            fs::create_dir_all(&dir)?;
            let link = dir.join(service);
            if !link.exists() {
                std::os::unix::fs::symlink(format!("/etc/init.d/{service}"), &link)
                    .with_context(|| format!("linking {}", link.display()))?;
            }
        }
        Ok(())
    }

    fn copy_files(&self, root: &Path) -> Result<()> {
        for dir in &self.def.files {
            let src = self.def_dir.join(dir);
            if !self.runner.dry_run && !src.is_dir() {
                bail!("files directory {} does not exist", src.display());
            }
            // "src/." copies the directory's contents, including dotfiles.
            // Plain -R (not -a) so copies are owned by root, not by whoever
            // checked out the repository; modes such as +x are kept.
            self.runner
                .run(Command::new("cp").arg("-R").arg(src.join(".")).arg(root))?;
        }
        Ok(())
    }

    fn configure(&self, root: &Path) -> Result<()> {
        // OS defaults live in /usr (stateless /etc, ADR-006); /etc only points at them.
        self.runner
            .step("point /etc/os-release at /usr/lib/os-release");
        self.runner.step("lock the root password");
        if let Some(hostname) = &self.def.hostname {
            self.runner.step(&format!("set the hostname to {hostname}"));
        }
        if self.runner.dry_run {
            return Ok(());
        }

        if !root.join("usr/lib/os-release").is_file() {
            bail!("the image has no /usr/lib/os-release; add one to the files directories");
        }
        let os_release = root.join("etc/os-release");
        if os_release.symlink_metadata().is_ok() {
            fs::remove_file(&os_release)?;
        }
        std::os::unix::fs::symlink("../usr/lib/os-release", &os_release)?;

        let shadow_path = root.join("etc/shadow");
        let shadow = fs::read_to_string(&shadow_path).context("reading /etc/shadow")?;
        fs::write(&shadow_path, lock_root(&shadow)?)?;

        if let Some(hostname) = &self.def.hostname {
            fs::write(root.join("etc/hostname"), format!("{hostname}\n"))?;
        }
        Ok(())
    }

    fn pack_container(&self, root: &Path) -> Result<()> {
        let tarball = self.out.join(format!("{}.tar.gz", self.def.stem()));
        self.runner.run(
            Command::new("tar")
                .args(["--numeric-owner", "-czf"])
                .arg(&tarball)
                .arg("-C")
                .arg(root)
                .arg("."),
        )?;
        println!("container image: {}", tarball.display());
        Ok(())
    }

    fn pack_vm(&self, root: &Path) -> Result<()> {
        let vm = self
            .def
            .vm
            .as_ref()
            .context("vm image without a [vm] section")?;
        let stem = self.out.join(self.def.stem());
        let disk = stem.with_extension("ext4");
        let kernel = stem.with_extension("vmlinuz");
        let initramfs = stem.with_extension("initramfs");

        self.runner.step(&format!(
            "copy the {} kernel and initramfs next to the disk image",
            vm.kernel
        ));
        self.runner
            .step(&format!("create a sparse {} MiB disk image", vm.size_mib));
        if !self.runner.dry_run {
            let boot = root.join("boot");
            fs::copy(boot.join(format!("vmlinuz-{}", vm.kernel)), &kernel)
                .context("copying the kernel; did the kernel package install?")?;
            fs::copy(boot.join(format!("initramfs-{}", vm.kernel)), &initramfs)
                .context("copying the initramfs; did mkinitfs run?")?;
            let file = fs::File::create(&disk)?;
            file.set_len(vm.size_mib * 1024 * 1024)?;
        }
        self.runner.run(
            Command::new("mkfs.ext4")
                .args(["-q", "-F", "-L", "edel-root", "-d"])
                .arg(root)
                .arg(&disk),
        )?;
        println!("vm disk: {}", disk.display());
        println!("vm kernel: {}", kernel.display());
        println!("vm initramfs: {}", initramfs.display());
        Ok(())
    }
}

/// Replaces root's password hash with `*`, so nobody can log in as root with
/// a password. Access is granted later by keys or the system file.
fn lock_root(shadow: &str) -> Result<String> {
    let mut found = false;
    let mut out = String::with_capacity(shadow.len() + 1);
    for line in shadow.lines() {
        match line.split_once(':') {
            Some(("root", rest)) => {
                found = true;
                let after_hash = rest.split_once(':').map_or("", |(_, after)| after);
                out.push_str("root:*:");
                out.push_str(after_hash);
            }
            _ => out.push_str(line),
        }
        out.push('\n');
    }
    if !found {
        bail!("/etc/shadow has no root entry");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locks_an_empty_root_password() {
        let shadow = "root::0:0:99999:7:::\nbin:!::0:::::\n";
        assert_eq!(
            lock_root(shadow).unwrap(),
            "root:*:0:0:99999:7:::\nbin:!::0:::::\n"
        );
    }

    #[test]
    fn locks_a_set_root_password() {
        let shadow = "root:$6$salt$hash:19000:0:::::\n";
        assert_eq!(lock_root(shadow).unwrap(), "root:*:19000:0:::::\n");
    }

    #[test]
    fn leaves_lookalike_users_alone() {
        let shadow = "rootless::0:::::\nroot::0:::::\n";
        assert_eq!(
            lock_root(shadow).unwrap(),
            "rootless::0:::::\nroot:*:0:::::\n"
        );
    }

    #[test]
    fn fails_without_a_root_entry() {
        assert!(lock_root("bin:!::0:::::\n").is_err());
    }
}
