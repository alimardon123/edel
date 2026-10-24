//! The one way shell-ui writes a file whole (ADR-010): `replace` writes it
//! to `PATH.new` and renames that over the file, so a reader never finds it
//! half written. What to write, and whether to write at all, stays with each
//! caller: the apps' overrides (`app_permissions.rs`), the apps' colours
//! (`apps_colours.rs`), foot's theme (`terminal.rs`), the stamps of
//! `telling.rs`, the greeter's sessions and the look pictures.

use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

/// Writes `bytes` to `path` through `PATH.new` renamed over it, making the
/// folder when it is missing. `PATH.new` is made afresh: whatever was left
/// at that name, a link too, is removed first, never written through.
pub fn replace(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut new = path.as_os_str().to_owned();
    new.push(".new");
    let new = PathBuf::from(new);
    match std::fs::remove_file(&new) {
        Ok(()) => {}
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&new)?
        .write_all(bytes)?;
    std::fs::rename(&new, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_is_written_whole_in_a_new_folder_and_a_link_at_new_is_not_followed() {
        let dir = std::env::temp_dir().join(format!("edel-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("deep").join("file");
        replace(&path, b"one\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "one\n");
        let target = dir.join("target");
        std::fs::write(&target, "kept\n").unwrap();
        std::os::unix::fs::symlink(&target, dir.join("deep").join("file.new")).unwrap();
        replace(&path, b"two\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "two\n");
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "kept\n");
        assert!(!dir.join("deep").join("file.new").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
