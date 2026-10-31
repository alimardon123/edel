//! One way to replace a file whole (the architecture review of
//! 2026-10-31): the new bytes go to a file of this process's own beside
//! it, are flushed to the disk, and are renamed over it, then the folder
//! is flushed, so a power cut leaves the old file or the new one, never
//! an empty or half one. A file that is there keeps its mode and owner
//! (an account file stays `0640 root:shadow`), and the new file is never
//! more open than it while it is written. A failed write, a full disk
//! say, takes the half-written file out again. Std only, so every part
//! can use it, the desktop's too.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// The mode of a file made new, before the umask, as `fs::write` makes it.
const NEW_MODE: u32 = 0o666;

/// Replaces `path` with `bytes` as this module says, making its folder
/// first when it is not there. A link at `path` is replaced by the file;
/// a writer that keeps links passes where the link points.
pub fn replace(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(dir)?;
    let old = fs::metadata(path).ok();
    let new = beside(path);
    let written = write_new(&new, bytes, old.as_ref())
        .and_then(|()| fs::rename(&new, path))
        .and_then(|()| File::open(dir)?.sync_all());
    if written.is_err() {
        let _ = fs::remove_file(&new);
    }
    written
}

/// The new file's name: `PATH.edel-new-PID`, this process's own, so two
/// writers never write into one file.
fn beside(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".edel-new-{}", std::process::id()));
    PathBuf::from(name)
}

fn write_new(new: &Path, bytes: &[u8], old: Option<&fs::Metadata>) -> io::Result<()> {
    // One a cut left is made again, so it starts with the mode below.
    let _ = fs::remove_file(new);
    let mode = old.map_or(NEW_MODE, |meta| meta.permissions().mode() & 0o7777);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(new)?;
    file.write_all(bytes)?;
    if let Some(meta) = old {
        let made = file.metadata()?;
        // Only root may give a file away: a person's own write of a file
        // root once made becomes theirs, as with any editor.
        if (made.uid(), made.gid()) != (meta.uid(), meta.gid()) {
            match std::os::unix::fs::chown(new, Some(meta.uid()), Some(meta.gid())) {
                Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {}
                other => other?,
            }
        }
        // The umask may have taken bits the old file had.
        fs::set_permissions(new, meta.permissions())?;
    }
    file.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("edel-files-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_file_is_replaced_whole_and_keeps_its_mode() {
        let dir = dir("mode");
        let path = dir.join("shadow");
        fs::write(&path, "old").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        replace(&path, b"new").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "new");
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o640
        );
        assert_eq!(
            fs::read_dir(&dir).unwrap().count(),
            1,
            "nothing left beside it"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_new_file_gets_its_folder_and_a_failed_write_leaves_nothing() {
        let dir = dir("new");
        let path = dir.join("a/b/file.toml");
        replace(&path, b"text").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "text");
        // A folder where the file should go: the rename fails, the old
        // folder stays and the new file is taken out.
        let blocked = dir.join("blocked");
        fs::create_dir_all(blocked.join("inside")).unwrap();
        assert!(replace(&blocked, b"text").is_err());
        assert!(blocked.join("inside").is_dir());
        assert!(!beside(&blocked).exists());
        fs::remove_dir_all(&dir).unwrap();
    }
}
