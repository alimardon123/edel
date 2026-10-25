//! The encrypted data partition (roadmap M8.2): LUKS2 on partition 4,
//! made by `edel install` and unlocked by `edel boot mount-data`, which
//! asks the passphrase on every console the kernel writes to (the screen
//! and a serial port alike) in the keyboard layout the person installed
//! with. Only the data is encrypted: the slots are signed and public.
//!
//! The layout's console map is named in the LUKS2 header's `subsystem`
//! field (`keymap=de`), which can be read without the passphrase and goes
//! wherever the disk goes. Seam (ADR-006, M11.2): unlocking with the TPM
//! adds a key slot, and the passphrase stays the way in.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;

use anyhow::{Context, Result, bail};

use edel::i18n::{tr, trf};
use edel::password::Password;
use edel::places;

use crate::boot::{CRYPT_LABEL, DATA_LABEL};
use crate::privilege;

/// The tool, from the ab-boot feature's `cryptsetup` package.
const TOOL: &str = "cryptsetup";
/// busybox's console maps, from `kbd-bkeymaps`: `NAME/NAME.bmap.gz` and
/// `NAME/NAME-VARIANT.bmap.gz`.
const KEYMAPS: &str = "/usr/share/bkeymaps";
/// The start of every LUKS header.
const MAGIC: &[u8] = b"LUKS\xba\xbe";
/// Where LUKS2's binary header keeps its 48-byte `subsystem` field: after
/// the magic, version, size, sequence, label, checksum name, salt and uuid.
const SUBSYSTEM_AT: usize = 6 + 2 + 8 + 8 + 48 + 32 + 64 + 40;
const SUBSYSTEM_LEN: usize = 48;
const KEYMAP_PREFIX: &str = "keymap=";
/// argon2id's memory, in KiB: enough to be costly to guess, little enough
/// that a disk moved to a small machine still opens.
const PBKDF_MEMORY_KIB: &str = "262144";
/// cryptsetup's exit code when no key slot takes the passphrase.
const WRONG_PASSPHRASE: i32 = 2;

/// The command, without udev's part, so it never waits on udev for the
/// node it makes itself.
fn cryptsetup() -> Command {
    // device-mapper and its encryption are modules in Alpine's kernel,
    // and udev is not asked for /dev/mapper/control.
    let _ = Command::new("modprobe").arg("dm_crypt").status();
    let mut command = Command::new(TOOL);
    command.env("DM_DISABLE_UDEV", "1");
    command
}

/// The console map for the keyboard setting `keyboard` (xkb's names, such
/// as `de`, `uz(latin)` or `ru,us`): the first Latin layout's map that
/// `exists`, the variant's if there is one, else none for `us` or a layout
/// busybox lacks, which leaves the kernel's own US map.
pub fn keymap_for(keyboard: &str, exists: impl Fn(&str) -> bool) -> Option<String> {
    let layout = keyboard
        .split(',')
        .map(str::trim)
        .find(|l| !edel::install::NON_LATIN.contains(&l.split('(').next().unwrap_or(l)))?;
    let (name, variant) = match layout.split_once('(') {
        Some((name, rest)) => (name, rest.trim_end_matches(')')),
        None => (layout, ""),
    };
    if name == "us" && variant.is_empty() {
        return None;
    }
    [format!("{name}-{variant}"), name.to_string()]
        .into_iter()
        .filter(|map| !variant.is_empty() || map == name)
        .find(|map| exists(&map_path(name, map)))
}

/// Where busybox's map `map` of the layout `name` is.
fn map_path(name: &str, map: &str) -> String {
    format!("{KEYMAPS}/{name}/{map}.bmap.gz")
}

/// Whether `head`, a partition's first bytes, is a LUKS header.
pub fn is_luks_header(head: &[u8]) -> bool {
    head.starts_with(MAGIC)
}

