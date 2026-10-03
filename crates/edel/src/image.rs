//! `edel image build`: turns an image definition into a container tarball or
//! a bootable A/B VM disk, using Alpine's own `apk` to install packages.

use std::fs::{self, File, OpenOptions};
use std::io::Read;

use std::os::unix::fs::{FileExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use edel::features;
use flate2::Compression;
use flate2::write::GzEncoder;

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
    /// Another health timeout in seconds, for test images (M1.5).
    pub health_timeout: Option<u64>,
    /// Another slot size in MiB, for test images.
    pub slot_mib: Option<u64>,
    /// More public keys for updates, for test images (M1.6).
    pub extra_keys: Vec<PathBuf>,
    /// A tag written into grub.cfg so the loader differs, for test images
    /// (M1.8).
    pub loader_tag: Option<String>,
    /// The release version (`YYYY.MM.N`) written into os-release (M3.1);
    /// without one the overlay's development version stays
    pub version: Option<String>,
    /// The channel written into os-release, such as `preview` (M3.1)
    pub channel: String,
    /// An apk cache shared by every image of one run, so they all install
    /// from one package index (M3.1)
    pub apk_cache: Option<PathBuf>,
    /// Whether to write the disk's `.gz` copy releases publish; test
    /// images nobody downloads skip it (the slot's `.gz` is always made)
    pub compress: bool,
    pub out: PathBuf,
    pub runner: Runner,
}

/// The platform level images promise (ADR-005, M8.8): 0 until level 1
/// exists.
pub const PLATFORM_LEVEL: u32 = 0;

/// `os-release` with the build's version and channel (M3.1): the version
/// replaces `VERSION_ID`, `VERSION` and `PRETTY_NAME` when given, and
/// `EDEL_CHANNEL`, `EDEL_ARCH` and `EDEL_PLATFORM_LEVEL` are added.
pub fn with_version(os_release: &str, version: Option<&str>, channel: &str, arch: &str) -> String {
    let mut out = String::new();
    for line in os_release.lines() {
        let key = line.split('=').next().unwrap_or_default();
        let replaced = match (key, version) {
            ("VERSION_ID", Some(v)) => format!("VERSION_ID={v}"),
            ("VERSION", Some(v)) => format!("VERSION=\"{v} ({channel})\""),
            ("PRETTY_NAME", Some(v)) => format!("PRETTY_NAME=\"Edel OS {v}\""),
            _ => line.to_string(),
        };
        out.push_str(&replaced);
        out.push('\n');
    }
    out.push_str(&format!(
        "EDEL_CHANNEL=\"{channel}\"\nEDEL_ARCH=\"{arch}\"\nEDEL_PLATFORM_LEVEL={PLATFORM_LEVEL}\n"
    ));
    out
}

