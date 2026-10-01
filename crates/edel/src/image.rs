//! `edel image build`: turns an image definition into a container tarball or
//! a bootable A/B VM disk, using Alpine's own `apk` to install packages.

use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::boot::{self, Layout};
use crate::def::{ImageDef, Variant};
use crate::run::{Runner, ensure_nothing_mounted_under};

const MIB: u64 = 1024 * 1024;

/// Where Alpine keeps its trusted package signing keys. The build host must
/// be Alpine (or the alpine container) so these are present and trusted.
const HOST_KEYS: &str = "/etc/apk/keys";

pub struct Build<'a> {
    pub def: &'a ImageDef,
    /// Directory holding the definition file; `files` paths are relative to it.
    pub def_dir: PathBuf,
    /// More directories copied over the image after the definition's own,
    /// for test images. Not part of the definition, so a release image can
    /// never pick them up by accident.
    pub extra_files: Vec<PathBuf>,
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
        // Our files first: some of them are services to enable.
        self.copy_files(&root)?;
        self.enable_services(&root)?;
        self.configure(&root)?;

        match def.variant {
            Variant::Container => self.pack_container(&root),
            Variant::Vm => self.pack_vm(&work, &root),
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
                bail!(
                    "service {service} has no /etc/init.d/{service}; \
                     is its package installed or its file in a files directory?"
                );
            }
            let dir = root.join("etc/runlevels").join(level);
            fs::create_dir_all(&dir)?;
            let link = dir.join(service);
            // symlink_metadata, not exists: the link points at a path inside
            // the image, which exists() would look up on the build host.
            if link.symlink_metadata().is_err() {
                std::os::unix::fs::symlink(format!("/etc/init.d/{service}"), &link)
                    .with_context(|| format!("linking {}", link.display()))?;
            }
        }
        Ok(())
    }

    fn copy_files(&self, root: &Path) -> Result<()> {
        let dirs = self.def.files.iter().map(|dir| self.def_dir.join(dir));
        for src in dirs.chain(self.extra_files.iter().cloned()) {
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

    /// Writes the VM's two outputs: `<stem>.ext4`, one root slot, which is
    /// also what an update installs; and `<stem>.img`, a bootable disk with
    /// GRUB, slot A filled and slot B empty (see `boot.rs`).
    fn pack_vm(&self, work: &Path, root: &Path) -> Result<()> {
        let vm = self
            .def
            .vm
            .as_ref()
            .context("vm image without a [vm] section")?;
        let stem = self.out.join(self.def.stem());
        let update = stem.with_extension("ext4");
        let disk = stem.with_extension("img");
        let layout = Layout {
            slot_mib: vm.slot_mib,
        };

        self.make_slot(root, &update, vm.slot_mib)?;
        // Slot A gets a filesystem of its own rather than a copy of the
        // update image: the kernel finds its root by filesystem UUID, so no
        // two filesystems a machine can see may share one.
        let slot_a = work.join("slot-a.ext4");
        self.make_slot(root, &slot_a, vm.slot_mib)?;
        let esp = work.join("esp.img");
        self.make_esp(&work.join("esp"), &esp, &vm.kernel, &vm.cmdline)?;

        self.runner.step(&format!(
            "create a sparse {} MiB disk image",
            layout.disk_mib()
        ));
        if !self.runner.dry_run {
            File::create(&disk)?.set_len(layout.disk_mib() * MIB)?;
        }
        self.runner.run_with_input(
            Command::new("sfdisk").arg("--quiet").arg(&disk),
            &layout.sfdisk_script(),
        )?;
        self.runner
            .step("copy the EFI system partition and slot A into the disk");
        if !self.runner.dry_run {
            copy_sparse(&esp, &disk, layout.esp_start_mib() * MIB)?;
            copy_sparse(&slot_a, &disk, layout.slot_start_mib(0) * MIB)?;
        }
        println!("vm disk: {}", disk.display());
        println!("update image: {}", update.display());
        Ok(())
    }

    fn make_slot(&self, root: &Path, image: &Path, size_mib: u64) -> Result<()> {
        self.runner.step(&format!(
            "create a sparse {size_mib} MiB slot image {}",
            image.display()
        ));
        if !self.runner.dry_run {
            File::create(image)?.set_len(size_mib * MIB)?;
        }
        self.runner.run(
            Command::new("mkfs.ext4")
                .args(["-q", "-F", "-L", "edel", "-d"])
                .arg(root)
                .arg(image),
        )
    }

    /// Builds the EFI system partition: GRUB as the removable-disk boot
    /// file, plus its config and environment block.
    fn make_esp(&self, dir: &Path, esp: &Path, kernel: &str, cmdline: &str) -> Result<()> {
        let (target, efi_name) = boot::efi_target(&self.def.arch)?;
        let efi = dir.join(efi_name);
        let cfg = dir.join("grub.cfg");
        let env = dir.join("grubenv");

        self.runner.step(&format!(
            "write GRUB's config and environment block in {}",
            dir.display()
        ));
        if !self.runner.dry_run {
            fs::create_dir_all(dir)?;
            fs::write(&cfg, boot::grub_cfg(kernel, cmdline))?;
            fs::write(&env, boot::initial_grubenv())?;
        }
        self.runner.run(
            Command::new("grub-mkimage")
                .args(["-O", target, "-d"])
                .arg(Path::new("/usr/lib/grub").join(target))
                .args(["-p", boot::GRUB_PREFIX, "-o"])
                .arg(&efi)
                .args(boot::GRUB_MODULES),
        )?;

        self.runner.step(&format!(
            "create a sparse {} MiB EFI system partition image",
            boot::ESP_MIB
        ));
        if !self.runner.dry_run {
            File::create(esp)?.set_len(boot::ESP_MIB * MIB)?;
        }
        self.runner.run(
            Command::new("mkfs.vfat")
                .args(["-F", "32", "-n", boot::ESP_LABEL])
                .arg(esp),
        )?;
        // mtools writes into the FAT image directly, without mounting it.
        let on_esp = |path: &str| format!("::{path}");
        self.runner.run(
            Command::new("mmd")
                .arg("-i")
                .arg(esp)
                .args(["::/EFI", "::/EFI/BOOT"])
                .arg(on_esp(boot::GRUB_PREFIX)),
        )?;
        self.runner.run(
            Command::new("mcopy")
                .arg("-i")
                .arg(esp)
                .arg(&efi)
                .arg(on_esp(&format!("/EFI/BOOT/{efi_name}"))),
        )?;
        self.runner.run(
            Command::new("mcopy")
                .arg("-i")
                .arg(esp)
                .arg(&cfg)
                .arg(&env)
                .arg(on_esp(&format!("{}/", boot::GRUB_PREFIX))),
        )
    }
}

/// Copies `src` into `dst` starting `offset` bytes in, skipping blocks of
/// zeros so sparse images stay sparse. That region of `dst` must already be
/// zeros, as it is in a freshly created disk image.
fn copy_sparse(src: &Path, dst: &Path, offset: u64) -> Result<()> {
    let mut input = File::open(src).with_context(|| format!("opening {}", src.display()))?;
    let output = OpenOptions::new()
        .write(true)
        .open(dst)
        .with_context(|| format!("opening {}", dst.display()))?;
    let mut block = vec![0u8; 64 * 1024];
    let mut pos = offset;
    loop {
        let mut filled = 0;
        while filled < block.len() {
            let n = input.read(&mut block[filled..])?;
            if n == 0 {
                break;
            }
            filled += n;
        }
        if filled == 0 {
            return Ok(());
        }
        let chunk = &block[..filled];
        if chunk.iter().any(|&b| b != 0) {
            output
                .write_all_at(chunk, pos)
                .with_context(|| format!("writing {}", dst.display()))?;
        }
        pos += filled as u64;
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

    #[test]
    fn copies_into_place_and_keeps_holes() {
        let dir = std::env::temp_dir().join(format!("edel-copy-sparse-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let src = dir.join("src");
        let dst = dir.join("dst");

        // 200 KiB: data, a 128 KiB hole, more data at an odd length.
        let mut data = vec![0u8; 200 * 1024];
        data[..10].copy_from_slice(b"first part");
        data[196 * 1024..196 * 1024 + 4].copy_from_slice(b"last");
        fs::write(&src, &data).unwrap();
        File::create(&dst).unwrap().set_len(MIB).unwrap();

        copy_sparse(&src, &dst, 4096).unwrap();

        let out = fs::read(&dst).unwrap();
        assert_eq!(out.len() as u64, MIB);
        assert!(out[..4096].iter().all(|&b| b == 0));
        assert_eq!(&out[4096..4096 + data.len()], &data[..]);
        assert!(out[4096 + data.len()..].iter().all(|&b| b == 0));
        fs::remove_dir_all(&dir).unwrap();
    }
}
