//! This device's own identity key (roadmap M6.12, ADR-011's seam for
//! pairing devices, M7.10b, and passkeys, M9.11): an ed25519 key pair made
//! once, at the first boot, on the data partition, so it survives updates
//! and rollbacks and never leaves the device. Nothing reads it yet.

use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use anyhow::{Context, Result};
use ed25519_dalek::SigningKey;

use crate::release::{fixed, from_hex, to_hex};

/// Makes the key pair at `secret` (hex seed, mode 0600) and `public` (hex
/// public key, mode 0644) when `secret` is not there yet, making their
/// folder. Gives whether it made them; a key already there is never
/// replaced.
pub fn ensure(secret: &Path, public: &Path) -> Result<bool> {
    if secret.exists() {
        match read_seed(secret) {
            Ok(seed) => {
                // A public half lost after the secret was made is written
                // again from the secret, so the two always match.
                if !public.exists() {
                    write_file(public, &public_text(&seed), 0o644)?;
                }
                return Ok(false);
            }
            // A secret cut short by a power cut opens nothing: kept aside,
            // never deleted, and a new pair made in its place.
            Err(why) => {
                let aside = secret.with_extension("key.damaged");
                fs::rename(secret, &aside)
                    .with_context(|| format!("could not move {} aside", secret.display()))?;
                let _ = fs::remove_file(public);
                eprintln!(
                    "edel: {why:#}, so it is kept as {} and this device gets a new identity key",
                    aside.display()
                );
            }
        }
    }
    let mut seed = [0u8; 32];
    let mut urandom = fs::File::open("/dev/urandom").context("could not open /dev/urandom")?;
    urandom
        .read_exact(&mut seed)
        .context("could not read the random numbers from /dev/urandom")?;
    write_pair(secret, public, seed)?;
    Ok(true)
}

/// Writes the secret, then the public key, both made from `seed`. The
/// secret is created new, so a key that appears meanwhile is an error,
/// never replaced.
fn write_pair(secret: &Path, public: &Path, seed: [u8; 32]) -> Result<()> {
    write_file(secret, &format!("{}\n", to_hex(&seed)), 0o600)?;
    write_file(public, &public_text(&seed), 0o644)
}

/// The public key of `seed` in hex, with a newline.
fn public_text(seed: &[u8; 32]) -> String {
    let key = SigningKey::from_bytes(seed);
    format!("{}\n", to_hex(key.verifying_key().as_bytes()))
}

/// Reads the seed from a secret key file: 32 bytes as hex.
fn read_seed(secret: &Path) -> Result<[u8; 32]> {
    let text = fs::read_to_string(secret)
        .with_context(|| format!("could not read {}", secret.display()))?;
    let bytes =
        from_hex(&text).with_context(|| format!("{} is not a hex key", secret.display()))?;
    fixed::<32>(bytes, "the device key")
}

/// Creates `path` with `text` and `mode`, making its folder first. Never
/// replaces a file that is there.
fn write_file(path: &Path, text: &str, mode: u32) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).with_context(|| format!("could not make {}", dir.display()))?;
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(path)
        .with_context(|| format!("could not create {}", path.display()))?;
    file.write_all(text.as_bytes())
        .and_then(|()| file.sync_all())
        .with_context(|| format!("could not write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    use super::*;

    /// A folder named after the test and this process, emptied first. The
    /// tests of one process run in parallel threads, so each has its own.
    fn scratch(test: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("edel-device-key-{test}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn makes_the_key_once_and_never_replaces_it() {
        let dir = scratch("makes-once");
        let (secret, public) = (dir.join("edel/device.key"), dir.join("edel/device.pub"));
        assert!(ensure(&secret, &public).unwrap());
        assert!(secret.is_file() && public.is_file());
        let mode = fs::metadata(&secret).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let text = fs::read_to_string(&secret).unwrap();
        assert_eq!(text.len(), 65);
        assert!(text.ends_with('\n'));
        assert!(text.trim_end().bytes().all(|b| b.is_ascii_hexdigit()));
        assert!(!ensure(&secret, &public).unwrap());
        assert_eq!(fs::read_to_string(&secret).unwrap(), text);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_public_key_is_the_secret_keys_own() {
        let dir = scratch("public-is-own");
        let (secret, public) = (dir.join("device.key"), dir.join("device.pub"));
        write_pair(&secret, &public, [7u8; 32]).unwrap();
        let want = format!(
            "{}\n",
            to_hex(
                SigningKey::from_bytes(&[7u8; 32])
                    .verifying_key()
                    .as_bytes()
            )
        );
        assert_eq!(fs::read_to_string(&public).unwrap(), want);
        assert_eq!(
            fs::read_to_string(&secret).unwrap(),
            format!("{}\n", to_hex(&[7u8; 32]))
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_missing_public_half_is_written_again_from_the_secret() {
        let dir = scratch("public-again");
        let (secret, public) = (dir.join("device.key"), dir.join("device.pub"));
        assert!(ensure(&secret, &public).unwrap());
        let before = fs::read_to_string(&public).unwrap();
        fs::remove_file(&public).unwrap();
        assert!(!ensure(&secret, &public).unwrap());
        assert_eq!(fs::read_to_string(&public).unwrap(), before);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_secret_cut_short_is_kept_aside_and_made_again() {
        let dir = scratch("cut-short");
        let (secret, public) = (dir.join("device.key"), dir.join("device.pub"));
        fs::create_dir_all(&dir).unwrap();
        fs::write(&secret, "").unwrap();
        assert!(ensure(&secret, &public).unwrap());
        assert!(read_seed(&secret).is_ok());
        assert!(public.exists());
        assert_eq!(
            fs::read_to_string(dir.join("device.key.damaged")).unwrap(),
            ""
        );
        fs::remove_dir_all(dir).unwrap();
    }
}
