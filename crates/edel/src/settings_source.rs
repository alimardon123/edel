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
        Source::Path(path) => Ok(Local {
            path: PathBuf::from(path),
            shown: location.to_string(),
            downloaded: false,
        }),
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
