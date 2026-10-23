//! `edel image build`: turns an image definition into a container tarball or
//! a bootable A/B VM disk, using Alpine's own `apk` to install packages.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::Read;

use std::os::unix::fs::{FileExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use edel::features;

use crate::addons;
use crate::boot::{self, Layout};
use crate::def::{ImageDef, Listed, Variant};
use crate::run::{Runner, ensure_nothing_mounted_under};

const MIB: u64 = 1024 * 1024;

/// Where Alpine keeps its trusted package signing keys. The build host must
/// be Alpine (or the alpine container) so these are present and trusted.
const HOST_KEYS: &str = "/etc/apk/keys";

pub struct Build<'a> {
    pub def: &'a ImageDef,
    /// Directory holding the definition file; `[release] public_keys` are
    /// relative to it.
    pub def_dir: PathBuf,
    /// More directories copied over the image after the definition's own,
    /// for test images. Not part of the definition, so a release image can
    /// never pick them up by accident.
    pub extra_files: Vec<PathBuf>,
    /// More add-ons to build, by name, beside the definition's; for test
    /// images (M7.2a).
    pub extra_addons: Vec<Listed>,
    /// The merged settings file baked in as the slot's own
    /// (M7.3), checked already; none leaves the slot's own, if any.
    pub settings: Option<String>,
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
    /// Where the default apps come from (M6.2): a `.flatpakrepo` file's
    /// address or a repository's.
    pub apps_from: String,
    /// Where machines update the default apps from, when not `apps_from`.
    pub apps_url: Option<String>,
    /// The default apps the data partition gets (M6.2): the definition's,
    /// or a test's own.
    pub apps: Vec<String>,
    /// The default apps a release dropped (M6.2): the definition's, or a
    /// test's own.
    pub apps_dropped: Vec<String>,
    /// Whether to write the disk's `.gz` copy releases publish; test
    /// images nobody downloads skip it (the slot's `.gz` is always made)
    pub compress: bool,
    pub out: PathBuf,
    pub runner: Runner,
}

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
    let key = edel::platform::OS_RELEASE_KEY;
    let level = edel::platform::LEVEL;
    out.push_str(&format!(
        "EDEL_CHANNEL=\"{channel}\"\nEDEL_ARCH=\"{arch}\"\n{key}={level}\n"
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
        // After the files, so `--settings` wins over a file of the same name.
        self.install_settings(&root)?;
        self.install_features(&root)?;
        self.install_places(&root)?;
        self.bake_font_cache(&root)?;
        if def.variant == Variant::Vm {
            self.install_edel(&root)?;
            self.install_keys(&root)?;
            self.install_programs(&root)?;
            self.install_apps_conf(&root)?;
        }
        self.enable_services(&root)?;
        self.configure(&root)?;
        // Last, once every file is in: what changes /usr alone then
        // changes all of the system (M7.1).
        self.merge_usr(&root)?;

        // A VM image builds its add-ons before its disk, as the disk
        // carries some of them (M8.17c); a container has none.
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
        // Before the initramfs is made, so it holds only what stays.
        self.keep_files(root)?;
        if let Some(vm) = &self.def.vm {
            self.make_initramfs(root, &vm.kernel)?;
        }
        self.record_packages(root)
    }

    /// Takes out what the features' `[[keep]]` lists do not keep (M5.30):
    /// under each folder, every file and link no listed pattern keeps,
    /// and every version of a versioned file but the newest its driver
    /// loads. Prints what stayed and what went, in MiB.
    fn keep_files(&self, root: &Path) -> Result<()> {
        for keep in &self.def.keep {
            let under = keep.under.trim_matches('/');
            self.runner.step(&format!(
                "keep under /{under} only the files the features list"
            ));
            if self.runner.dry_run {
                continue;
            }
            let dir = root.join(under);
            let entries = keep_entries(&dir)
                .with_context(|| format!("listing /{under} to keep its listed files"))?;
            let declared = match &keep.newest_by {
                Some(module) => declared_firmware(root, module)?,
                None => Vec::new(),
            };
            let plan = edel::keep::plan(&entries, keep, &declared)?;
            for path in &plan.removed {
                let at = dir.join(path);
                fs::remove_file(&at).with_context(|| format!("removing {}", at.display()))?;
            }
            remove_empty_dirs(&dir)?;
            println!(
                "kept {} files under /{under} ({:.1} MiB), took out {} ({:.1} MiB)",
                plan.kept.len(),
                mib(plan.kept_bytes),
                plan.removed.len(),
                mib(plan.removed_bytes)
            );
        }
        Ok(())
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
        // Alpine's init, changed to mount a root GRUB names by its partition
        // (M1.12), written beside our other files and handed to mkinitfs.
        let alpine = root.join("usr/share/mkinitfs/initramfs-init");
        let text = fs::read_to_string(&alpine)
            .with_context(|| format!("reading Alpine's {}", alpine.display()))?;
        let ours = root.join(edel::places::INITRAMFS_INIT.trim_start_matches('/'));
        fs::create_dir_all(ours.parent().unwrap_or(root))?;
        fs::write(&ours, partuuid_init(&text)?)?;
        self.runner.run(Command::new("chroot").arg(root).args([
            "mkinitfs",
            "-i",
            edel::places::INITRAMFS_INIT,
            "-o",
            &format!("/boot/initramfs-{flavor}"),
            kernel,
        ]))
    }

    /// Writes the installed packages, one `name-version` a line, into the
    /// image (`/usr/share/edel/packages`) and beside it
    /// (`out/<image>.packages`), so every release says what is inside it,
    /// and writes the bill of materials beside it
    /// (`out/<image>.cdx.json`, M8.12). Both come from apk's own record of
    /// the root, read once.
    fn record_packages(&self, root: &Path) -> Result<()> {
        let stem = self.def.stem();
        let beside = self.out.join(format!("{stem}.packages"));
        let bom = self.out.join(format!("{stem}.cdx.json"));
        self.runner.step(&format!(
            "record the installed packages in {} and {} and their bill of materials in {}, and check them against platform level {}",
            edel::places::PACKAGES_FILE,
            beside.display(),
            bom.display(),
            edel::platform::LEVEL
        ));
        if self.runner.dry_run {
            return Ok(());
        }
        let db = root.join(crate::bom::INSTALLED_DB);
        let text = fs::read_to_string(&db).with_context(|| {
            format!(
                "reading apk's record of installed packages {}",
                db.display()
            )
        })?;
        let packages = crate::bom::parse_installed(&text);
        let features: Vec<&str> = self.def.features.iter().map(|f| f.name.as_str()).collect();
        self.check_platform(&packages, &features)?;
        let mut lines: Vec<String> = packages
            .iter()
            .map(|p| format!("{}-{}", p.name, p.version))
            .collect();
        lines.sort_unstable();
        let list: String = lines.iter().map(|l| format!("{l}\n")).collect();
        let inside = root.join(edel::places::PACKAGES_FILE.trim_start_matches('/'));
        fs::create_dir_all(inside.parent().unwrap_or(root))?;
        fs::write(&inside, &list)?;
        fs::write(&beside, &list)?;
        let distro = format!("alpine-{}", self.def.alpine.branch.trim_start_matches('v'));
        let version = self.version.as_deref().unwrap_or("");
        let document = crate::bom::cyclonedx(&stem, version, &distro, &packages);
        fs::write(&bom, document)
            .with_context(|| format!("writing the bill of materials {}", bom.display()))?;
        Ok(())
    }

    /// Refuses an image that falls below the platform level it promises
    /// (ADR-005, M8.8): its kernel and the app platform's packages, read
    /// from the level's file, against the packages apk recorded.
    fn check_platform(&self, packages: &[crate::bom::Package], features: &[&str]) -> Result<()> {
        let number = edel::platform::LEVEL;
        let Some(level) = edel::platform::level(number) else {
            bail!("platform level {number} is not built in; see platform/level-{number}.toml");
        };
        let pairs: Vec<(&str, &str)> = packages
            .iter()
            .map(|p| (p.name.as_str(), p.version.as_str()))
            .collect();
        let kernel = self
            .def
            .vm
            .as_ref()
            .map(|vm| format!("linux-{}", vm.kernel));
        let short = edel::platform::shortfalls(number, &level, &pairs, kernel.as_deref(), features);
        if !short.is_empty() {
            bail!(
                "the image falls below platform level {number} (platform/level-{number}.toml): {}",
                short.join("; ")
            );
        }
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
            self.copy_tree(&src, root)?;
        }
        Ok(())
    }

    /// Copies the files of one folder (a feature's `features/NAME/`, or a
    /// test's `--files`) over `root`.
    fn copy_tree(&self, src: &Path, root: &Path) -> Result<()> {
        if !self.runner.dry_run && !src.is_dir() {
            bail!("files directory {} does not exist", src.display());
        }
        // "src/." copies the directory's contents, including dotfiles.
        // Plain -R (not -a) so copies are owned by root, not by whoever
        // checked out the repository; modes such as +x are kept.
        self.runner
            .run(Command::new("cp").arg("-R").arg(src.join(".")).arg(root))
    }

    /// Writes the merged settings file as the slot's own (M7.3), which
    /// seeds a machine's first boot when no stick or partition holds one.
    fn install_settings(&self, root: &Path) -> Result<()> {
        let Some(text) = &self.settings else {
            return Ok(());
        };
        // The path is edel::settings's to name, as it writes the file.
        self.runner
            .step("write the fleet's settings file as the image's own, with edel::settings");
        if self.runner.dry_run {
            return Ok(());
        }
        edel::settings::write_slot_copy(root, text)
    }

    /// Copies the running `edel` into the image as `/usr/bin/edel`, the
    /// updater of a bootable image. CI builds it inside the Alpine release the base feature names, so it
    /// links against the image's musl.
    fn install_edel(&self, root: &Path) -> Result<()> {
        self.runner.step("copy this edel binary to /usr/bin/edel");
        self.runner.step(&format!(
            "write edel's tab completion, from its own command table, to {}",
            crate::completion::PATH
        ));
        if self.runner.dry_run {
            return Ok(());
        }
        // The completion (M5.26): bash reads it where the completion
        // feature brings bash; elsewhere it is a few kilobytes unused.
        let completion = root.join(crate::completion::PATH.trim_start_matches('/'));
        fs::create_dir_all(completion.parent().unwrap_or(root))?;
        fs::write(&completion, crate::completion::bash(&crate::cli_command()))?;
        // The data partition's mount point; / may be read-only at boot.
        fs::create_dir_all(root.join("data"))?;
        let exe = std::env::current_exe().context("finding the running edel binary")?;
        let dest = root.join(edel::places::EDEL_BIN.trim_start_matches('/'));
        fs::create_dir_all(root.join("usr/bin"))?;
        fs::copy(&exe, &dest).with_context(|| format!("copying {}", exe.display()))?;
        fs::set_permissions(&dest, fs::Permissions::from_mode(0o755))?;
        Ok(())
    }

    /// Copies the programs the features name (`programs`) from beside the
    /// running `edel`, where `cargo build --release --workspace` leaves
    /// them, to `/usr/bin`.
    fn install_programs(&self, root: &Path) -> Result<()> {
        self.copy_programs(&self.def.programs, root)
    }

    /// Copies `programs` from beside the running `edel` to `root`'s
    /// `/usr/bin`: the image's, or an add-on's (M8.17c).
    fn copy_programs(&self, programs: &[String], root: &Path) -> Result<()> {
        if programs.is_empty() {
            return Ok(());
        }
        let exe = std::env::current_exe().context("finding the running edel binary")?;
        let dir = exe.parent().context("finding the directory of edel")?;
        for program in programs {
            let src = dir.join(program);
            self.runner
                .step(&format!("copy {} to /usr/bin/{program}", src.display()));
            if self.runner.dry_run {
                continue;
            }
            if !src.is_file() {
                bail!(
                    "{} is missing; build it beside edel with cargo build --release --locked --workspace",
                    src.display()
                );
            }
            let dest = root.join("usr/bin").join(program);
            fs::copy(&src, &dest).with_context(|| format!("copying {}", src.display()))?;
            fs::set_permissions(&dest, fs::Permissions::from_mode(0o755))?;
        }
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
                "copy the public key {} to {}/",
                key.display(),
                edel::places::KEYS_DIR
            ));
        }
        if self.runner.dry_run {
            return Ok(());
        }
        let dir = root.join(edel::places::KEYS_DIR.trim_start_matches('/'));
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
    /// shipped off (M4.0), and the hostname that `edel settings apply` goes
    /// back to when the settings file has none; none for containers.
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
        if !self.def.health_screen.is_empty() {
            keys.push_str(&format!(
                "EDEL_HEALTH_SCREEN=\"{}\"\n",
                self.def.health_screen.join(" ")
            ));
        }
        if !self.def.off.is_empty() {
            keys.push_str(&format!(
                "EDEL_SERVICES_OFF=\"{}\"\n",
                self.def.off.join(" ")
            ));
        }
        if !self.def.headless.is_empty() {
            keys.push_str(&format!(
                "EDEL_SERVICES_HEADLESS=\"{}\"\n",
                self.def.headless.join(" ")
            ));
        }
        // The add-ons the disk carries are the machine's while its settings
        // file names none of its own (M8.17c).
        let carried = self.carried();
        if !carried.is_empty() {
            keys.push_str(&format!(
                "{}=\"{}\"\n",
                edel::addon_state::DEFAULT_KEY,
                carried.join(" ")
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
        // The add-ons built beside it, so the machine knows what it can
        // add without the network (M7.2b).
        let offered: Vec<&Listed> = self.def.addons.iter().chain(&self.extra_addons).collect();
        if !offered.is_empty() {
            let dir = root.join(edel::places::OFFERED_ADDONS_DIR.trim_start_matches('/'));
            fs::create_dir_all(&dir)?;
            for addon in offered {
                fs::write(dir.join(format!("{}.toml", addon.name)), &addon.text)?;
            }
        }
        Ok(())
    }

    /// Writes Edel OS's places as shell variables to `places.sh`, which
    /// the slot's own scripts source, so a path is written once, in
    /// `edel::places` (ADR-010, M5.27), and links `places::ADDON_CONF_DIR`
    /// to `/etc/conf.d` (M8.17c).
    fn install_places(&self, root: &Path) -> Result<()> {
        self.runner.step(&format!(
            "write Edel OS's places to {}",
            edel::places::PLACES_SH
        ));
        if self.runner.dry_run {
            return Ok(());
        }
        let file = root.join(edel::places::PLACES_SH.trim_start_matches('/'));
        fs::create_dir_all(file.parent().unwrap_or(root))?;
        fs::write(
            file,
            edel::places::shell_file("Edel OS's places, for the slot's own scripts (M5.27)"),
        )?;
        let conf = root.join(edel::places::ADDON_CONF_DIR.trim_start_matches('/'));
        if conf.symlink_metadata().is_err() {
            fs::create_dir_all(conf.parent().unwrap_or(root))?;
            std::os::unix::fs::symlink(edel::base::SERVICE_CONF_DIR, conf)?;
        }
        Ok(())
    }

    /// Bakes the fonts' cache into the slot (M6.2a): `fc-cache` writes it
    /// at build time, in the folder `places::FONT_CACHE_DIR` that Flatpak
    /// hands every app, so no app builds its own on its first start. The
    /// data partition's old cache (`places::VAR_FONT_CACHE`) is left out,
    /// as it predates the fonts this image has.
    fn bake_font_cache(&self, root: &Path) -> Result<()> {
        self.runner.step(&format!(
            "bake the fonts' cache into {}, so no app builds its own (M6.2a)",
            edel::places::FONT_CACHE_DIR
        ));
        if self.runner.dry_run {
            return Ok(());
        }
        // An image without fonts (the container, a server) has no cache
        // to bake, even when fontconfig came in with another package.
        if !root.join("usr/bin/fc-cache").is_file() || !has_file(&root.join("usr/share/fonts"))? {
            return Ok(());
        }
        let conf = root.join(edel::places::FONT_CACHE_CONF.trim_start_matches('/'));
        fs::create_dir_all(conf.parent().unwrap_or(root))?;
        fs::write(conf, font_cache_conf(edel::places::FONT_CACHE_DIR))?;
        let var = root.join(edel::places::VAR_FONT_CACHE.trim_start_matches('/'));
        if var.is_dir() {
            fs::remove_dir_all(&var)?;
        }
        let dir = root.join(edel::places::FONT_CACHE_DIR.trim_start_matches('/'));
        fs::create_dir_all(&dir)?;
        // fontconfig keeps each folder's time to the nanosecond and drops a
        // cache whose folder differs, and `mkfs.ext4 -d` keeps whole seconds
        // only, so the folders are rounded before the cache is made.
        whole_seconds(&root.join("usr/share/fonts"))?;
        let mut chroot = Command::new("chroot");
        chroot
            .arg(root)
            .args(["fc-cache", "--system-only", "--really-force"])
            // fc-cache given SOURCE_DATE_EPOCH (M8.6) writes no cache for a
            // folder newer than that date (1 cache file against 5 in a test
            // on 2026-10-17), so it runs without it.
            .env_remove("SOURCE_DATE_EPOCH");
        self.runner.run(&mut chroot)?;
        // fc-cache may still write to the old folder: move its files over,
        // so that folder never exists in the slot.
        if var.is_dir() {
            for entry in fs::read_dir(&var)? {
                let entry = entry?;
                fs::rename(entry.path(), dir.join(entry.file_name()))?;
            }
            fs::remove_dir_all(&var)?;
        }
        let wrote = fs::read_dir(&dir)?
            .filter_map(Result::ok)
            .any(|entry| entry.file_name().to_string_lossy().contains(".cache-"));
        if !wrote {
            bail!(
                "fc-cache wrote no cache into {}; the fonts' cache is baked so no app builds its own",
                dir.display()
            );
        }
        Ok(())
    }

    /// Tells Flatpak where the default apps' installation lies on the
    /// data partition (M6.2), when the image has default apps.
    fn install_apps_conf(&self, root: &Path) -> Result<()> {
        if self.apps.is_empty() && self.apps_dropped.is_empty() {
            return Ok(());
        }
        let list = edel::apps::DefaultApps {
            flatpak: self.apps.clone(),
            flatpak_dropped: self.apps_dropped.clone(),
        };
        self.runner.step(&format!(
            "record the default apps in {}",
            edel::places::DEFAULT_APPS_LIST
        ));
        if !self.runner.dry_run {
            let file = root.join(edel::places::DEFAULT_APPS_LIST.trim_start_matches('/'));
            fs::create_dir_all(file.parent().unwrap_or(root))?;
            fs::write(file, list.text())?;
        }
        let file =
            Path::new(edel::places::FLATPAK_INSTALLATIONS).join(edel::apps::installation_file());
        self.runner.step(&format!(
            "tell Flatpak of the default apps at {} in {}",
            edel::places::DEFAULT_APPS,
            file.display()
        ));
        let file = root.join(file.strip_prefix("/")?);
        if !self.runner.dry_run {
            fs::create_dir_all(file.parent().unwrap_or(root))?;
            fs::write(
                file,
                edel::apps::installation_conf(edel::places::DEFAULT_APPS),
            )?;
        }
        self.fetch_flathub_remote(root)
    }

    /// Fetches Flathub's remote file (M6.4) into the slot's share folder, so
    /// each person's session adds Flathub from it offline on a first start
    /// and the store lists apps without the network. The file must hold a
    /// `GPGKey`, or the store could not check what it installs.
    fn fetch_flathub_remote(&self, root: &Path) -> Result<()> {
        let url = edel::apps::FLATHUB_REMOTE;
        let target = edel::places::FLATHUB_REPO_FILE;
        self.runner.step(&format!(
            "fetch Flathub's remote file from {url} into {target}"
        ));
        let file = root.join(target.trim_start_matches('/'));
        if !self.runner.dry_run {
            fs::create_dir_all(file.parent().unwrap_or(root))?;
        }
        self.runner
            .run(Command::new("wget").args(["-q", "-O"]).arg(&file).arg(url))?;
        if self.runner.dry_run {
            return Ok(());
        }
        let text = fs::read_to_string(&file)
            .with_context(|| format!("reading the Flathub remote file {}", file.display()))?;
        if !is_flatpakrepo(&text) {
            bail!(
                "Flathub's remote file from {url} holds no GPGKey; the store could not check what it installs"
            );
        }
        Ok(())
    }

    /// Installs the default apps (M6.2) into what becomes the data
    /// partition, `work/data` below the place the partition is mounted
    /// at, from `apps_from`, with Flatpak's own installer told of a
    /// build-time installation there; returns that folder, or None when
    /// the image has no default apps.
    fn fill_apps(&self, work: &Path) -> Result<Option<PathBuf>> {
        if self.apps.is_empty() {
            return Ok(None);
        }
        let data = work.join("data");
        let below = edel::places::DEFAULT_APPS
            .strip_prefix(edel::places::DATA_MOUNT)
            .unwrap_or(edel::places::DEFAULT_APPS)
            .trim_start_matches('/');
        let apps = data.join(below);
        let config = work.join("flatpak-config");
        self.runner.step(&format!(
            "install the default apps {} from {} into {}",
            self.apps.join(", "),
            self.apps_from,
            apps.display()
        ));
        if !self.runner.dry_run {
            fs::create_dir_all(&apps)?;
            let installations = config.join("installations.d");
            fs::create_dir_all(&installations)?;
            fs::write(
                installations.join(edel::apps::installation_file()),
                edel::apps::installation_conf(&apps.to_string_lossy()),
            )?;
        }
        let flatpak = || {
            let mut command = Command::new("flatpak");
            command
                .env("FLATPAK_CONFIG_DIR", &config)
                .arg(format!("--installation={}", edel::apps::INSTALLATION));
            command
        };
        let mut add = flatpak();
        add.args(["remote-add", "--if-not-exists"]);
        add.args(edel::apps::remote_add_args(&self.apps_from));
        add.arg(edel::apps::APPS_REMOTE).arg(&self.apps_from);
        self.runner.run(&mut add)?;
        let mut install = flatpak();
        install
            .args(["install", "--noninteractive", "-y", edel::apps::APPS_REMOTE])
            .args(&self.apps);
        self.runner.run(&mut install)?;
        // Each runtime that asks for the Mesa driver must have it now: an
        // app without it draws in software (M6.2a).
        if !self.runner.dry_run {
            let missing = gl_missing(&apps)?;
            if !missing.is_empty() {
                bail!(
                    "the default apps' runtimes {} have no Mesa driver, so their apps would draw in software; flatpak install should have installed it with them (M6.2a)",
                    missing.join(", ")
                );
            }
        }
        if let Some(url) = &self.apps_url {
            let mut point = flatpak();
            point
                .args(["remote-modify", &format!("--url={url}")])
                .arg(edel::apps::APPS_REMOTE);
            self.runner.run(&mut point)?;
        }
        Ok(Some(data))
    }

    /// Makes the root `/usr`-merged with Alpine's `merge-usr` (M7.1):
    /// `/bin`, `/sbin` and `/lib` become links into `/usr`, so busybox,
    /// the C library and apk's database live there, and developer mode's
    /// one layer over `/usr` takes `apk add` (ADR-007). The tool runs on
    /// the build host, so the image carries none of it.
    fn merge_usr(&self, root: &Path) -> Result<()> {
        self.runner
            .run(Command::new("merge-usr").arg("--root").arg(root))
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
            "write version {}, channel {} and platform level {} into /usr/lib/os-release",
            self.version.as_deref().unwrap_or("of the overlay"),
            self.channel,
            edel::platform::LEVEL
        ));
        self.runner.step("lock the root password");
        if let Some(hostname) = &self.def.hostname {
            self.runner.step(&format!("set the hostname to {hostname}"));
        }
        if self.runner.dry_run {
            return Ok(());
        }

        if !root.join("usr/lib/os-release").is_file() {
            bail!("the image has no /usr/lib/os-release; it comes from features/base/");
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
        let mut shadow = lock_root(&shadow)?;
        if let Some(seconds) =
            source_date_epoch(std::env::var("SOURCE_DATE_EPOCH").ok().as_deref())?
        {
            shadow = date_shadow(&shadow, seconds);
        }
        fs::write(&shadow_path, shadow)?;

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
        // apk's log of this build holds the build's path and clock and is read
        // by nothing on a machine; left in, no two builds would match (M8.6).
        let apk_log = root.join(addons::APK_LOG);
        if apk_log.symlink_metadata().is_ok() {
            fs::remove_file(&apk_log)?;
        }
        Ok(())
    }

    fn pack_container(&self, root: &Path) -> Result<()> {
        let tarball = self.out.join(format!("{}.tar.gz", self.def.stem()));
        let epoch = source_date_epoch(std::env::var("SOURCE_DATE_EPOCH").ok().as_deref())?;
        self.runner.run(
            Command::new("tar")
                .args(container_tar_args(epoch))
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
        let loader = work.join("esp");
        self.make_loader(&loader, root, &vm.kernel, &vm.cmdline)?;
        self.make_slot(root, &update, slot_mib, "edel")?;
        self.shrink(&update)?;
        self.build_addons(work, root)?;
        let data_root = self.fill_apps(work)?;
        let data_root = self.carry_addons(work, root, data_root)?;
        let data_mib = match &data_root {
            Some(dir) if !self.runner.dry_run => data_mib_for(vm.data_mib, tree_bytes(dir)?),
            _ => vm.data_mib,
        };
        let layout = Layout { slot_mib, data_mib };

        // Slot A is the update image itself, byte for byte, as every slot
        // stays what was signed (M1.12): GRUB finds a slot by its
        // partition, so two slots may hold one file system UUID.
        let data = work.join("data.ext4");
        self.make_data(&data, data_mib, data_root.as_deref())?;
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
            copy_sparse(&update, &disk, layout.slot_start_mib(0) * MIB)?;
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

    /// The add-ons of this build the disk carries ([`Vm::carry`] that were
    /// built), in order (M8.17c).
    fn carried(&self) -> Vec<String> {
        let built: Vec<&Listed> = self.def.addons.iter().chain(&self.extra_addons).collect();
        self.def
            .vm
            .iter()
            .flat_map(|vm| vm.carry.iter())
            .filter(|name| built.iter().any(|l| &l.name == *name))
            .cloned()
            .collect()
    }

    /// Puts the add-ons the disk carries onto its data partition (M8.17c),
    /// in the running version's folder, where a machine started from the
    /// disk mounts them at its first boot: the stick's desktop. `data` is
    /// the data partition's tree so far (the default apps), if any; the
    /// tree is returned, made when there was none.
    fn carry_addons(
        &self,
        work: &Path,
        root: &Path,
        data: Option<PathBuf>,
    ) -> Result<Option<PathBuf>> {
        let carried = self.carried();
        if carried.is_empty() {
            return Ok(data);
        }
        let tree = data.unwrap_or_else(|| work.join("data"));
        let os_release = if self.runner.dry_run {
            String::new()
        } else {
            fs::read_to_string(root.join("usr/lib/os-release"))?
        };
        let version =
            crate::release::os_release_value(&os_release, "VERSION_ID").unwrap_or_default();
        let dir = tree.join(
            crate::addons::version_dir(&version)
                .strip_prefix(edel::places::DATA_MOUNT)
                .unwrap_or(Path::new("")),
        );
        for name in carried {
            let file = self.out.join(format!("{}.{name}.ext4", self.def.stem()));
            self.runner.step(&format!(
                "carry add-on {name} on the data partition, in {}",
                dir.display()
            ));
            if !self.runner.dry_run {
                fs::create_dir_all(&dir)?;
                fs::copy(&file, dir.join(format!("{name}.ext4")))
                    .with_context(|| format!("copying {}", file.display()))?;
            }
        }
        Ok(Some(tree))
    }

    /// Shrinks the update image to its file system, so it fits any slot at
    /// least that big; a slot keeps it that size, as a read-only root
    /// needs no room, so its bytes stay those signed (M1.12).
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

    /// Writes `FILE.gz` next to `file`, the form releases publish, with
    /// pigz: one ordinary gzip stream, made on every core, so the desktop
    /// image's two files take seconds instead of most of a minute each.
    /// `-n` leaves out the file's name and time, so the same file always
    /// gives the same bytes.
    fn gzip(&self, file: &Path) -> Result<()> {
        self.runner
            .run(Command::new("pigz").args(["-k", "-f", "-n"]).arg(file))
    }

    /// Makes the file system `image` of `root`'s files, labelled `label`:
    /// the slot's is `edel`, an add-on's its name (M7.2a).
    fn make_slot(&self, root: &Path, image: &Path, size_mib: u64, label: &str) -> Result<()> {
        self.runner.step(&format!(
            "create a sparse {size_mib} MiB slot image {}",
            image.display()
        ));
        if !self.runner.dry_run {
            File::create(image)?.set_len(size_mib * MIB)?;
        }
        // No journal: a slot is only ever mounted read-only, so it has
        // nothing to replay, and Linux writes its checksum settings into a
        // journal's superblock at every mount, read-only ones too, which
        // left a booted slot 6 bytes off its signed image (M1.12).
        self.runner.run(
            Command::new("mkfs.ext4")
                .args(["-q", "-F", "-O", "^has_journal", "-L"])
                .arg(label)
                .arg("-d")
                .arg(root)
                .arg(image),
        )
    }

    /// The ext4 data partition, empty or holding `root`'s files (the
    /// default apps, M6.2); it grows to the end of the disk on first boot
    /// (`edel boot mount-data`).
    fn make_data(&self, image: &Path, size_mib: u64, root: Option<&Path>) -> Result<()> {
        self.runner.step(&format!(
            "create a sparse {size_mib} MiB data partition image {}",
            image.display()
        ));
        if !self.runner.dry_run {
            File::create(image)?.set_len(size_mib * MIB)?;
        }
        let mut mkfs = Command::new("mkfs.ext4");
        mkfs.args(["-q", "-F", "-L", boot::DATA_LABEL]);
        if let Some(root) = root {
            mkfs.arg("-d").arg(root);
        }
        self.runner.run(mkfs.arg(image))
    }

    /// Builds the boot loader in `dir` (GRUB as the removable-disk boot
    /// file, its config, its environment block and `loader.toml`) and puts
    /// a copy into the slot's `/usr/lib/edel/boot/`, so an update carries
    /// its loader and installs it once the slot is confirmed (M1.8), with
    /// the slot's own [`boot::SLOT_CFG`], which GRUB starts it by (M6.17).
    fn make_loader(&self, dir: &Path, root: &Path, kernel: &str, cmdline: &str) -> Result<()> {
        let (target, efi_name) = boot::efi_target(&self.def.arch)?;
        let efi = dir.join(efi_name);
        let cfg = dir.join("grub.cfg");
        let env = dir.join("grubenv");

        self.runner.step(&format!(
            "write GRUB's config and environment block in {}",
            dir.display()
        ));
        let microcode: Vec<&str> = boot::MICROCODE
            .iter()
            .copied()
            .filter(|f| root.join("boot").join(f).is_file())
            .collect();
        let modules = self.def.modules.join(",");
        if !self.runner.dry_run {
            fs::create_dir_all(dir)?;
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
            "write loader.toml and copy the loader and {} to {}",
            boot::SLOT_CFG,
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
        // How GRUB starts this slot, read from the slot itself (M6.17). A
        // test tag goes on the slot's command line too, so the A/B test
        // sees that an update starts with its own lines under the loader
        // before it.
        let cmdline = match &self.loader_tag {
            Some(tag)
                if !tag.is_empty()
                    && tag
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') =>
            {
                format!("{cmdline} edel.tag={tag}")
            }
            Some(tag) => bail!("the loader tag {tag:?} must be letters, digits, - and _ only"),
            None => cmdline.to_string(),
        };
        fs::write(
            slot_dir.join(boot::SLOT_CFG),
            boot::slot_cfg(kernel, &modules, &cmdline, &microcode),
        )?;
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

    /// Builds the image's add-ons (M7.2a) once its slot is packed. Each
    /// starts as a copy of the root, or of the add-on it is built on
    /// (`on`, M8.17c), and gets its packages and files, its parts' too
    /// (`features`); what it adds becomes its own tree, packed as
    /// `OUT/IMAGE.NAME.ext4` with its feature file beside it. Packages come
    /// from the image's own package index, so an add-on matches the image
    /// exactly.
    fn build_addons(&self, work: &Path, root: &Path) -> Result<()> {
        let list: Vec<&Listed> = self.def.addons.iter().chain(&self.extra_addons).collect();
        if list.is_empty() {
            return Ok(());
        }
        // A plan has no cache yet, so it names a placeholder for it.
        let cache = match &self.apk_cache {
            Some(cache) if self.runner.dry_run || has_index(cache) => cache.clone(),
            None if self.runner.dry_run => PathBuf::from("APK_CACHE"),
            _ => bail!(
                "building add-ons needs --apk-cache with the image's package index, so they match the image exactly"
            ),
        };
        let a_root_of = |name: &str| work.join("addons").join(name).join("rootfs");
        let scan = |from: &Path| {
            if self.runner.dry_run {
                Ok(BTreeMap::new())
            } else {
                addons::scan(from)
            }
        };
        let image = scan(root)?;
        let mut bases: BTreeMap<String, BTreeMap<String, addons::Stamp>> = BTreeMap::new();
        let mut trees = Vec::new();
        let mut sets: Vec<(String, String)> = Vec::new();
        for addon in list.iter().copied() {
            let base = addon.feature.on.as_deref();
            let start = base.map_or(root.to_path_buf(), a_root_of);
            if let Some(base) = base
                && !bases.contains_key(base)
            {
                bases.insert(base.to_string(), scan(&start)?);
            }
            let before = base.and_then(|b| bases.get(b)).unwrap_or(&image);
            let earlier: Vec<String> = sets.iter().map(|(name, _)| name.clone()).collect();
            let (mut files, accounts) =
                self.build_addon(work, &start, addon, before, &cache, &earlier)?;
            // An add-on another is built `on` keeps its copy of the root
            // until that one is built; the others go at once.
            if !list
                .iter()
                .any(|a| a.feature.on.as_deref() == Some(&addon.name))
            {
                self.remove_addon_root(&a_root_of(&addon.name))?;
            }
            // Its copies of the merged caches lie over its base's by design.
            if base.is_some() {
                files.retain(|path| !addons::is_merged_cache(path) && !addons::is_font_cache(path));
            }
            trees.push((addon.name.clone(), files));
            sets.push((addon.name.clone(), accounts));
        }
        for base in bases.keys() {
            self.remove_addon_root(&a_root_of(base))?;
        }
        let clashes = addons::account_clashes(&sets);
        if !clashes.is_empty() {
            bail!(
                "add-ons clash in the account database: {}",
                clashes.join("; ")
            );
        }
        let tree_of = |name: &str| work.join("addons").join(name).join("tree");
        let same = |path: &str, a: &str, b: &str| {
            same_entry(&tree_of(a).join(path), &tree_of(b).join(path))
        };
        let collisions = addons::collisions(&trees, &same);
        if !collisions.is_empty() {
            let cache = collisions.iter().any(|line| {
                line.split_once(" is shipped by ")
                    .is_some_and(|(path, _)| addons::is_merged_cache(path))
            });
            let second = if cache {
                "; a cache two add-ons both change goes with its packages into one add-on"
            } else {
                ""
            };
            bail!(
                "add-ons ship the same files: {}{second}",
                collisions.join("; ")
            );
        }
        Ok(())
    }

    /// Adds the account lines of the add-ons built before (`earlier`) to an
    /// add-on's copy of the root, so its packages take ids none of them
    /// has (M8.17c, `addons::held_accounts`). Returns what it added to
    /// each account file, for [`Build::release_accounts`].
    fn hold_accounts(
        &self,
        a_root: &Path,
        work: &Path,
        earlier: &[String],
    ) -> Result<Vec<(&'static str, String)>> {
        let mut held = Vec::new();
        if self.runner.dry_run || earlier.is_empty() {
            return Ok(held);
        }
        for path in addons::ACCOUNT_FILES {
            let file = path.rsplit('/').next().unwrap_or(path);
            let lines: String = earlier
                .iter()
                .map(|other| {
                    let tree = work.join("addons").join(other).join("tree");
                    fs::read_to_string(tree.join(addons::accounts_dir(other)).join(file))
                        .unwrap_or_default()
                })
                .collect();
            let at = a_root.join(path);
            let Ok(text) = fs::read_to_string(&at) else {
                continue;
            };
            let add = addons::held_accounts(&text, &lines);
            if !add.is_empty() {
                fs::write(&at, format!("{text}{add}"))?;
                held.push((path, add));
            }
        }
        Ok(held)
    }

    /// Takes the lines [`Build::hold_accounts`] added out again.
    fn release_accounts(&self, a_root: &Path, held: &[(&'static str, String)]) -> Result<()> {
        for (path, add) in held {
            let at = a_root.join(path);
            let text = fs::read_to_string(&at).with_context(|| format!("reading {path}"))?;
            fs::write(&at, addons::without_held(&text, add))?;
        }
        Ok(())
    }

    /// Takes an add-on's copy of the root away once its tree is made.
    fn remove_addon_root(&self, a_root: &Path) -> Result<()> {
        if self.runner.dry_run {
            return Ok(());
        }
        ensure_nothing_mounted_under(a_root)?;
        fs::remove_dir_all(a_root).with_context(|| format!("removing {}", a_root.display()))
    }

    /// Builds one add-on (see [`Build::build_addons`]) from a copy of
    /// `start`, the image's root or its base add-on's. Returns the files
    /// its tree ships and its new account lines, for the checks that span
    /// the add-ons.
    fn build_addon(
        &self,
        work: &Path,
        start: &Path,
        addon: &Listed,
        before: &BTreeMap<String, addons::Stamp>,
        cache: &Path,
        earlier: &[String],
    ) -> Result<(Vec<String>, String)> {
        let name = addon.name.as_str();
        let dir = work.join("addons").join(name);
        let a_root = dir.join("rootfs");
        let tree = dir.join("tree");

        self.runner.step(&format!(
            "start add-on {name} from a copy of {}",
            start.display()
        ));
        if !self.runner.dry_run {
            if dir.exists() {
                ensure_nothing_mounted_under(&dir)?;
                fs::remove_dir_all(&dir).with_context(|| format!("removing {}", dir.display()))?;
            }
            fs::create_dir_all(&dir)?;
        }
        self.runner.run(
            Command::new("cp")
                .arg("-a")
                .arg(start.join("."))
                .arg(&a_root),
        )?;
        let held = self.hold_accounts(&a_root, work, earlier)?;
        let mut packages: Vec<String> = Vec::new();
        let mut programs: Vec<String> = Vec::new();
        for part in addon.parts() {
            add_new_names(&mut packages, &part.feature.packages);
            add_new_names(&mut programs, &part.feature.programs);
        }
        if packages.is_empty() {
            self.runner
                .step(&format!("add-on {name} installs no packages"));
        } else {
            // The package scripts run inside the copy, so it needs proc, sys
            // and dev too; the mounts end with this block.
            let _mounts = self.runner.mount_kernel_fs(&a_root)?;
            let mut apk = Command::new("apk");
            apk.arg("--root")
                .arg(&a_root)
                .args(["--arch", &self.def.arch, "--no-progress"]);
            self.runner
                .run(apk.arg("--cache-dir").arg(cache).arg("add").args(&packages))?;
        }
        self.release_accounts(&a_root, &held)?;
        for part in addon.parts() {
            if let Some(files) = &part.files {
                self.copy_tree(files, &a_root)?;
            }
        }
        self.copy_programs(&programs, &a_root)?;
        self.runner.step(&format!(
            "write the feature files of {name} to {}",
            features::DIR
        ));
        if !self.runner.dry_run {
            let features_dir = a_root.join(features::DIR.trim_start_matches('/'));
            fs::create_dir_all(&features_dir)?;
            for part in addon.parts() {
                fs::write(features_dir.join(format!("{}.toml", part.name)), &part.text)?;
            }
        }
        // A machine with the add-on keeps the platform level too (M8.17c):
        // the desktop brings Flatpak and its portals.
        if !self.runner.dry_run {
            let db = a_root.join(crate::bom::INSTALLED_DB);
            let text = fs::read_to_string(&db)
                .with_context(|| format!("reading apk's record of add-on {name}"))?;
            let packages = crate::bom::parse_installed(&text);
            let features: Vec<&str> = self
                .def
                .features
                .iter()
                .chain(addon.parts())
                .map(|f| f.name.as_str())
                .collect();
            self.check_platform(&packages, &features)
                .with_context(|| format!("checking add-on {name}"))?;
        }
        if keeps_font_cache(addon) {
            self.drop_unkept(addon, &a_root, before)?;
            self.bake_font_cache(&a_root)?;
        }

        let (files, accounts) = self.make_addon_tree(addon, start, &a_root, &tree, before)?;
        self.pack_addon(name, &tree, &addon.text)?;
        Ok((files, accounts))
    }

    /// Takes out of an add-on's root the new files its `[[keep]]` lists
    /// leave out, before its fonts' cache is baked (M8.17c), so the cache
    /// names only the fonts the add-on ships.
    fn drop_unkept(
        &self,
        addon: &Listed,
        a_root: &Path,
        before: &BTreeMap<String, addons::Stamp>,
    ) -> Result<()> {
        let keeps: Vec<edel::keep::Keep> = addon
            .parts()
            .flat_map(|part| part.feature.keep.iter().cloned())
            .collect();
        if self.runner.dry_run || keeps.is_empty() {
            return Ok(());
        }
        let after = addons::scan(a_root)?;
        let all = addons::diff(before, &after);
        let mut kept = all.clone();
        keep_addon(&addon.name, &keeps, &mut kept, &after)?;
        let kept: BTreeSet<&String> = kept.new.iter().collect();
        for path in all.new.iter().filter(|p| !kept.contains(p)) {
            let at = a_root.join(path);
            fs::remove_file(&at).with_context(|| format!("removing {}", at.display()))?;
        }
        Ok(())
    }

    /// Makes an add-on's tree (M7.2a): what its copy of the root adds, less
    /// what its `[[keep]]` lists leave out, sorted by `addons::plan_tree`
    /// into the slot's layout. Returns the files it ships and its new
    /// account lines; `build_addons` takes its copy of the root away.
    fn make_addon_tree(
        &self,
        addon: &Listed,
        root: &Path,
        a_root: &Path,
        tree: &Path,
        before: &BTreeMap<String, addons::Stamp>,
    ) -> Result<(Vec<String>, String)> {
        let name = addon.name.as_str();
        let keeps: Vec<edel::keep::Keep> = addon
            .parts()
            .flat_map(|part| part.feature.keep.iter().cloned())
            .collect();
        let font_cache = keeps_font_cache(addon);
        self.runner.step(&format!(
            "make the tree of add-on {name} at {}, with only the files it adds",
            tree.display()
        ));
        if self.runner.dry_run {
            return Ok((Vec::new(), String::new()));
        }
        let after = addons::scan(a_root)?;
        let mut diff = addons::diff(before, &after);
        // A trigger that writes a file again with the same bytes changes
        // only its time (the certificates' bundle when a package brings
        // ca-certificates, the fonts' cache setting the desktop bakes
        // again, M8.17c): the image's copy stays, so it is no change.
        diff.changed
            .retain(|path| !same_entry(&root.join(path), &a_root.join(path)));
        keep_addon(name, &keeps, &mut diff, &after)?;
        let plan = addons::plan_tree(&diff, &after, name, font_cache)?;
        let is_dir = |path: &str| after.get(path).is_some_and(|s| s.dir);

        fs::create_dir_all(tree)?;
        let mut shipped = Vec::new();
        let mut merged = Vec::new();
        for copy in &plan.copies {
            if !is_dir(&copy.from) {
                // A merged cache is in the image by design, so only any other
                // path the image already has is refused.
                let is_merged = plan.merged.contains(&copy.to);
                if !is_merged && root.join(&copy.to).symlink_metadata().is_ok() {
                    bail!(
                        "{} is already in the image; add-on {name} cannot ship it",
                        copy.to
                    );
                }
                // The packages rewrite the MIME database though nothing in it
                // changed; the image's copy shows through the overlay instead.
                if is_merged && same_entry(&root.join(&copy.to), &a_root.join(&copy.to)) {
                    continue;
                }
                if is_merged {
                    merged.push(copy.to.clone());
                }
                shipped.push(copy.to.clone());
            }
            copy_into_tree(a_root, tree, copy)?;
        }
        // Each copy keeps its root's time, set once all are in place, as a
        // file put into a folder moves that folder's time on: fontconfig
        // drops a baked cache whose folder's time differs (M6.2a). So do
        // the folders made as their parents, such as /usr/share/fonts that
        // the image has too, which the overlay shows with the add-on's time.
        for copy in &plan.copies {
            keep_time(&a_root.join(&copy.from), &tree.join(&copy.to))?;
        }
        keep_folder_times(a_root, tree, Path::new(""))?;
        // One line per add-on in the build's log: what it ships, the caches
        // it carries a merged copy of, those left out, and the base's small
        // tools it keeps where a package brought its own.
        let mut line = format!("add-on {name}: {} files", shipped.len());
        for (what, list) in [
            ("keeps the merged caches", addons::folded(&merged)),
            ("left out the system caches", addons::folded(&plan.caches)),
            (
                "keeps the base's small tools at",
                diff.kept_applets.join(", "),
            ),
        ] {
            if !list.is_empty() {
                line.push_str(&format!(", {what}: {list}"));
            }
        }
        println!("{line}");
        let mut accounts = String::new();
        for path in &plan.accounts {
            let read = |base: &Path| {
                fs::read_to_string(base.join(path)).with_context(|| format!("reading {path}"))
            };
            let (image, addon) = (read(root)?, read(a_root)?);
            let file = path.rsplit('/').next().unwrap_or(path);
            let dir = tree.join(addons::accounts_dir(name));
            // Members added to a group the image has, as QEMU puts its
            // user into kvm, join that group at boot (M7.2b).
            let members = if file == "group" {
                addons::added_members(&image, &addon)
            } else {
                String::new()
            };
            if !members.is_empty() {
                fs::create_dir_all(&dir)?;
                fs::write(dir.join(addons::MEMBERS_FILE), &members)?;
                fs::set_permissions(
                    dir.join(addons::MEMBERS_FILE),
                    fs::Permissions::from_mode(0o644),
                )?;
            }
            let was: BTreeMap<&str, &str> = image
                .lines()
                .map(|line| (line.split(':').next().unwrap_or_default(), line))
                .collect();
            let changed: Vec<String> = addons::changed_accounts(&image, &addon)
                .into_iter()
                .filter(|changed| {
                    let now = addon.lines().find(|l| l.split(':').next() == Some(changed));
                    !(file == "group"
                        && now
                            .zip(was.get(changed.as_str()))
                            .is_some_and(|(now, was)| addons::members_only(was, now)))
                })
                .collect();
            if !changed.is_empty() {
                let names = changed.join(", ");
                eprintln!(
                    "warning: add-on {name} changes the image's {path} line of {names}; the image's line stays, so put what it needs into the feature's files"
                );
            }
            let new = addons::new_accounts(&image, &addon);
            if new.is_empty() {
                continue;
            }
            fs::create_dir_all(&dir)?;
            fs::write(dir.join(file), &new)?;
            let mode = if file == "shadow" { 0o640 } else { 0o644 };
            fs::set_permissions(dir.join(file), fs::Permissions::from_mode(mode))?;
            accounts.push_str(&new);
        }

        Ok((shipped, accounts))
    }

    /// Packs an add-on's tree as `OUT/IMAGE.NAME.ext4`, a file system the
    /// tree's size plus a fifth and 8 MiB, shrunk and gzipped as a slot is,
    /// with its feature file beside it as `OUT/IMAGE.NAME.toml` (M7.2a).
    fn pack_addon(&self, name: &str, tree: &Path, text: &str) -> Result<()> {
        let stem = self.def.stem();
        let image = self.out.join(format!("{stem}.{name}.ext4"));
        // A plan has no tree to measure, so it shows the smallest size.
        let size_mib = if self.runner.dry_run {
            8
        } else {
            let mib = tree_bytes(tree)?.div_ceil(MIB);
            mib + mib.div_ceil(5) + 8
        };
        // ext4 labels are at most 16 bytes; names are a-z, 0-9 and '-'.
        let label = &name[..name.len().min(16)];
        self.make_slot(tree, &image, size_mib, label)?;
        self.shrink(&image)?;
        self.gzip(&image)?;
        let feature = self.out.join(format!("{stem}.{name}.toml"));
        self.runner.step(&format!(
            "write the feature file of {name} to {}",
            feature.display()
        ));
        if !self.runner.dry_run {
            fs::write(&feature, text)?;
        }
        Ok(())
    }
}

/// Whether `addon` keeps the fonts' cache it bakes (M8.17c): the desktop
/// brings the fonts, so it brings their cache too, and an add-on over it
/// the cache with its own fonts; any other add-on leaves the cache out and
/// its apps rebuild it, as before.
fn keeps_font_cache(addon: &Listed) -> bool {
    !addon.members.is_empty() || addon.feature.on.is_some()
}

/// Appends the names of `from` that `to` lacks.
fn add_new_names(to: &mut Vec<String>, from: &[String]) {
    for item in from {
        if !to.contains(item) {
            to.push(item.clone());
        }
    }
}

/// Reads the standard `SOURCE_DATE_EPOCH` variable (reproducible-builds.org),
/// which dates every file of a container image, so two builds of one commit
/// give the same bytes (M8.6). Unset or empty means no date.
fn source_date_epoch(value: Option<&str>) -> Result<Option<u64>> {
    match value {
        None | Some("") => Ok(None),
        Some(text) => match text.parse::<u64>() {
            Ok(seconds) if text.bytes().all(|b| b.is_ascii_digit()) => Ok(Some(seconds)),
            _ => bail!(
                "SOURCE_DATE_EPOCH is {text:?}, not a number of seconds since 1970; unset it or give the commit's time, as `git log -1 --format=%ct` prints it"
            ),
        },
    }
}

/// The arguments `tar` gets to pack a container image (M8.6). Names are
/// sorted so the order never depends on the file system; every file is dated
/// by the epoch when one is given, so files the build writes do not carry its
/// clock; gzip reads standard input, so its header holds no time.
fn container_tar_args(epoch: Option<u64>) -> Vec<String> {
    let mut args: Vec<String> = ["--numeric-owner", "--sort=name"].map(String::from).into();
    if let Some(seconds) = epoch {
        args.push(format!("--mtime=@{seconds}"));
    }
    args.push("-czf".into());
    args
}

/// Takes out of an add-on's new files what its `[[keep]]` lists do not keep
/// (M7.2a), as the image's own `[[keep]]` does for its packages: under each
/// folder, the new files a listed pattern matches stay. A pattern that
/// matches no new file is a mistake, as in the image. The image's own files
/// are never touched: the add-on only ships fewer of them.
fn keep_addon(
    name: &str,
    keeps: &[edel::keep::Keep],
    diff: &mut addons::Diff,
    after: &BTreeMap<String, addons::Stamp>,
) -> Result<()> {
    for keep in keeps {
        let under = keep.under.trim_matches('/');
        let prefix = format!("{under}/");
        // Whether a new file below the folder is one the list keeps.
        let keeps_it = |path: &str| {
            keep.files
                .iter()
                .any(|pattern| edel::keep::matches(pattern, &path[prefix.len()..]))
        };
        let files: Vec<&str> = diff
            .new
            .iter()
            .map(String::as_str)
            .filter(|p| p.starts_with(&prefix) && after.get(*p).is_some_and(|s| !s.dir))
            .collect();
        for pattern in &keep.files {
            if !files
                .iter()
                .any(|p| edel::keep::matches(pattern, &p[prefix.len()..]))
            {
                bail!(
                    "add-on {name}: keep pattern {pattern:?} under /{under} matches no new file; fix the list in the feature"
                );
            }
        }
        let total = files.len();
        let taken: BTreeSet<String> = files
            .iter()
            .copied()
            .filter(|path| !keeps_it(path))
            .map(str::to_string)
            .collect();
        let kept = total - taken.len();
        diff.new.retain(|p| !taken.contains(p));
        // A file of the start the packages rewrote below the folder (the
        // fonts' X index over the desktop, M8.17c) is no file the add-on
        // keeps either: its start's copy shows, or none where the add-on
        // under it took that copy out too.
        diff.changed
            .retain(|p| !(p.starts_with(&prefix) && !keeps_it(p)));
        println!(
            "add-on {name}: kept {kept} files under /{under}, took out {}",
            taken.len()
        );
    }
    Ok(())
}

/// Whether two add-ons' trees hold the same entry at `a` and `b`: links
/// to one target, or files with the same mode and bytes (M7.2a).
fn same_entry(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;

    let (Ok(ma), Ok(mb)) = (fs::symlink_metadata(a), fs::symlink_metadata(b)) else {
        return false;
    };
    if ma.file_type().is_symlink() || mb.file_type().is_symlink() {
        return fs::read_link(a)
            .ok()
            .is_some_and(|t| fs::read_link(b).ok() == Some(t));
    }
    ma.mode() == mb.mode()
        && ma.len() == mb.len()
        && fs::read(a)
            .ok()
            .is_some_and(|bytes| fs::read(b).ok() == Some(bytes))
}

/// Copies one path of an add-on's root into its tree (M7.2a): a folder or
/// a file with its mode and owner, a link with its target. Parents the plan
/// does not name are made as root's folders.
fn copy_into_tree(a_root: &Path, tree: &Path, copy: &addons::Copy) -> Result<()> {
    use std::os::unix::fs::MetadataExt;

    let src = a_root.join(&copy.from);
    let dst = tree.join(&copy.to);
    let meta = fs::symlink_metadata(&src).with_context(|| format!("reading {}", src.display()))?;
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }
    let link = meta.file_type().is_symlink();
    if link {
        let target = match &copy.link {
            Some(target) => PathBuf::from(target),
            None => fs::read_link(&src)?,
        };
        std::os::unix::fs::symlink(&target, &dst)
            .with_context(|| format!("linking {}", dst.display()))?;
    } else if meta.is_dir() {
        fs::create_dir_all(&dst)?;
    } else {
        fs::copy(&src, &dst).with_context(|| format!("copying {}", src.display()))?;
    }
    if !link {
        fs::set_permissions(&dst, fs::Permissions::from_mode(meta.mode() & 0o7777))?;
    }
    std::os::unix::fs::lchown(&dst, Some(meta.uid()), Some(meta.gid()))
        .with_context(|| format!("setting the owner of {}", dst.display()))?;
    Ok(())
}

/// Gives each folder below `tree` at `rel` the time of the folder at the
/// same place in `a_root`, where there is one: the parents the copies made.
fn keep_folder_times(a_root: &Path, tree: &Path, rel: &Path) -> Result<()> {
    for entry in fs::read_dir(tree.join(rel))? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            keep_folder_times(a_root, tree, &rel.join(entry.file_name()))?;
        }
    }
    let src = a_root.join(rel);
    if fs::symlink_metadata(&src).is_ok_and(|m| m.is_dir()) {
        keep_time(&src, &tree.join(rel))?;
    }
    Ok(())
}

/// Gives `dst` the time `src` was last changed; a link keeps its own, as
/// nothing reads a link's time.
fn keep_time(src: &Path, dst: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(src).with_context(|| format!("reading {}", src.display()))?;
    if meta.file_type().is_symlink() {
        return Ok(());
    }
    fs::File::open(dst)
        .and_then(|f| f.set_modified(meta.modified()?))
        .with_context(|| format!("setting the time of {}", dst.display()))
}

/// The data partition's size in MiB (M6.2): the definition's, or more
/// when its files need it, with a quarter more and 64 MiB for ext4's own
/// tables and journal; it grows to the disk on first boot anyway.
fn data_mib_for(defined: u64, bytes: u64) -> u64 {
    defined.max((bytes + bytes / 4).div_ceil(MIB) + 64)
}

/// The bytes the files under `dir` hold, each file once however many hard
/// links it has, as Flatpak's checkouts link the repository's objects.
fn tree_bytes(dir: &Path) -> Result<u64> {
    use std::collections::HashSet;
    use std::os::unix::fs::MetadataExt;

    let mut seen = HashSet::new();
    let mut total = 0;
    let mut todo = vec![dir.to_path_buf()];
    while let Some(dir) = todo.pop() {
        for entry in fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))? {
            let path = entry?.path();
            let meta = fs::symlink_metadata(&path)?;
            if meta.is_dir() {
                todo.push(path);
            } else if seen.insert((meta.dev(), meta.ino())) {
                total += meta.blocks() * 512;
            }
        }
    }
    Ok(total)
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
/// a password. Access is granted later by keys or the settings file.
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

/// Dates every account's last password change in `/etc/shadow` by the
/// epoch's day (M8.6): busybox's `adduser` writes the day it runs there, so
/// two builds either side of midnight made different images. An empty field
/// stays empty.
fn date_shadow(shadow: &str, epoch: u64) -> String {
    let day = (epoch / 86_400).to_string();
    let mut out = String::with_capacity(shadow.len() + 1);
    for line in shadow.lines() {
        let mut fields: Vec<&str> = line.split(':').collect();
        if fields.get(2).is_some_and(|field| !field.is_empty()) {
            fields[2] = &day;
        }
        out.push_str(&fields.join(":"));
        out.push('\n');
    }
    out
}

/// Whether an apk cache already holds a package index.
fn has_index(cache: &Path) -> bool {
    fs::read_dir(cache).is_ok_and(|entries| {
        entries
            .flatten()
            .any(|e| e.file_name().to_string_lossy().starts_with("APKINDEX."))
    })
}

/// Where Alpine's init waits for the root device, just before it may drop
/// to a shell and then mounts it.
const ROOT_FOUND: &str = "\tif [ \"$SINGLEMODE\" = \"yes\" ]; then\n";

/// Our change to Alpine's init (M1.12): once nlplug-findfs has found the
/// root, a `root=PARTUUID=` becomes its device, read from the kernel's
/// partition events, as busybox `mount` knows only `UUID=` and `LABEL=`.
const PARTUUID_ROOT: &str = "\t# Edel OS (M1.12): GRUB names the slot by its partition, which
\t# nlplug-findfs found; busybox mount needs its device.
\tcase \"$KOPT_root\" in
\tPARTUUID=*)
\t\tfor _uevent in /sys/class/block/*/uevent; do
\t\t\tif grep -qix \"PARTUUID=${KOPT_root#PARTUUID=}\" \"$_uevent\"; then
\t\t\t\tKOPT_root=/dev/$(sed -n 's/^DEVNAME=//p' \"$_uevent\")
\t\t\t\tbreak
\t\t\tfi
\t\tdone
\t\t;;
\tesac
";

/// Alpine's initramfs `init`, `alpine`, with [`PARTUUID_ROOT`] where it has
/// found the root device; an error when that place is not where it was,
/// so a changed Alpine init stops the build rather than the boot.
pub fn partuuid_init(alpine: &str) -> Result<String> {
    let start = alpine
        .find("ebegin \"Mounting root\"")
        .context("Alpine's initramfs init no longer says \"Mounting root\"; M1.12's change needs a new place")?;
    let at = alpine[start..]
        .find(ROOT_FOUND)
        .map(|i| start + i)
        .context("Alpine's initramfs init no longer checks SINGLEMODE after finding the root; M1.12's change needs a new place")?;
    Ok(format!("{}{PARTUUID_ROOT}{}", &alpine[..at], &alpine[at..]))
}

/// Every file and link below `dir`, with its path from `dir`, where a
/// link points and a file's size.
fn keep_entries(dir: &Path) -> Result<Vec<edel::keep::Entry>> {
    let mut entries = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(at) = stack.pop() {
        for entry in fs::read_dir(&at)? {
            let entry = entry?;
            let path = entry.path();
            let meta = fs::symlink_metadata(&path)?;
            let rel = path
                .strip_prefix(dir)
                .unwrap_or(&path)
                .to_string_lossy()
                .into_owned();
            if meta.is_dir() {
                stack.push(path);
            } else if meta.file_type().is_symlink() {
                let to = fs::read_link(&path)?.to_string_lossy().into_owned();
                entries.push(edel::keep::Entry {
                    path: rel,
                    link: Some(to),
                    size: 0,
                });
            } else {
                entries.push(edel::keep::Entry {
                    path: rel,
                    link: None,
                    size: meta.len(),
                });
            }
        }
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}

/// The firmware names kernel module `module` declares (`firmware=` in its
/// `.modinfo`), read from the root's kernel; the module may be gzipped, as
/// Alpine ships them.
fn declared_firmware(root: &Path, module: &str) -> Result<Vec<String>> {
    let wanted = [format!("{module}.ko"), format!("{module}.ko.gz")];
    let mut stack = vec![root.join("lib/modules")];
    while let Some(at) = stack.pop() {
        let Ok(dir) = fs::read_dir(&at) else { continue };
        for entry in dir.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if !wanted.contains(&name) {
                continue;
            }
            let raw = fs::read(&path)?;
            let mut bytes = Vec::new();
            if name.ends_with(".gz") {
                flate2::read::GzDecoder::new(raw.as_slice())
                    .read_to_end(&mut bytes)
                    .with_context(|| format!("unpacking {}", path.display()))?;
            } else {
                bytes = raw;
            }
            return Ok(firmware_names(&bytes));
        }
    }
    bail!(
        "no kernel module {module} in the root's /lib/modules: keep.newest_by names the driver that loads the versioned files, so the kernel's own limit can be read"
    )
}

/// The `firmware=NAME` strings of a module's `.modinfo` section.
fn firmware_names(bytes: &[u8]) -> Vec<String> {
    bytes
        .split(|&b| b == 0)
        .filter_map(|s| s.strip_prefix(b"firmware="))
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect()
}

/// Removes the folders below `dir` that hold nothing any more.
fn remove_empty_dirs(dir: &Path) -> Result<()> {
    for entry in fs::read_dir(dir)?.flatten() {
        let path = entry.path();
        if fs::symlink_metadata(&path)?.is_dir() {
            remove_empty_dirs(&path)?;
            if fs::read_dir(&path)?.next().is_none() {
                fs::remove_dir(&path)?;
            }
        }
    }
    Ok(())
}

/// Bytes as MiB.
fn mib(bytes: u64) -> f64 {
    bytes as f64 / 1_048_576.0
}

/// Sets the modification time of `dir` and every folder below it to its
/// whole second, as the slot's file system keeps it (M6.2a).
fn whole_seconds(dir: &Path) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() && !path.is_symlink() {
            whole_seconds(&path)?;
        }
    }
    let modified = fs::metadata(dir)?.modified()?;
    let secs = modified
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    fs::File::open(dir)
        .and_then(|f| f.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs)))
        .with_context(|| format!("rounding the time of {}", dir.display()))
}

/// Whether `dir` holds a file anywhere below it; false when it is not
/// there (M6.2a).
fn has_file(dir: &Path) -> Result<bool> {
    if !dir.is_dir() {
        return Ok(false);
    }
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_file() || (path.is_dir() && has_file(&path)?) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The fontconfig file that names `dir` as a cache folder (M6.2a), the
/// text `bake_font_cache` writes to `places::FONT_CACHE_CONF`.
fn font_cache_conf(dir: &str) -> String {
    format!(
        "<?xml version=\"1.0\"?>\n\
         <!DOCTYPE fontconfig SYSTEM \"urn:fontconfig:fonts.dtd\">\n\
         <!-- Written by edel image build (M6.2a): the fonts' cache baked into the slot, read first. -->\n\
         <fontconfig>\n\
         \t<cachedir>{dir}</cachedir>\n\
         </fontconfig>\n"
    )
}

/// The Mesa driver extension a runtime's metadata asks for (M6.2a): the
/// `versions=` of its `[Extension org.freedesktop.Platform.GL]` section,
/// else its `version=`; None when the section is absent.
fn gl_versions(metadata: &str) -> Option<Vec<String>> {
    let mut lines = metadata.lines().map(str::trim);
    lines.find(|line| *line == "[Extension org.freedesktop.Platform.GL]")?;
    let mut versions = None;
    let mut version = None;
    for line in lines {
        if line.starts_with('[') {
            break;
        }
        if let Some(list) = line.strip_prefix("versions=") {
            versions = Some(
                list.split(';')
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .map(String::from)
                    .collect(),
            );
        } else if let Some(one) = line.strip_prefix("version=") {
            version = Some(vec![one.trim().to_string()]).filter(|v| !v[0].is_empty());
        }
    }
    versions.or(version)
}

/// The folders directly inside `dir`, sorted.
fn subdirs(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}

/// The last part of a path, as text.
fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The runtimes under `apps` whose Mesa driver (`org.freedesktop.Platform.GL.default`)
/// is missing (M6.2a): each runtime's metadata says which versions of the
/// driver it wants, and one of them must be installed for its arch. Sorted.
fn gl_missing(apps: &Path) -> Result<Vec<String>> {
    let runtimes = apps.join("runtime");
    let mut missing = Vec::new();
    if !runtimes.is_dir() {
        return Ok(missing);
    }
    for name_dir in subdirs(&runtimes)? {
        let name = name_of(&name_dir);
        for arch_dir in subdirs(&name_dir)? {
            let arch = name_of(&arch_dir);
            for branch_dir in subdirs(&arch_dir)? {
                let branch = name_of(&branch_dir);
                let metadata = branch_dir.join("active/metadata");
                if !metadata.is_file() {
                    continue;
                }
                let Some(versions) = gl_versions(&fs::read_to_string(&metadata)?) else {
                    continue;
                };
                let driver = runtimes
                    .join("org.freedesktop.Platform.GL.default")
                    .join(&arch);
                if !versions.iter().any(|v| driver.join(v).is_dir()) {
                    missing.push(format!(
                        "{name}//{branch} (wants org.freedesktop.Platform.GL.default of {})",
                        versions.join(" or ")
                    ));
                }
            }
        }
    }
    missing.sort();
    Ok(missing)
}

/// Whether a Flatpak remote file (M6.4) has what the store needs: the
/// `[Flatpak Repo]` group and a `GPGKey` line, the key its apps are checked
/// with.
fn is_flatpakrepo(text: &str) -> bool {
    text.lines().any(|line| line.trim() == "[Flatpak Repo]")
        && text
            .lines()
            .any(|line| line.trim_start().starts_with("GPGKey="))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_date_epoch_takes_digits_and_nothing_else() {
        assert_eq!(source_date_epoch(None).unwrap(), None);
        assert_eq!(source_date_epoch(Some("")).unwrap(), None);
        assert_eq!(
            source_date_epoch(Some("1760000000")).unwrap(),
            Some(1760000000)
        );
        for bad in ["abc", "-1", "1.5", " 1"] {
            assert!(
                source_date_epoch(Some(bad)).is_err(),
                "{bad:?} must be refused"
            );
        }
    }

    #[test]
    fn container_tar_sorts_and_dates_every_file() {
        assert_eq!(
            container_tar_args(None),
            ["--numeric-owner", "--sort=name", "-czf"]
        );
        assert_eq!(
            container_tar_args(Some(42)),
            ["--numeric-owner", "--sort=name", "--mtime=@42", "-czf"]
        );
    }

    #[test]
    fn alpines_init_resolves_a_root_named_by_its_partition() {
        let alpine = "if [ -n \"$KOPT_root\" ]; then\n\tebegin \"Mounting root\"\n\tnlplug-findfs \"$KOPT_root\"\n\n\tif [ \"$SINGLEMODE\" = \"yes\" ]; then\n\t\tsh\n\tfi\n\tmount \"${KOPT_root#ZFS=}\" \"$sysroot\"\nfi\n";
        let ours = partuuid_init(alpine).unwrap();
        let resolve = ours.find("PARTUUID=*)").unwrap();
        assert!(ours.find("nlplug-findfs").unwrap() < resolve);
        assert!(resolve < ours.find("SINGLEMODE").unwrap());
        assert!(ours.contains("KOPT_root=/dev/$(sed -n 's/^DEVNAME=//p' \"$_uevent\")"));
        // A changed Alpine init stops the build.
        assert!(partuuid_init("ebegin \"Mounting root\"\n").is_err());
        assert!(partuuid_init("no root here").is_err());
    }

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
    fn dates_every_password_change_by_the_epoch() {
        // 1792281034 is 2026-10-17 23:50:34, day 20743; a build after
        // midnight once wrote 20744 for klogd.
        let shadow = "root:*:0:0:::::\nklogd:!:20744:0:99999:7:::\nbin:!::0:::::\n";
        assert_eq!(
            date_shadow(shadow, 1_792_281_034),
            "root:*:20743:0:::::\nklogd:!:20743:0:99999:7:::\nbin:!::0:::::\n"
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
                "EDEL_CHANNEL=\"preview\"\nEDEL_ARCH=\"x86_64\"\nEDEL_PLATFORM_LEVEL=1\n"
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

    #[test]
    fn a_modules_firmware_names_are_read_from_its_modinfo() {
        let bytes = b"license=GPL\0firmware=iwlwifi-so-a0-gf-a0-89.ucode\0firmware=iwlwifi-cc-a0-77.ucode\0name=iwlwifi\0";
        assert_eq!(
            firmware_names(bytes),
            ["iwlwifi-so-a0-gf-a0-89.ucode", "iwlwifi-cc-a0-77.ucode"]
        );
    }

    #[test]
    fn the_data_partition_grows_to_hold_the_default_apps() {
        assert_eq!(data_mib_for(64, 0), 64);
        // 800 MiB of apps: a quarter more and 64 MiB of room.
        assert_eq!(data_mib_for(64, 800 * MIB), 1064);
        assert_eq!(data_mib_for(2000, 800 * MIB), 2000);
    }

    #[test]
    fn hard_links_count_once() {
        let dir = std::env::temp_dir().join(format!("edel-tree-bytes-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("a")).unwrap();
        fs::write(dir.join("a/one"), vec![1u8; 8192]).unwrap();
        fs::hard_link(dir.join("a/one"), dir.join("two")).unwrap();
        let bytes = tree_bytes(&dir).unwrap();
        fs::remove_dir_all(&dir).unwrap();
        assert!((8192..16384).contains(&bytes), "{bytes}");
    }

    #[test]
    fn a_copied_folder_keeps_its_time_after_files_go_in() {
        let dir = std::env::temp_dir().join(format!("edel-keep-time-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("src/fonts")).unwrap();
        let then = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        fs::File::open(dir.join("src/fonts"))
            .unwrap()
            .set_modified(then)
            .unwrap();
        fs::create_dir_all(dir.join("dst/fonts")).unwrap();
        fs::write(dir.join("dst/fonts/a.ttf"), b"x").unwrap();
        keep_time(&dir.join("src/fonts"), &dir.join("dst/fonts")).unwrap();
        let t = fs::metadata(dir.join("dst/fonts"))
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(t, then);
        // A parent the copies made takes its root's time too.
        fs::create_dir_all(dir.join("src/fonts/noto")).unwrap();
        fs::create_dir_all(dir.join("dst/fonts/noto")).unwrap();
        fs::File::open(dir.join("src/fonts"))
            .unwrap()
            .set_modified(then)
            .unwrap();
        keep_folder_times(&dir.join("src"), &dir.join("dst"), Path::new("")).unwrap();
        let t = fs::metadata(dir.join("dst/fonts"))
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(t, then);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn rounds_folder_times_to_whole_seconds() {
        let dir = std::env::temp_dir().join(format!("edel-whole-seconds-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("noto")).unwrap();
        fs::write(dir.join("noto/a.ttf"), b"x").unwrap();
        whole_seconds(&dir).unwrap();
        for d in [dir.clone(), dir.join("noto")] {
            let t = fs::metadata(&d).unwrap().modified().unwrap();
            let nanos = t
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos();
            assert_eq!(nanos, 0, "{}", d.display());
        }
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_font_cache_conf_names_the_baked_folder() {
        let text = font_cache_conf(edel::places::FONT_CACHE_DIR);
        assert!(text.contains(&format!(
            "<cachedir>{}</cachedir>",
            edel::places::FONT_CACHE_DIR
        )));
        assert!(text.starts_with("<?xml"));
    }

    #[test]
    fn reads_the_gl_versions_of_a_runtime() {
        let list = "[Runtime]\nname=x\n\n[Extension org.freedesktop.Platform.GL]\ndirectory=lib/GL\nversions=25.08;25.08-extra\n";
        assert_eq!(
            gl_versions(list),
            Some(vec!["25.08".to_string(), "25.08-extra".to_string()])
        );
        let one = "[Extension org.freedesktop.Platform.GL]\nversion=1.4\n";
        assert_eq!(gl_versions(one), Some(vec!["1.4".to_string()]));
        let other =
            "[Extension org.freedesktop.Platform.GL.default]\nversions=9\n[Runtime]\nname=x\n";
        assert_eq!(gl_versions(other), None);
        let none = "[Runtime]\nversions=3\n";
        assert_eq!(gl_versions(none), None);
    }

    #[test]
    fn names_a_runtime_whose_mesa_driver_is_missing() {
        let dir =
            std::env::temp_dir().join(format!("edel-gl-missing-named-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let runtime = dir.join("runtime/org.edel.Platform/x86_64/25.08/active");
        fs::create_dir_all(&runtime).unwrap();
        fs::write(
            runtime.join("metadata"),
            "[Extension org.freedesktop.Platform.GL]\nversions=25.08\n",
        )
        .unwrap();
        let missing = gl_missing(&dir).unwrap();
        fs::create_dir_all(dir.join("runtime/org.freedesktop.Platform.GL.default/x86_64/25.08"))
            .unwrap();
        let found = gl_missing(&dir).unwrap();
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(
            missing,
            vec!["org.edel.Platform//25.08 (wants org.freedesktop.Platform.GL.default of 25.08)"]
        );
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_runtime_without_the_gl_section_is_never_named() {
        let dir =
            std::env::temp_dir().join(format!("edel-gl-missing-plain-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        assert!(gl_missing(&dir).unwrap().is_empty());
        let runtime = dir.join("runtime/org.edel.TestPlatform/x86_64/1/active");
        fs::create_dir_all(&runtime).unwrap();
        fs::write(
            runtime.join("metadata"),
            "[Runtime]\nname=org.edel.TestPlatform\n",
        )
        .unwrap();
        let missing = gl_missing(&dir).unwrap();
        fs::remove_dir_all(&dir).unwrap();
        assert!(missing.is_empty(), "{missing:?}");
    }

    /// M7.2a: an add-on's `[[keep]]` takes out only its new files that no
    /// pattern keeps, and never a file the image already had.
    #[test]
    fn an_add_on_keeps_only_the_new_files_its_keep_lists() {
        let stamp = |dir: bool| addons::Stamp {
            dir,
            size: 1,
            mtime: 0,
            mode: 0o644,
            link: None,
        };
        let after: BTreeMap<String, addons::Stamp> = [
            ("usr", true),
            ("usr/share", true),
            ("usr/share/fonts", true),
            ("usr/share/fonts/NotoSansCJK-Regular.ttf", false),
            ("usr/share/fonts/DejaVu.ttf", false),
        ]
        .into_iter()
        .map(|(path, dir)| (path.to_string(), stamp(dir)))
        .collect();
        let keep = edel::keep::Keep {
            under: "usr/share/fonts".into(),
            files: vec!["NotoSansCJK-*".into()],
            newest: Vec::new(),
            newest_by: None,
        };
        let mut diff = addons::Diff {
            new: after.keys().cloned().collect(),
            changed: vec!["usr/share/fonts/fonts.scale".into(), "usr/bin/x".into()],
            ..Default::default()
        };
        keep_addon("fonts-cjk", &[keep], &mut diff, &after).unwrap();
        // The index the packages rewrote below the folder stays out.
        assert_eq!(diff.changed, ["usr/bin/x"]);
        assert_eq!(
            diff.new,
            [
                "usr",
                "usr/share",
                "usr/share/fonts",
                "usr/share/fonts/NotoSansCJK-Regular.ttf"
            ]
        );

        let missing = edel::keep::Keep {
            under: "usr/share/fonts".into(),
            files: vec!["Hack-*".into()],
            newest: Vec::new(),
            newest_by: None,
        };
        let mut diff = addons::Diff {
            new: after.keys().cloned().collect(),
            ..Default::default()
        };
        let err = keep_addon("fonts-hack", &[missing], &mut diff, &after).unwrap_err();
        assert!(err.to_string().contains("matches no new file"), "{err}");
    }

    #[test]
    fn a_flatpakrepo_file_needs_its_group_and_a_key() {
        let file =
            "[Flatpak Repo]\nTitle=Flathub\nUrl=https://dl.flathub.org/repo/\nGPGKey=mQINBF\n";
        assert!(is_flatpakrepo(file));
        assert!(!is_flatpakrepo(
            "[Flatpak Repo]\nTitle=Flathub\nUrl=https://dl.flathub.org/repo/\n"
        ));
        assert!(!is_flatpakrepo(""));
    }
}
