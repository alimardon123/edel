//! A file's extended attributes, read and put back. A file's capabilities
//! (`security.capability`, which `newuidmap` needs for rootless containers)
//! live there, and the kernel drops them when the file's owner changes, so
//! whatever copies a file or changes its owner keeps them with these.

use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use anyhow::{Context, Result};

/// A file's extended attributes: names and values.
pub type Attrs = Vec<(CString, Vec<u8>)>;

fn c_path(path: &Path) -> Result<CString> {
    CString::new(path.as_os_str().as_bytes())
        .with_context(|| format!("{} has a NUL in its name", path.display()))
}

/// Calls `call` with a buffer, first to learn the size, then to fill it.
fn sized(mut call: impl FnMut(*mut libc::c_void, usize) -> isize) -> Option<Vec<u8>> {
    let size = call(std::ptr::null_mut(), 0);
    if size < 0 {
        return None;
    }
    let mut buf = vec![0u8; size as usize];
    let got = call(buf.as_mut_ptr().cast(), buf.len());
    (got >= 0).then(|| {
        buf.truncate(got as usize);
        buf
    })
}

/// The attributes of `path` itself (a link's own, never its target's). A
/// file system without them gives none.
pub fn read(path: &Path) -> Result<Attrs> {
    let at = c_path(path)?;
    // SAFETY: `at` is a NUL-terminated path; the buffer is as long as said.
    let Some(names) = sized(|buf, len| unsafe { libc::llistxattr(at.as_ptr(), buf.cast(), len) })
    else {
        return Ok(Vec::new());
    };
    let mut attrs = Vec::new();
    for name in names.split(|b| *b == 0).filter(|n| !n.is_empty()) {
        let name = CString::new(name)?;
        // SAFETY: as above, with `name` NUL-terminated too.
        if let Some(value) =
            sized(|buf, len| unsafe { libc::lgetxattr(at.as_ptr(), name.as_ptr(), buf, len) })
        {
            attrs.push((name, value));
        }
    }
    Ok(attrs)
}

/// Gives `path` the attributes `attrs`.
pub fn write(path: &Path, attrs: &Attrs) -> Result<()> {
    let at = c_path(path)?;
    for (name, value) in attrs {
        // SAFETY: NUL-terminated path and name; the value's length is its own.
        let done = unsafe {
            libc::lsetxattr(
                at.as_ptr(),
                name.as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
            )
        };
        if done != 0 {
            return Err(std::io::Error::last_os_error()).with_context(|| {
                format!("setting {} on {}", name.to_string_lossy(), path.display())
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attributes_read_back_as_written() {
        let dir = std::env::temp_dir().join(format!("edel-xattrs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b) = (dir.join("a"), dir.join("b"));
        std::fs::write(&a, "a").unwrap();
        std::fs::write(&b, "b").unwrap();
        let attrs = vec![(CString::new("user.edel").unwrap(), b"kept".to_vec())];
        // A file system without user attributes (some tmpfs) has nothing
        // to check.
        if write(&a, &attrs).is_ok() {
            write(&b, &read(&a).unwrap()).unwrap();
            assert_eq!(read(&b).unwrap(), attrs);
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
