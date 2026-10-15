//! A settings file from a path or an `https://` address (roadmap M6.6b):
//! `edel install --settings`, `edel settings import` and `edel settings
//! check` take either, so a fleet keeps one file on a web server and every
//! new machine starts from it. Plain `http://` is refused: a settings file
//! is not signed, and it makes administrators and adds ssh keys, so only a
//! connection that proves its server is taken (ADR-006). The server is
//! checked against the machine's own certificate authorities,
//! `places::CA_BUNDLE`, so an authority a fleet adds there is trusted as a
//! public one is; without that file, the authorities built into the tool.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};

use edel::i18n::trf;
use edel::places;

/// The most a downloaded settings file may hold: a whole machine's file is
/// a few kilobytes, so anything near this is the wrong address.
pub const MOST_BYTES: u64 = 1 << 20;

/// Where a settings file comes from.
#[derive(Debug, PartialEq, Eq)]
enum Source<'a> {
    Path(&'a str),
    Https(&'a str),
}

/// Reads where `location` points: an `https://` address, a `file://` one
/// or a path. Plain `http://` is refused with what to do instead.
fn source_of(location: &str) -> Result<Source<'_>> {
    let lower = location.to_ascii_lowercase();
    if lower.starts_with("https://") {
        Ok(Source::Https(location))
    } else if lower.starts_with("http://") {
        bail!(
            "{}",
            trf(
                "refused: {location} is plain http; a settings file can make administrators and add ssh keys, so give it over https:// or as a file",
                &[("location", location)],
            )
        )
    } else {
        Ok(Source::Path(
            location.strip_prefix("file://").unwrap_or(location),
        ))
    }
}

/// A settings file on this machine: the path given, or a downloaded copy
/// that is removed when this is dropped.
pub struct Local {
    path: PathBuf,
    shown: String,
    downloaded: bool,
}

impl Local {
    /// The file to read.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// What messages call it: the path or the address given.
    pub fn shown(&self) -> &str {
        &self.shown
    }
}

