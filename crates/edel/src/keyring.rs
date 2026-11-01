//! The keyring that keeps apps' passwords (roadmap M7.18): oo7's Secret
//! Service, which D-Bus starts only when an app first asks, and its Secret
//! portal for Flatpak apps (the `keyring` feature). Each person's keyring
//! sits in their home on the data partition and opens with a key of its
//! own, never their password, so a changed password changes nothing: the
//! key is made once, kept here root's alone, and handed to the person's
//! session while they are logged in.
//!
//! `edel boot keyring` does both halves. Run by PAM as a session opens
//! (`PAM_TYPE=open_session`), as root, it copies the person's key into
//! this boot's folder, readable by them alone; as it closes, it takes the
//! copy out. Run as the person, with no PAM_TYPE, it starts the daemon
//! with the key on its standard input (`--login`), which opens a keyring
//! already on disk, and hands the key over the daemon's PAM socket too,
//! which is the only way oo7 0.6 opens a keyring made for the first time;
//! with no key, the daemon starts locked.
//!
//! The session starts it (`edel::presets::SESSION_START`), before any app
//! can ask: an app that asks while the daemon is still opening its
//! keyring finds it locked and waits for a password prompt the desktop
//! does not draw. D-Bus starts it again only if it stopped.

use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

use edel::places;
use edel::users;

use crate::release::to_hex;

/// `edel boot keyring`: PAM's half or D-Bus's, by `PAM_TYPE`.
pub fn boot() -> Result<()> {
    match std::env::var("PAM_TYPE").as_deref() {
        Ok("open_session") => open(
            &pam_user()?,
            Path::new(places::KEYRING_KEYS),
            Path::new(places::KEYRING_RUN),
        ),
        Ok("close_session") => close(&pam_user()?, Path::new(places::KEYRING_RUN)),
        // Other PAM stages have nothing to do.
        Ok(_) => Ok(()),
        Err(_) => match std::env::var_os(HAND_OVER) {
            Some(socket) => hand_over(Path::new(&socket)),
            None => start(),
        },
    }
}

/// Set to the daemon's PAM socket for the helper that hands the key over.
const HAND_OVER: &str = "EDEL_KEYRING_HAND_OVER";

/// Set for the helper's first step, which starts the second and ends, so
/// the second is nobody's child and nothing is left unreaped.
const HAND_OVER_DETACH: &str = "EDEL_KEYRING_HAND_OVER_DETACH";

/// How long the helper waits for the daemon's PAM socket.
const HAND_OVER_WAIT: std::time::Duration = std::time::Duration::from_secs(10);

/// The person PAM is opening or closing a session for.
fn pam_user() -> Result<String> {
    std::env::var("PAM_USER").context("PAM gave no PAM_USER")
}

/// The person called `name`, when they are a person or a stick's `live`
/// account, rather than a system account (the greeter's has no keyring).
fn person(name: &str) -> Option<users::Person> {
    users::read(Some(places::LIVE_UID))
        .into_iter()
        .find(|p| p.name == name && (p.uid != places::LIVE_UID || name == places::LIVE_USER))
}

/// As a session opens: makes the person's key once in `keys`, then copies
/// it to `run/UID`, owned by them, mode 0600.
fn open(name: &str, keys: &Path, run: &Path) -> Result<()> {
    let Some(person) = person(name) else {
        return Ok(());
    };
    let key = ensure_key(&keys.join(&person.name))?;
    fs::create_dir_all(run).with_context(|| format!("could not make {}", run.display()))?;
    fs::set_permissions(run, fs::Permissions::from_mode(0o711))
        .with_context(|| format!("could not set the mode of {}", run.display()))?;
    let copy = run_file(run, person.uid);
    let new = copy.with_extension("new");
    let _ = fs::remove_file(&new);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&new)
        .with_context(|| format!("could not create {}", new.display()))?;
    file.write_all(key.as_bytes())
        .with_context(|| format!("could not write {}", new.display()))?;
    std::os::unix::fs::chown(&new, Some(person.uid), None)
        .with_context(|| format!("could not give {} to {}", new.display(), person.name))?;
    fs::rename(&new, &copy).with_context(|| format!("could not write {}", copy.display()))
}

/// As a session closes: takes the person's copy out.
fn close(name: &str, run: &Path) -> Result<()> {
    let Some(person) = person(name) else {
        return Ok(());
    };
    match fs::remove_file(run_file(run, person.uid)) {
        Err(why) if why.kind() != std::io::ErrorKind::NotFound => {
            Err(why).context("could not take the keyring's key out")
        }
        _ => Ok(()),
    }
}

/// This boot's copy of the key of the person with `uid`.
fn run_file(run: &Path, uid: u32) -> PathBuf {
    run.join(uid.to_string())
}

/// The key at `path`, 32 random bytes in hex, made with its folder (mode
/// 0700) when it is not there yet. A key is never replaced: the keyring
/// was made with it.
fn ensure_key(path: &Path) -> Result<String> {
    if let Ok(text) = fs::read_to_string(path) {
        let key = text.trim();
        if key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Ok(key.to_string());
        }
        bail!(
            "{} is not a keyring key; it is kept for a person to look at",
            path.display()
        );
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).with_context(|| format!("could not make {}", dir.display()))?;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
            .with_context(|| format!("could not set the mode of {}", dir.display()))?;
    }
    let mut bytes = [0u8; 32];
    fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .context("could not read random numbers from /dev/urandom")?;
    let key = to_hex(&bytes);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("could not create {}", path.display()))?;
    file.write_all(format!("{key}\n").as_bytes())
        .and_then(|()| file.sync_all())
        .with_context(|| format!("could not write {}", path.display()))?;
    Ok(key)
}