/// The console map a LUKS2 header names, from its first bytes.
pub fn keymap_in(head: &[u8]) -> Option<String> {
    let field = head.get(SUBSYSTEM_AT..SUBSYSTEM_AT + SUBSYSTEM_LEN)?;
    let text = std::str::from_utf8(field.split(|b| *b == 0).next()?).ok()?;
    let map = text.strip_prefix(KEYMAP_PREFIX)?;
    (!map.is_empty()
        && map
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'))
    .then(|| map.to_string())
}

/// The first 4 KiB of `device`.
fn head_of(device: &Path) -> Result<Vec<u8>> {
    let mut head = vec![0; 4096];
    let mut file = File::open(device).with_context(|| format!("reading {}", device.display()))?;
    let read = file.read(&mut head)?;
    head.truncate(read);
    Ok(head)
}

/// Whether `device` holds a LUKS header.
pub fn is_luks(device: &Path) -> Result<bool> {
    Ok(is_luks_header(&head_of(device)?))
}

/// Runs `command` with `passphrase`'s exact bytes on standard input, which
/// is closed after, so cryptsetup's `--key-file -` reads nothing more.
fn with_passphrase(
    command: &mut Command,
    passphrase: &Password,
) -> Result<std::process::ExitStatus> {
    let mut child = command
        .stdin(Stdio::piped())
        .spawn()
        .with_context(|| trf("could not start {tool}", &[("tool", TOOL)]))?;
    let mut stdin = child.stdin.take().context("cryptsetup has no input")?;
    stdin.write_all(passphrase.text().unwrap_or_default().as_bytes())?;
    drop(stdin);
    Ok(child.wait()?)
}

/// Makes LUKS2 on `device` with `passphrase`, naming the console map
/// `keymap` in its header.
pub fn format(device: &Path, passphrase: &Password, keymap: Option<&str>) -> Result<()> {
    let mut command = cryptsetup();
    command.args([
        "luksFormat",
        "--type",
        "luks2",
        "--batch-mode",
        "--label",
        CRYPT_LABEL,
        "--pbkdf",
        "argon2id",
        "--pbkdf-memory",
        PBKDF_MEMORY_KIB,
    ]);
    if let Some(map) = keymap {
        command
            .arg("--subsystem")
            .arg(format!("{KEYMAP_PREFIX}{map}"));
    }
    command.args(["--key-file", "-"]).arg(device);
    if !with_passphrase(&mut command, passphrase)?.success() {
        bail!(
            "{}",
            trf(
                "cryptsetup could not encrypt {device}, so the disk is half written; run edel install again",
                &[("device", &device.display().to_string())]
            )
        );
    }
    Ok(())
}

/// Opens `device` as `/dev/mapper/NAME`: false when the passphrase does not
/// unlock it.
pub fn open(device: &Path, name: &str, passphrase: &Password) -> Result<bool> {
    let status = with_passphrase(
        cryptsetup()
            .args(["open", "--type", "luks2", "--key-file", "-"])
            .arg(device)
            .arg(name),
        passphrase,
    )?;
    match status.code() {
        Some(0) => Ok(true),
        Some(WRONG_PASSPHRASE) => Ok(false),
        _ => bail!(
            "{}",
            trf(
                "cryptsetup could not open {device} ({status})",
                &[
                    ("device", &device.display().to_string()),
                    ("status", &status.to_string()),
                ]
            )
        ),
    }
}

/// The device an open mapping `name` is at.
pub fn mapper(name: &str) -> PathBuf {
    Path::new("/dev/mapper").join(name)
}

/// Closes the mapping `name`, if it is open.
pub fn close(name: &str) {
    if mapper(name).exists() {
        let _ = cryptsetup().arg("close").arg(name).status();
    }
}

/// Closes a mapping when dropped, so a failed install leaves none open.
pub struct Opened(pub String);

impl Drop for Opened {
    fn drop(&mut self) {
        close(&self.0);
    }
}

/// Loads the console map `map` for the screen's consoles; a failure is
/// said and the kernel's US map stays.
fn load_keymap(map: &str) {
    let name = map.split('-').next().unwrap_or(map);
    let loaded = File::open(map_path(name, map))
        .map(flate2::read::GzDecoder::new)
        .and_then(|mut gz| {
            let mut bytes = Vec::new();
            gz.read_to_end(&mut bytes)?;
            let mut child = Command::new("loadkmap").stdin(Stdio::piped()).spawn()?;
            child
                .stdin
                .take()
                .map(|mut i| i.write_all(&bytes))
                .transpose()?;
            child.wait()
        });
    match loaded {
        Ok(status) if status.success() => {}
        Ok(status) => eprintln!(
            "warning: edel-data: loadkmap {map} failed ({status}); the keyboard has the US layout"
        ),
        Err(err) => eprintln!(
            "warning: edel-data: could not load the {map} keyboard map: {err}; the keyboard has the US layout"
        ),
    }
}

/// Names the console map of `keyboard` in the header of the encrypted
/// `device` when it names another, so the next start's prompt types in
/// the layout the machine's file, or else its first person, now uses
/// (M8.2b). Run by `edel boot mount-data` once `/data` and `/etc` are
/// there; a failure is only said.
pub fn follow_keyboard(device: &Path, keyboard: &str) {
    let wanted = keymap_for(keyboard, |path| Path::new(path).exists());
    let Ok(head) = head_of(device) else { return };
    if keymap_in(&head) == wanted {
        return;
    }
    let field = wanted
        .as_deref()
        .map_or(String::new(), |map| format!("{KEYMAP_PREFIX}{map}"));
    let done = cryptsetup()
        .args(["config", "--subsystem", &field])
        .arg(device)
        .status();
    match done {
        Ok(status) if status.success() => println!(
            "edel-data: the passphrase prompt now types in the {} layout",
            wanted.as_deref().unwrap_or("us")
        ),
        _ => eprintln!(
            "warning: edel-data: could not name the {} keyboard map in the data's header; the prompt keeps its layout",
            wanted.as_deref().unwrap_or("us")
        ),
    }
}

/// The consoles the kernel writes to (`/proc/consoles`' first field).
fn consoles(text: &str) -> Vec<String> {
    let mut names: Vec<String> = text
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .filter(|name| name.bytes().all(|b| b.is_ascii_alphanumeric()))
        .map(String::from)
        .collect();
    names.dedup();
    names
}

/// One console: the terminal read from, written to, and its settings
/// before the prompt.
struct Console {
    reader: File,
    writer: File,
    saved: Option<String>,
}

/// Unlocks the encrypted data partition `device` as
/// `/dev/mapper/edel-data`, asking its passphrase on every console until
/// one unlocks it, and returns the unlocked device.
pub fn unlock(device: &Path) -> Result<PathBuf> {
    let unlocked = mapper(DATA_LABEL);
    if unlocked.exists() {
        return Ok(unlocked);
    }
    if let Some(map) = keymap_in(&head_of(device)?) {
        load_keymap(&map);
    }
    let mut open_consoles = Vec::new();
    for name in consoles(&fs::read_to_string("/proc/consoles").unwrap_or_default()) {
        let path = Path::new("/dev").join(&name);
        let reader = File::open(&path);
        let writer = OpenOptions::new().write(true).open(&path);
        if let (Ok(reader), Ok(writer)) = (reader, writer) {
            let saved = privilege::stty(&reader, &["-g"])
                .ok()
                .map(|s| s.trim().to_string());
            open_consoles.push(Console {
                reader,
                writer,
                saved,
            });
        }
    }
    if open_consoles.is_empty() {
        bail!(
            "{}",
            tr("the data partition is encrypted and there is no console to ask its passphrase on")
        );
    }
    let prompt = tr(
        "This computer's data is encrypted. Type its passphrase, the password it was installed with: ",
    );
    let (lines, typed) = mpsc::channel();
    let (mut writers, mut verdicts) = (Vec::new(), Vec::new());
    for (index, console) in open_consoles.iter().enumerate() {
        writers.push(console.writer.try_clone()?);
        let (reader, writer) = (console.reader.try_clone()?, console.writer.try_clone()?);
        let lines = lines.clone();
        // A console asks again only once its last line was tried, so the
        // refusal comes before the next prompt.
        let (verdict, tried) = mpsc::channel::<()>();
        verdicts.push(verdict);
        std::thread::spawn(move || {
            loop {
                let (Ok(reader), Ok(writer)) = (reader.try_clone(), writer.try_clone()) else {
                    return;
                };
                match privilege::read_secret_from(reader, writer, prompt) {
                    Ok(passphrase) if passphrase.letters() > 0 => {
                        if lines.send((index, passphrase)).is_err() || tried.recv().is_err() {
                            return;
                        }
                    }
                    // An empty line, Ctrl-C, or a port nobody is on: ask
                    // again a little later.
                    _ => std::thread::sleep(std::time::Duration::from_secs(1)),
                }
            }
        });
    }
    drop(lines);
    let result = loop {
        // Only the wait for a person stops the guard's clock (M8.2).
        let _ = fs::create_dir_all(places::RUN_DIR);
        let _ = fs::write(places::UNLOCK_WAITING, "");
        let got = typed.recv();
        let _ = fs::remove_file(places::UNLOCK_WAITING);
        let Ok((index, passphrase)) = got else {
            break Err(anyhow::anyhow!(
                "{}",
                tr("no console could take the data's passphrase")
            ));
        };
        match open(device, DATA_LABEL, &passphrase) {
            Ok(true) => break Ok(unlocked.clone()),
            Ok(false) => {
                let line =
                    tr("That passphrase does not unlock this computer's data; type it again.");
                for writer in &mut writers {
                    let _ = writeln!(writer, "{line}");
                }
                let _ = verdicts[index].send(());
            }
            Err(err) => break Err(err),
        }
    };
    // The readers still waiting on the other consoles end with the
    // process; their terminals get echo back now.
    for console in &open_consoles {
        if let Some(saved) = &console.saved {
            let _ = privilege::stty(&console.reader, &[saved.as_str()]);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_keyboard_setting_names_its_console_map() {
        let have = |path: &str| {
            [
                "/usr/share/bkeymaps/de/de.bmap.gz",
                "/usr/share/bkeymaps/uz/uz-latin.bmap.gz",
                "/usr/share/bkeymaps/fr/fr.bmap.gz",
                "/usr/share/bkeymaps/us/us-intl.bmap.gz",
            ]
            .contains(&path)
        };
        assert_eq!(keymap_for("de", have).as_deref(), Some("de"));
        assert_eq!(keymap_for("uz(latin)", have).as_deref(), Some("uz-latin"));
        assert_eq!(keymap_for("ru,fr", have).as_deref(), Some("fr"));
        assert_eq!(keymap_for("us(intl)", have).as_deref(), Some("us-intl"));
        assert_eq!(keymap_for("us", have), None);
        assert_eq!(keymap_for("ru,us", have), None);
        assert_eq!(keymap_for("xx", have), None);
        assert_eq!(keymap_for("de(nope)", have).as_deref(), Some("de"));
    }

    #[test]
    fn the_header_names_its_map_and_luks_is_known_by_its_magic() {
        let mut head = vec![0u8; 4096];
        head[..6].copy_from_slice(MAGIC);
        assert!(is_luks_header(&head));
        assert!(!is_luks_header(&[0x53, 0xef, 0, 0, 0, 0]));
        assert_eq!(keymap_in(&head), None);
        head[SUBSYSTEM_AT..SUBSYSTEM_AT + 9].copy_from_slice(b"keymap=de");
        assert_eq!(keymap_in(&head).as_deref(), Some("de"));
        head[SUBSYSTEM_AT..SUBSYSTEM_AT + 12].copy_from_slice(b"keymap=d/../");
        assert_eq!(keymap_in(&head), None);
    }

    #[test]
    fn the_consoles_come_from_their_first_field() {
        let text =
            "ttyS0                -W- (EC p a)    4:64\ntty0                 -WU (E  p  )    4:1\n";
        assert_eq!(consoles(text), ["ttyS0", "tty0"]);
        assert!(consoles("../x -W-\n").is_empty());
    }
}