/// `YYYY.MM.N` and the like: numbers joined by dots, as
/// `release::compare_versions` orders them.
pub fn is_version(version: &str) -> bool {
    !version.is_empty()
        && version
            .split('.')
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
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
        self.install_features(&root)?;
        if def.variant == Variant::Vm {
            self.install_edel(&root)?;
            self.install_keys(&root)?;
        }
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
        let mut apk = Command::new("apk");
        apk.arg("--root").arg(root).args(["--arch", &self.def.arch]);
        apk.args(["--initdb", "--no-progress"]);
        // With a shared cache, only the first image of a run fetches the
        // index; the others install from the same one.
        match &self.apk_cache {
            Some(cache) if has_index(cache) => {
                apk.arg("--cache-dir").arg(cache);
            }
            Some(cache) => {
                if !self.runner.dry_run {
                    fs::create_dir_all(cache)?;
                }
                apk.arg("--cache-dir").arg(cache).arg("--update-cache");
            }
            None => {
                apk.arg("--update-cache");
            }
        }
        self.runner.run(apk.arg("add").args(&self.def.packages))?;
        if let Some(vm) = &self.def.vm {
            self.make_initramfs(root, &vm.kernel)?;
        }
        self.record_packages(root)
    }

    /// Rebuilds the initramfs with the mkinitfs features the image's
    /// features name (`initramfs`, roadmap M4.0), and leaves them in
    /// `/etc/mkinitfs/mkinitfs.conf` for any later kernel install.
    fn make_initramfs(&self, root: &Path, flavor: &str) -> Result<()> {
        let features = self.def.initramfs.join(" ");
        self.runner.step(&format!(
            "rebuild /boot/initramfs-{flavor} with the features {features}"
        ));
        if self.runner.dry_run {
            return Ok(());
        }
        let conf = root.join("etc/mkinitfs/mkinitfs.conf");
        fs::create_dir_all(conf.parent().unwrap_or(root))?;
        fs::write(&conf, format!("features=\"{features}\"\n"))?;
        let suffix = format!("-{flavor}");
        let mut kernels = Vec::new();
        for entry in fs::read_dir(root.join("lib/modules"))? {
            let name = entry?.file_name().to_string_lossy().into_owned();
            if name.ends_with(&suffix) {
                kernels.push(name);
            }
        }
        let [kernel] = kernels.as_slice() else {
            bail!("expected one {flavor} kernel in /lib/modules, found {kernels:?}");
        };
        self.runner.run(Command::new("chroot").arg(root).args([
            "mkinitfs",
            "-o",
            &format!("/boot/initramfs-{flavor}"),
            kernel,
        ]))
    }

    /// Writes the installed packages, one `name-version` a line, into the
    /// image (`/usr/share/edel/packages`) and beside it
    /// (`out/<image>.packages`), so every release says what is inside it.
    fn record_packages(&self, root: &Path) -> Result<()> {
        let beside = self.out.join(format!("{}.packages", self.def.stem()));
        self.runner.step(&format!(
            "record the installed packages in /usr/share/edel/packages and {}",
            beside.display()
        ));
        if self.runner.dry_run {
            return Ok(());
        }
        let out = Command::new("apk")
            .arg("--root")
            .arg(root)
            .args(["info", "-v"])
            .output()
            .context("starting apk info")?;
        if !out.status.success() {
            bail!(
                "apk info -v failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        let mut packages: Vec<&str> = std::str::from_utf8(&out.stdout)?.lines().collect();
        packages.sort_unstable();
        let text: String = packages.iter().map(|p| format!("{p}\n")).collect();
        let inside = root.join("usr/share/edel/packages");
        fs::create_dir_all(inside.parent().unwrap_or(root))?;
        fs::write(&inside, &text)?;
        fs::write(&beside, &text)?;
        Ok(())
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

    /// Copies each feature's `features/NAME/` over the root, in the order
    /// the definition lists them, then the test directories of `--files`.
    fn copy_files(&self, root: &Path) -> Result<()> {
        let dirs = self.def.features.iter().filter_map(|f| f.files.clone());
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

    /// Copies the running `edel` into the image as `/usr/bin/edel`, the
    /// updater of a bootable image. CI builds it inside `alpine:3.24`, so it
    /// links against the image's musl.
    fn install_edel(&self, root: &Path) -> Result<()> {
        self.runner.step("copy this edel binary to /usr/bin/edel");
        if self.runner.dry_run {
            return Ok(());
        }
        // The data partition's mount point; / may be read-only at boot.
        fs::create_dir_all(root.join("data"))?;
        let exe = std::env::current_exe().context("finding the running edel binary")?;
        let dest = root.join("usr/bin/edel");
        fs::create_dir_all(root.join("usr/bin"))?;
        fs::copy(&exe, &dest).with_context(|| format!("copying {}", exe.display()))?;
        fs::set_permissions(&dest, fs::Permissions::from_mode(0o755))?;
        Ok(())
    }

    /// Copies the public keys that may sign updates to
    /// `/usr/share/edel/keys/`, checking that each one is a key.
    fn install_keys(&self, root: &Path) -> Result<()> {
        let keys: Vec<PathBuf> = self
            .def
            .release
            .public_keys
            .iter()
            .map(|k| self.def_dir.join(k))
            .chain(self.extra_keys.iter().cloned())
            .collect();
        for key in &keys {
            // A secret key is 32 bytes of hex too; only the name tells
            // them apart, so a slot never carries NAME.key by mistake.
            if key.extension().is_none_or(|e| e != "pub") {
                bail!(
                    "{} is not a *.pub file; images carry only public keys, and NAME.key is the secret",
                    key.display()
                );
            }
            self.runner.step(&format!(
                "copy the public key {} to /usr/share/edel/keys/",
                key.display()
            ));
        }
        if self.runner.dry_run {
            return Ok(());
        }
        let dir = root.join("usr/share/edel/keys");
        fs::create_dir_all(&dir)?;
        for key in &keys {
            crate::release::read_public_key(key)?;
            let name = key.file_name().context("a key file needs a name")?;
            fs::copy(key, dir.join(name)).with_context(|| format!("copying {}", key.display()))?;
        }
        Ok(())
    }

    /// The image facts for os-release (one standard file for them, roadmap
    /// default row "Image facts"): the health keys, the switchable features
    /// shipped off (M4.0), and the hostname that `edel system apply` goes
    /// back to when the system file has none; none for containers.
    fn health_keys(&self) -> Option<String> {
        if self.def.health.is_empty() {
            return None;
        }
        let timeout = self.health_timeout.unwrap_or(self.def.image.health_timeout);
        let mut keys = format!(
            "EDEL_IMAGE=\"{}\"\nEDEL_HEALTH=\"{}\"\nEDEL_HEALTH_TIMEOUT={timeout}\n",
            self.def.stem(),
            self.def.health.join(" ")
        );
        if !self.def.off.is_empty() {
            keys.push_str(&format!(
                "EDEL_SERVICES_OFF=\"{}\"\n",
                self.def.off.join(" ")
            ));
        }
        if let Some(hostname) = &self.def.hostname {
            keys.push_str(&format!("EDEL_HOSTNAME=\"{hostname}\"\n"));
        }
        Some(keys)
    }

    /// Copies each listed feature's file to `/usr/share/edel/features/`,
    /// so the parts can ask what this machine has (ADR-008).
    fn install_features(&self, root: &Path) -> Result<()> {
        let names: Vec<&str> = self.def.features.iter().map(|f| f.name.as_str()).collect();
        self.runner.step(&format!(
            "copy the feature files of {} to {}",
            names.join(", "),
            features::DIR
        ));
        if self.runner.dry_run {
            return Ok(());
        }
        let dir = root.join(features::DIR.trim_start_matches('/'));
        fs::create_dir_all(&dir)?;
        for feature in &self.def.features {
            fs::write(dir.join(format!("{}.toml", feature.name)), &feature.text)?;
        }
        Ok(())
    }

    fn configure(&self, root: &Path) -> Result<()> {
        // OS defaults live in /usr (stateless /etc, ADR-006); /etc only points at them.
        self.runner
            .step("point /etc/os-release at /usr/lib/os-release");
        let health = self.health_keys();
        if let Some(keys) = &health {
            self.runner.step(&format!(
                "add {} to /usr/lib/os-release",
                keys.trim().replace('\n', " ")
            ));
        }
        self.runner.step(&format!(
            "write version {}, channel {} and platform level {PLATFORM_LEVEL} into /usr/lib/os-release",
            self.version.as_deref().unwrap_or("of the overlay"),
            self.channel
        ));
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
        let path = root.join("usr/lib/os-release");
        let mut text = with_version(
            &fs::read_to_string(&path)?,
            self.version.as_deref(),
            &self.channel,
            &self.def.arch,
        );
        if let Some(keys) = &health {
            text += keys;
        }
        fs::write(&path, text)?;

        let shadow_path = root.join("etc/shadow");
        let shadow = fs::read_to_string(&shadow_path).context("reading /etc/shadow")?;
        fs::write(&shadow_path, lock_root(&shadow)?)?;

        if let Some(hostname) = &self.def.hostname {
            fs::write(root.join("etc/hostname"), format!("{hostname}\n"))?;
        }
        // Machine identity is made on the machine, never shipped in a slot.
        for file in ["machine-id", "passwd-", "group-", "shadow-"] {
            let path = root.join("etc").join(file);
            if path.symlink_metadata().is_ok() {
                fs::remove_file(&path)?;
            }
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
        let slot_mib = self.slot_mib.unwrap_or(vm.slot_mib);
        let layout = Layout {
            slot_mib,
            data_mib: vm.data_mib,
        };

        let loader = work.join("esp");
        self.make_loader(&loader, root, &vm.kernel, &vm.cmdline)?;
        self.make_slot(root, &update, slot_mib)?;
        self.shrink(&update)?;
        // Slot A gets a filesystem of its own rather than a copy of the
        // update image: the kernel finds its root by filesystem UUID, so no
        // two filesystems a machine can see may share one.
        let slot_a = work.join("slot-a.ext4");
        self.make_slot(root, &slot_a, slot_mib)?;
        let data = work.join("data.ext4");
        self.make_data(&data, vm.data_mib)?;
        let esp = work.join("esp.img");
        self.make_esp(&loader, &esp)?;

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
            .step("copy the EFI system partition, slot A and the data partition into the disk");
        if !self.runner.dry_run {
            copy_sparse(&esp, &disk, layout.esp_start_mib() * MIB)?;
            copy_sparse(&slot_a, &disk, layout.slot_start_mib(0) * MIB)?;
            copy_sparse(&data, &disk, layout.data_start_mib() * MIB)?;
        }
        // The update image is small; the disk is the size of both slots.
        self.gzip(&update)?;
        if self.compress {
            self.gzip(&disk)?;
        }
        println!("vm disk: {}", disk.display());
        println!("update image: {}", update.display());
        Ok(())
    }

    /// Shrinks the update image to its file system, so it fits any slot at
    /// least that big; `edel update install` grows it again (M1.7).
    fn shrink(&self, image: &Path) -> Result<()> {
        self.runner
            .run(Command::new("e2fsck").arg("-fp").arg(image))?;
        self.runner
            .run(Command::new("resize2fs").arg("-M").arg(image))?;
        self.runner.step(&format!(
            "cut {} to the size of its file system",
            image.display()
        ));
        if self.runner.dry_run {
            return Ok(());
        }
        let mut superblock = vec![0u8; 2048];
        File::open(image)?.read_exact(&mut superblock)?;
        let size = ext4_size(&superblock)?;
        OpenOptions::new().write(true).open(image)?.set_len(size)?;
        Ok(())
    }

    /// Writes `FILE.gz` next to `file`, the form releases publish.
    fn gzip(&self, file: &Path) -> Result<()> {
        let gz = PathBuf::from(format!("{}.gz", file.display()));
        self.runner
            .step(&format!("compress {} to {}", file.display(), gz.display()));
        if self.runner.dry_run {
            return Ok(());
        }
        let mut encoder = GzEncoder::new(
            File::create(&gz).with_context(|| format!("creating {}", gz.display()))?,
            Compression::default(),
        );
        std::io::copy(&mut File::open(file)?, &mut encoder)?;
        encoder.finish()?;
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

    /// An empty ext4 data partition; it grows to the end of the disk on
    /// first boot (`edel boot mount-data`).
    fn make_data(&self, image: &Path, size_mib: u64) -> Result<()> {
        self.runner.step(&format!(
            "create a sparse {size_mib} MiB data partition image {}",
            image.display()
        ));
        if !self.runner.dry_run {
            File::create(image)?.set_len(size_mib * MIB)?;
        }
        self.runner.run(
            Command::new("mkfs.ext4")
                .args(["-q", "-F", "-L", boot::DATA_LABEL])
                .arg(image),
        )
    }

    /// Builds the EFI system partition: GRUB as the removable-disk boot
    /// file, plus its config and environment block.
    /// Builds the boot loader in `dir` (GRUB as the removable-disk boot
    /// file, its config, its environment block and `loader.toml`) and puts
    /// a copy into the slot's `/usr/lib/edel/boot/`, so an update carries
    /// its loader and installs it once the slot is confirmed (M1.8).
    fn make_loader(&self, dir: &Path, root: &Path, kernel: &str, cmdline: &str) -> Result<()> {
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
            let microcode: Vec<&str> = boot::MICROCODE
                .iter()
                .copied()
                .filter(|f| root.join("boot").join(f).is_file())
                .collect();
            let modules = self.def.modules.join(",");
            let mut text = boot::grub_cfg(kernel, &modules, cmdline, &microcode);
            if let Some(tag) = &self.loader_tag {
                // A test tag makes a loader that differs from the last one.
                text += &format!("\n# loader tag: {tag}\n");
            }
            fs::write(&cfg, text)?;
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
        let slot_dir = root.join(crate::loader::SLOT_DIR.trim_start_matches('/'));
        self.runner.step(&format!(
            "write loader.toml and copy the loader to {}",
            crate::loader::SLOT_DIR
        ));
        if self.runner.dry_run {
            return Ok(());
        }
        let version = crate::loader::loader_toml(&fs::read(&efi)?, &fs::read(&cfg)?);
        fs::write(dir.join("loader.toml"), &version)?;
        fs::create_dir_all(&slot_dir)?;
        for file in [efi_name, "grub.cfg", "loader.toml"] {
            fs::copy(dir.join(file), slot_dir.join(file))?;
        }
        // The slot says which loader it carries (M3.1).
        let os_release = root.join("usr/lib/os-release");
        let loader = crate::release::os_release_value(&version, "version").unwrap_or_default();
        let text =
            fs::read_to_string(&os_release)? + &format!("EDEL_LOADER_VERSION=\"{loader}\"\n");
        fs::write(&os_release, text)?;
        Ok(())
    }

    /// Builds the EFI system partition image from the loader in `dir`.
    fn make_esp(&self, dir: &Path, esp: &Path) -> Result<()> {
        let (_, efi_name) = boot::efi_target(&self.def.arch)?;
        let efi = dir.join(efi_name);
        let cfg = dir.join("grub.cfg");
        let env = dir.join("grubenv");
        let version = dir.join("loader.toml");

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
                .arg(&version)
                .arg(on_esp(&format!("{}/", boot::GRUB_PREFIX))),
        )
    }
}

/// The size in bytes of the ext4 file system whose first 2048 bytes are
/// `head`: block count times block size, from the superblock at 1024.
fn ext4_size(head: &[u8]) -> Result<u64> {
    let sb = head
        .get(1024..1024 + 0x160)
        .context("too short for an ext4 superblock")?;
    let u32_at = |off: usize| {
        u64::from(u32::from_le_bytes([
            sb[off],
            sb[off + 1],
            sb[off + 2],
            sb[off + 3],
        ]))
    };
    if sb[0x38] != 0x53 || sb[0x39] != 0xEF {
        bail!("not an ext4 file system");
    }
    let blocks = u32_at(0x04) | (u32_at(0x150) << 32);
    Ok(blocks * (1024 << u32_at(0x18)))
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

/// Whether an apk cache already holds a package index.
fn has_index(cache: &Path) -> bool {
    fs::read_dir(cache).is_ok_and(|entries| {
        entries
            .flatten()
            .any(|e| e.file_name().to_string_lossy().starts_with("APKINDEX."))
    })
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
    fn reads_the_ext4_size_from_the_superblock() {
        let mut head = vec![0u8; 2048];
        head[1024 + 0x04..1024 + 0x08].copy_from_slice(&40_000u32.to_le_bytes());
        head[1024 + 0x18..1024 + 0x1c].copy_from_slice(&2u32.to_le_bytes());
        head[1024 + 0x38] = 0x53;
        head[1024 + 0x39] = 0xEF;
        assert_eq!(ext4_size(&head).unwrap(), 40_000 * 4096);
        head[1024 + 0x38] = 0;
        assert!(ext4_size(&head).is_err());
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

    #[test]
    fn writes_the_version_into_os_release() {
        let os = "NAME=\"Edel OS\"\nVERSION_ID=0.1\nVERSION=\"0.1 (development)\"\nPRETTY_NAME=\"Edel OS 0.1 (development)\"\n";
        let text = with_version(os, Some("2026.10.57"), "preview", "x86_64");
        assert!(text.contains("\nVERSION_ID=2026.10.57\n"));
        assert!(text.contains("\nVERSION=\"2026.10.57 (preview)\"\n"));
        assert!(text.contains("\nPRETTY_NAME=\"Edel OS 2026.10.57\"\n"));
        assert!(
            text.ends_with(
                "EDEL_CHANNEL=\"preview\"\nEDEL_ARCH=\"x86_64\"\nEDEL_PLATFORM_LEVEL=0\n"
            )
        );
        let dev = with_version(os, None, "dev", "x86_64");
        assert!(dev.starts_with(os), "{dev}");
    }

    #[test]
    fn versions_are_numbers_joined_by_dots() {
        assert!(is_version("2026.10.57") && is_version("0.1.1"));
        assert!(
            !is_version("")
                && !is_version("2026.10.")
                && !is_version("v2026.10.1")
                && !is_version("2026.10.1-rc1")
        );
        use std::cmp::Ordering;
        assert_eq!(
            crate::release::compare_versions("2026.10.57.1", "2026.10.57"),
            Ordering::Greater
        );
        assert_eq!(
            crate::release::compare_versions("2026.09.99", "2026.10.1"),
            Ordering::Less
        );
    }
}