/// As the person, from the session or D-Bus: becomes the daemon, given
/// the key when the session has one, with a helper to hand it over the
/// daemon's PAM socket as well.
fn start() -> Result<()> {
    let uid = users::current().context("could not tell who is running this")?;
    let copy = run_file(Path::new(places::KEYRING_RUN), uid);
    let socket = std::env::var_os("XDG_RUNTIME_DIR")
        .map(|dir| Path::new(&dir).join(edel::base::KEYRING_PAM_SOCKET));
    let mut daemon = Command::new(edel::base::KEYRING_DAEMON);
    if let Some(socket) = &socket {
        daemon.env("OO7_PAM_SOCKET", socket);
    }
    match fs::File::open(&copy) {
        Ok(key) => {
            daemon.arg("--login").stdin(key);
            if let Some(socket) = &socket {
                // The first step returns at once, the second waiting on.
                let helper = std::env::current_exe().and_then(|edel| {
                    Command::new(edel)
                        .args(["boot", "keyring"])
                        .env(HAND_OVER, socket)
                        .env(HAND_OVER_DETACH, "1")
                        .stdin(Stdio::null())
                        .status()
                });
                if let Err(why) = helper {
                    eprintln!(
                        "edel: could not start the keyring's helper, so a new keyring starts locked: {why}"
                    );
                }
            }
        }
        Err(_) => eprintln!(
            "edel: this session has no keyring key at {}, so the keyring starts locked",
            copy.display()
        ),
    }
    let why = daemon.exec();
    Err(why).with_context(|| format!("could not start {}", edel::base::KEYRING_DAEMON))
}

/// The helper: hands this session's key to the daemon listening at
/// `socket`, as oo7's own PAM module does, once it listens.
fn hand_over(socket: &Path) -> Result<()> {
    if std::env::var_os(HAND_OVER_DETACH).is_some() {
        let edel = std::env::current_exe().context("could not find edel itself")?;
        Command::new(edel)
            .args(["boot", "keyring"])
            .env_remove(HAND_OVER_DETACH)
            .stdin(Stdio::null())
            .spawn()
            .context("could not start the keyring's helper")?;
        return Ok(());
    }
    let uid = users::current().context("could not tell who is running this")?;
    let copy = run_file(Path::new(places::KEYRING_RUN), uid);
    let key = fs::read(&copy).with_context(|| format!("could not read {}", copy.display()))?;
    let name = users::read(Some(uid))
        .into_iter()
        .find(|p| p.uid == uid)
        .map(|p| p.name)
        .unwrap_or_default();
    let message = unlock_message(&name, &key);
    let until = std::time::Instant::now() + HAND_OVER_WAIT;
    loop {
        match std::os::unix::net::UnixStream::connect(socket) {
            Ok(mut stream) => {
                return stream
                    .write_all(&message)
                    .with_context(|| format!("could not hand the key to {}", socket.display()));
            }
            Err(why) if std::time::Instant::now() >= until => {
                return Err(why).with_context(|| {
                    format!(
                        "the keyring did not listen at {} within {} s, so a new keyring stays locked",
                        socket.display(),
                        HAND_OVER_WAIT.as_secs()
                    )
                });
            }
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(50)),
        }
    }
}

/// oo7's PAM message to unlock with `key`: a little-endian length, then
/// the D-Bus serialised struct of the operation (0, unlock), the person's
/// name, an empty old secret and the new one, each field on its D-Bus
/// alignment.
fn unlock_message(name: &str, key: &[u8]) -> Vec<u8> {
    fn pad(body: &mut Vec<u8>) {
        while body.len() % 4 != 0 {
            body.push(0);
        }
    }
    fn length(body: &mut Vec<u8>, n: usize) {
        pad(body);
        body.extend_from_slice(&(n as u32).to_le_bytes());
    }
    let mut body = vec![0u8];
    length(&mut body, name.len());
    body.extend_from_slice(name.as_bytes());
    body.push(0);
    length(&mut body, 0);
    length(&mut body, key.len());
    body.extend_from_slice(key);
    let mut message = (body.len() as u32).to_le_bytes().to_vec();
    message.extend(body);
    message
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("edel-keyring-{test}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_key_is_made_once_and_kept() {
        let dir = scratch("once");
        let path = dir.join("keys").join("ci");
        let first = ensure_key(&path).unwrap();
        assert_eq!(first.len(), 64);
        assert_eq!(ensure_key(&path).unwrap(), first);
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let folder = fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(folder, 0o700);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_damaged_key_is_refused_and_kept() {
        let dir = scratch("damaged");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ci");
        fs::write(&path, "half").unwrap();
        assert!(ensure_key(&path).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "half");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_unlock_message_is_oo7s_pam_message() {
        let message = unlock_message("ci", b"key");
        let mut want = vec![
            23, 0, 0, 0, // the body's length
            0, 0, 0, 0, // unlock, padded to 4
            2, 0, 0, 0, b'c', b'i', 0, 0, // the name and its NUL, padded
            0, 0, 0, 0, // no old secret
            3, 0, 0, 0, // the new secret's length
        ];
        want.extend_from_slice(b"key");
        assert_eq!(message, want);
    }

    #[test]
    fn nobody_unknown_gets_a_key() {
        let dir = scratch("unknown");
        open("no-such-person-here", &dir.join("keys"), &dir.join("run")).unwrap();
        assert!(!dir.exists());
        close("no-such-person-here", &dir.join("run")).unwrap();
    }
}
