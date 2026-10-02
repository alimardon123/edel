//! The boot loader rides along with the slot (roadmap M1.8). Each slot
//! carries the GRUB binary, `grub.cfg` and `loader.toml` (its version) in
//! `/usr/lib/edel/boot/`. Only after a slot is confirmed does
//! `edel update mark-good` compare that version with the one on the EFI
//! system partition and, when they differ, swap the files in: the loader is
//! the one piece outside the A/B rollback, so it changes only under a slot
//! known to work. The environment block is never replaced.

use std::fs::{self, File};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

use crate::release::{os_release_value, to_hex};

/// Where a slot keeps the loader it was built with.
pub const SLOT_DIR: &str = "/usr/lib/edel/boot";

/// `loader.toml` for a GRUB binary and its config: the version is a hash
/// of both, so any change to either is a new loader.
pub fn loader_toml(efi: &[u8], cfg: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(efi);
    hasher.update(cfg);
    let version = to_hex(&hasher.finalize()[..8]);
    format!("format = 1\nversion = \"{version}\"\n")
}

fn version_in(path: &Path) -> Option<String> {
    os_release_value(&fs::read_to_string(path).ok()?, "version")
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    PathBuf::from(format!("{}{suffix}", path.display()))
}

/// Replaces `dest` with `src` the careful way on FAT, where a rename is not
/// guaranteed atomic: keep the old file as `.prev`, write `.new`, flush it,
/// rename it over `dest` and flush the directory. `dest` exists at every
/// moment, so a power cut leaves the old or the new file.
pub fn swap_file(src: &Path, dest: &Path) -> Result<()> {
    if dest.exists() {
        fs::copy(dest, with_suffix(dest, ".prev"))?;
    }
    let new = with_suffix(dest, ".new");
    fs::copy(src, &new).with_context(|| format!("writing {}", new.display()))?;
    File::open(&new)?.sync_all()?;
    fs::rename(&new, dest).with_context(|| format!("replacing {}", dest.display()))?;
    if let Some(dir) = dest.parent() {
        File::open(dir)?.sync_all()?;
    }
    Ok(())
}

/// Installs the running slot's loader on the EFI system partition mounted
/// at `esp` when its version differs. Returns a line to report, if any.
pub fn update_esp(esp: &Path, slot: &Path, efi_name: &str) -> Result<Option<String>> {
    let Some(version) = version_in(&slot.join("loader.toml")) else {
        return Ok(None);
    };
    let on_esp = esp.join("EFI/edel/loader.toml");
    if version_in(&on_esp).as_deref() == Some(version.as_str()) {
        return Ok(None);
    }
    // The version file goes last, so it names a loader that is in place.
    swap_file(&slot.join(efi_name), &esp.join("EFI/BOOT").join(efi_name))?;
    swap_file(&slot.join("grub.cfg"), &esp.join("EFI/edel/grub.cfg"))?;
    swap_file(&slot.join("loader.toml"), &on_esp)?;
    Ok(Some(format!(
        "edel update: boot loader updated to {version}"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("edel-{test}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("slot")).unwrap();
        fs::create_dir_all(dir.join("esp/EFI/BOOT")).unwrap();
        fs::create_dir_all(dir.join("esp/EFI/edel")).unwrap();
        dir
    }

    #[test]
    fn swaps_keeping_the_previous_file() {
        let dir = tree("swap-file");
        let (src, dest) = (
            dir.join("slot/BOOTX64.EFI"),
            dir.join("esp/EFI/BOOT/BOOTX64.EFI"),
        );
        fs::write(&src, "new loader").unwrap();
        fs::write(&dest, "old loader").unwrap();
        swap_file(&src, &dest).unwrap();
        assert_eq!(fs::read_to_string(&dest).unwrap(), "new loader");
        assert_eq!(
            fs::read_to_string(with_suffix(&dest, ".prev")).unwrap(),
            "old loader"
        );
        assert!(!with_suffix(&dest, ".new").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn installs_a_different_loader_once() {
        let dir = tree("update-esp");
        let (slot, esp) = (dir.join("slot"), dir.join("esp"));
        fs::write(slot.join("BOOTX64.EFI"), "grub 2").unwrap();
        fs::write(slot.join("grub.cfg"), "cfg 2").unwrap();
        fs::write(slot.join("loader.toml"), loader_toml(b"grub 2", b"cfg 2")).unwrap();
        fs::write(esp.join("EFI/BOOT/BOOTX64.EFI"), "grub 1").unwrap();
        fs::write(esp.join("EFI/edel/grubenv"), "env").unwrap();
        fs::write(
            esp.join("EFI/edel/loader.toml"),
            loader_toml(b"grub 1", b"cfg 1"),
        )
        .unwrap();
        let line = update_esp(&esp, &slot, "BOOTX64.EFI").unwrap().unwrap();
        assert!(line.contains("boot loader updated"));
        assert_eq!(
            fs::read_to_string(esp.join("EFI/BOOT/BOOTX64.EFI")).unwrap(),
            "grub 2"
        );
        assert_eq!(
            fs::read_to_string(esp.join("EFI/edel/grub.cfg")).unwrap(),
            "cfg 2"
        );
        assert_eq!(
            fs::read_to_string(esp.join("EFI/edel/grubenv")).unwrap(),
            "env"
        );
        assert_eq!(update_esp(&esp, &slot, "BOOTX64.EFI").unwrap(), None);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_slot_without_a_loader_changes_nothing() {
        let dir = tree("no-loader");
        assert_eq!(
            update_esp(&dir.join("esp"), &dir.join("slot"), "BOOTX64.EFI").unwrap(),
            None
        );
        fs::remove_dir_all(&dir).unwrap();
    }
}