impl Drop for Local {
    fn drop(&mut self) {
        if self.downloaded {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// The settings file at `location` as a file on this machine: a path as
/// it is, an `https://` address downloaded into a file only its owner
/// reads.
pub fn local(location: &str) -> Result<Local> {
    match source_of(location)? {
        Source::Path(path) => {
            readable_by_caller(Path::new(path))?;
            Ok(Local {
                path: PathBuf::from(path),
                shown: location.to_string(),
                downloaded: false,
            })
        }
        Source::Https(address) => {
            let bytes = download(address)?;
            let path =
                std::env::temp_dir().join(format!("edel-settings-{}.toml", std::process::id()));
            write_private(&path, &bytes)?;
            Ok(Local {
                path,
                shown: location.to_string(),
                downloaded: true,
            })
        }
    }
}

/// Refuses, when this runs as root through doas, a file the person who ran
/// doas could not read themselves (M6.17, the review of 2026-10-13): an
/// administrator runs `edel` with no password, and a file read as root
/// shows its lines in a parse error, so `/etc/shadow` would have shown.
fn readable_by_caller(path: &Path) -> Result<()> {
    let Ok(person) = std::env::var(places::AS_ROOT_USER) else {
        return Ok(());
    };
    if edel::users::current() != Some(0) || person.is_empty() {
        return Ok(());
    }
    let read = |file: &str| fs::read_to_string(file).unwrap_or_default();
    let ids = ids_of(
        &person,
        &read(edel::users::PASSWD),
        &read(edel::users::GROUP),
    );
    let shown = path.display().to_string();
    let allowed = ids.is_some_and(|(uid, gids)| may_read(uid, &gids, path));
    if !allowed {
        bail!(
            "{}",
            trf(
                "{person} may not read {path}, and through doas edel reads as root; give a file {person} can read",
                &[("person", &person), ("path", &shown)]
            )
        );
    }
    Ok(())
}

/// The user id and group ids of login `name`: its own group, then every
/// group that lists it, from `/etc/passwd`'s and `/etc/group`'s text.
fn ids_of(name: &str, passwd: &str, group: &str) -> Option<(u32, Vec<u32>)> {
    let fields: Vec<&str> = passwd
        .lines()
        .map(|line| line.split(':').collect::<Vec<_>>())
        .find(|f| f.len() >= 4 && f[0] == name)?;
    let uid = fields[2].parse().ok()?;
    let mut gids: Vec<u32> = fields[3].parse().ok().into_iter().collect();
    for line in group.lines() {
        let f: Vec<&str> = line.split(':').collect();
        if f.len() >= 4 && f[3].split(',').any(|m| m == name) {
            if let Ok(gid) = f[2].parse() {
                gids.push(gid);
            }
        }
    }
    Some((uid, gids))
}

/// Whether the permission bits let `uid` in `gids` take `bit` (4 read, 1
/// search) on a file of `owner`, `group` and `mode`.
fn allows(uid: u32, gids: &[u32], owner: u32, group: u32, mode: u32, bit: u32) -> bool {
    if uid == 0 {
        return true;
    }
    let shift = if owner == uid {
        6
    } else if gids.contains(&group) {
        3
    } else {
        0
    };
    mode >> shift & bit != 0
}

/// Whether `uid` in `gids` may read `path`: every folder on its real path
/// searchable and the file itself readable, by the permission bits.
fn may_read(uid: u32, gids: &[u32], path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let Ok(real) = fs::canonicalize(path) else {
        return false;
    };
    let check = |p: &Path, bit| {
        fs::metadata(p).is_ok_and(|m| allows(uid, gids, m.uid(), m.gid(), m.mode(), bit))
    };
    real.ancestors().skip(1).all(|dir| check(dir, 1)) && check(&real, 4)
}

/// Writes `bytes` to a new file that only its owner may read.
fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    let _ = fs::remove_file(path);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .with_context(|| {
            trf(
                "could not write {path}",
                &[("path", &path.display().to_string())],
            )
        })?;
    file.write_all(bytes)?;
    Ok(())
}

/// Downloads the file at `address`, at most [`MOST_BYTES`].
fn download(address: &str) -> Result<Vec<u8>> {
    let failed = || {
        trf(
            "could not download the settings file {location}; check the network and the address, then try again (nothing was changed)",
            &[("location", address)],
        )
    };
    let mut response = agent().get(address).call().with_context(failed)?;
    response
        .body_mut()
        .with_config()
        .limit(MOST_BYTES)
        .read_to_vec()
        .with_context(failed)
}

/// The HTTP client, trusting the machine's certificate authorities.
fn agent() -> ureq::Agent {
    use ureq::tls::{PemItem, RootCerts, TlsConfig, parse_pem};

    let roots = fs::read(places::CA_BUNDLE)
        .ok()
        .map(|pem| {
            parse_pem(&pem)
                .filter_map(|item| match item {
                    Ok(PemItem::Certificate(cert)) => Some(cert.to_owned()),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .filter(|certs| !certs.is_empty())
        .map_or(RootCerts::WebPki, |certs| RootCerts::new_with_certs(&certs));
    ureq::Agent::config_builder()
        .tls_config(TlsConfig::builder().root_certs(roots).build())
        .timeout_connect(Some(Duration::from_secs(30)))
        .timeout_recv_response(Some(Duration::from_secs(60)))
        .build()
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_is_read_through_doas_only_when_its_caller_could_read_it() {
        // M6.17: `doas edel settings check /etc/shadow` showed its lines.
        let passwd = "root:x:0:0::/root:/bin/sh\nali:x:1000:1000::/home/ali:/bin/sh\n";
        let group = "root:x:0:\nali:x:1000:\nadmin:x:10:ali\nshadow:x:42:\n";
        assert_eq!(ids_of("ali", passwd, group), Some((1000, vec![1000, 10])));
        assert_eq!(ids_of("nobody", passwd, group), None);
        let ali = |owner, group, mode, bit| allows(1000, &[1000, 10], owner, group, mode, bit);
        // /etc/shadow, root's and group shadow's, 0640: not ali's.
        assert!(!ali(0, 42, 0o640, 4));
        // A file of ali's own, a file for the admin group, a public one.
        assert!(ali(1000, 1000, 0o600, 4));
        assert!(ali(0, 10, 0o640, 4));
        assert!(ali(0, 0, 0o644, 4));
        // A folder ali may not search hides what lies in it.
        assert!(!ali(0, 0, 0o700, 1));
        assert!(ali(0, 0, 0o755, 1));
        // Root reads everything.
        assert!(allows(0, &[0], 1000, 1000, 0o000, 4));
        // On this machine: a file the test made is readable by its owner.
        let file = std::env::temp_dir().join(format!("edel-source-read-{}", std::process::id()));
        fs::write(&file, "format = 1\n").unwrap();
        let me = edel::users::current().unwrap();
        assert!(may_read(me, &[], &file));
        assert!(!may_read(me, &[], &file.with_extension("missing")));
        fs::remove_file(&file).unwrap();
    }

    #[test]
    fn https_is_downloaded_and_a_path_or_file_address_read_as_it_is() {
        assert_eq!(
            source_of("https://lab.example/fleet.toml").unwrap(),
            Source::Https("https://lab.example/fleet.toml")
        );
        assert_eq!(
            source_of("HTTPS://lab.example/a.toml").unwrap(),
            Source::Https("HTTPS://lab.example/a.toml")
        );
        assert_eq!(
            source_of("/media/stick/fleet.toml").unwrap(),
            Source::Path("/media/stick/fleet.toml")
        );
        assert_eq!(
            source_of("file:///media/stick/fleet.toml").unwrap(),
            Source::Path("/media/stick/fleet.toml")
        );
    }

    #[test]
    fn plain_http_is_refused_with_what_to_do() {
        let refused = source_of("http://10.0.2.2:8080/install.toml").unwrap_err();
        assert_eq!(
            refused.to_string(),
            "refused: http://10.0.2.2:8080/install.toml is plain http; a settings file can make administrators and add ssh keys, so give it over https:// or as a file"
        );
        assert!(source_of("HTTP://lab.example/a.toml").is_err());
    }

    #[test]
    fn a_path_is_kept_and_never_removed() {
        let dir = std::env::temp_dir().join(format!("edel-settings-path-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("mine.toml");
        fs::write(&file, "format = 1\n").unwrap();
        {
            let local = local(file.to_str().unwrap()).unwrap();
            assert_eq!(local.path(), file);
            assert_eq!(local.shown(), file.to_str().unwrap());
        }
        assert!(file.exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_written_copy_is_its_owners_only() {
        use std::os::unix::fs::PermissionsExt;

        let dir =
            std::env::temp_dir().join(format!("edel-settings-private-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("copy.toml");
        write_private(&file, b"format = 1\n").unwrap();
        assert_eq!(
            fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::remove_dir_all(&dir).unwrap();
    }
}
