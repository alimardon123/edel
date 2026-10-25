//! `edel install DISK --settings FILE` (roadmap M2.4): makes a blank or old
//! disk an Edel OS machine. It lays out the disk as images are laid out
//! (`boot.rs`) with slots of `SLOT_MIB` (M3.3b), copies the running slot
//! into slot A unchanged, byte for byte, as it was signed (M1.12), writes the EFI system partition from the slot's own boot loader with an
//! initial environment block, and creates the data partition holding the
//! settings file. It never writes the disk it runs from. The plan it shows is
//! `edel::install::Plan`.

use std::fs::{self, File};
use std::io::{self, BufRead, IsTerminal, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread::sleep;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use edel::base;
use edel::fleet;
use edel::i18n::{n_, tr, trf};
use edel::install::{
    Answers, Disk, ESP_MIB, MIN_DATA_MIB, Partition, Plan, SLOT_MIB, asked, is_candidate,
    list_entry, parse_blkid, suggested_hostname, suggested_keyboard,
};
use edel::password::Password;
use edel::settings;

use crate::boot::{self, DATA_LABEL, ESP_LABEL, GRUB_PREFIX, Layout};
use crate::loader::SLOT_DIR;
use crate::privilege;
use crate::update::{self, Lock, run};

/// The external tools a real install runs, and the package each comes
/// from; busybox's `mount`, `umount`, `blkid` and `tar` (which copies the
/// default apps, M6.2) come with the base. A test
/// holds `images/edel.toml` to this list.
pub const TOOLS: &[(&str, &str)] = &[
    ("sfdisk", "sfdisk"),
    ("mkfs.vfat", "dosfstools"),
    ("mkfs.ext4", "e2fsprogs"),
    ("e2fsck", "e2fsprogs"),
    ("cryptsetup", "cryptsetup"),
];

/// The name the new disk's data has while `edel install` writes it
/// encrypted (M8.2), apart from a running machine's own.
const INSTALL_MAPPER: &str = "edel-install-data";

/// Whether to encrypt the data partition (M8.2): `--encrypt`,
/// `--no-encrypt`, or neither, which is the machine's default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encrypt {
    Default,
    On,
    Off,
}

/// Whether the data is encrypted, and why not when the default would
/// have (`edel::install::encrypts_by_default`), which needs the password
/// the install gives to encrypt with.
fn encrypt_data(
    asked: Encrypt,
    form: &str,
    screen: bool,
    given: bool,
) -> (bool, Option<&'static str>) {
    match asked {
        Encrypt::Off => (false, None),
        Encrypt::On => (true, None),
        Encrypt::Default if edel::install::encrypts_by_default(form, screen) => {
            if given {
                (true, None)
            } else {
                (
                    false,
                    Some(n_(
                        "the data is not encrypted, as the install gives nobody a password to encrypt it with; --encrypt asks for a passphrase",
                    )),
                )
            }
        }
        Encrypt::Default => (false, None),
    }
}

/// Asks the data's passphrase twice on the terminal, or reads one line of
/// standard input when there is no terminal.
fn ask_passphrase() -> Result<Password> {
    if !io::stdin().is_terminal() {
        let line = privilege::read_password_line(io::stdin().lock()).context(tr(
            "could not read the data's passphrase from standard input; give it on one line",
        ))?;
        if line.letters() == 0 {
            bail!(
                "{}",
                tr(
                    "no passphrase for the data on standard input, so nothing was changed; give it on one line, run edel install in a terminal, or leave out --encrypt"
                )
            );
        }
        return Ok(line);
    }
    loop {
        let first = privilege::read_secret(tr("Passphrase for the data: "))?;
        if first.letters() == 0 {
            println!("  {}", tr("a passphrase cannot be empty"));
            continue;
        }
        let again = privilege::read_secret(tr("The same passphrase again: "))?;
        if first.text() == again.text() {
            return Ok(first);
        }
        println!("  {}", tr("the two passphrases differ; type them again"));
    }
}

const MIB: u64 = 1024 * 1024;
/// The first block of a slot: it holds the ext4 superblock and its UUID.
const HEAD: u64 = 4096;
/// What is said when the plan was shown and nobody could confirm it.
const NO_ANSWER: &str = n_(
    "edel install: no terminal to confirm on and no --yes, so nothing was changed; run it in a terminal, or read the plan above and add --yes",
);

/// Why a laptop's data stays plain: its password cannot be typed at the
/// start's prompt.
const NOT_TYPABLE: &str = n_(
    "the data is not encrypted, as the password has letters other than a-z, A-Z, digits and signs, which the start's prompt may not be able to type",
);
/// What an encrypted install without a password asks once confirmed.
const PASSPHRASE_ASKED: &str = n_(
    "a passphrase for the data is asked once the plan is confirmed (or read as one line of standard input)",
);

/// Exit code when the plan was shown but nobody could confirm it.
const NOT_CONFIRMED: i32 = 3;

/// The tools in `TOOLS` this system lacks, so install can stop before it
/// erases anything.
fn missing_tools() -> Vec<String> {
    let path = std::env::var("PATH").unwrap_or_else(|_| "/usr/sbin:/usr/bin:/sbin:/bin".into());
    TOOLS
        .iter()
        .filter(|(tool, _)| {
            !path
                .split(':')
                .any(|dir| Path::new(dir).join(tool).exists())
        })
        .map(|(tool, package)| format!("{tool} (package {package})"))
        .collect()
}

/// `/dev/sda` and `sda` both name the disk `sda`.
fn disk_name(disk: &str) -> &str {
    disk.trim_start_matches("/dev/")
}

/// The kernel name of partition `number` of `disk`: `sda1`, `nvme0n1p1`.
fn partition_name(disk: &str, number: u32) -> String {
    if disk.ends_with(|c: char| c.is_ascii_digit()) {
        format!("{disk}p{number}")
    } else {
        format!("{disk}{number}")
    }
}

/// Whether a part of disk `name` is in use: mounted (`mounts`, as
/// `/proc/mounts` reads), swapped on (`swaps`, as `/proc/swaps`), or held
/// open below something else (an unlocked encrypted partition, a RAID),
/// which `sys`'s `holders` folders list and neither file names.
fn in_use(sys: &Path, mounts: &str, swaps: &str, name: &str) -> bool {
    let ours = |device: &str| {
        device
            .strip_prefix("/dev/")
            .and_then(|d| d.strip_prefix(name))
            .is_some_and(|rest| {
                let number = rest
                    .strip_prefix('p')
                    .filter(|_| name.ends_with(|c: char| c.is_ascii_digit()))
                    .unwrap_or(rest);
                rest.is_empty()
                    || (!number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()))
            })
    };
    let listed = |text: &str| {
        text.lines()
            .filter_map(|l| l.split_whitespace().next())
            .any(ours)
    };
    let block = sys.join("class/block");
    let held = |part: &str| {
        fs::read_dir(block.join(part).join("holders")).is_ok_and(|mut d| d.next().is_some())
    };
    let parts = fs::read_dir(sys.join("block").join(name))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|p| ours(&format!("/dev/{p}")));
    listed(mounts) || listed(swaps) || held(name) || parts.into_iter().any(|p| held(&p))
}

fn read_number(path: &Path) -> Result<u64> {
    let text = fs::read_to_string(path).with_context(|| could_not_read(path))?;
    Ok(text.trim().parse()?)
}

/// `could not read PATH`, the start of a failure to read a file.
fn could_not_read(path: &Path) -> String {
    trf(
        "could not read {path}",
        &[("path", &path.display().to_string())],
    )
}

/// `could not write PATH`, the start of a failure to write a file.
fn could_not_write(path: &Path) -> String {
    trf(
        "could not write {path}",
        &[("path", &path.display().to_string())],
    )
}

/// The target disk as sysfs and blkid see it.
fn describe(name: &str) -> Result<Disk> {
    let sys = Path::new("/sys/block").join(name);
    if !sys.exists() {
        bail!(
            "{}",
            trf(
                "/dev/{name} is not a whole disk; name a disk such as /dev/sda, not a partition",
                &[("name", name)]
            )
        );
    }
    let model = fs::read_to_string(sys.join("device/model"))
        .ok()
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty());
    let mut partitions = Vec::new();
    for entry in fs::read_dir(&sys)? {
        let path = entry?.path();
        if !path.join("partition").exists() {
            continue;
        }
        let part = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let out = Command::new("blkid")
            .arg(Path::new("/dev").join(&part))
            .output();
        let line = out
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default();
        let (mut label, mut fs) = parse_blkid(line.trim());
        // blkid says nothing about a partition it may not open, which is
        // not the same as an empty one (a dry run needs no root).
        if line.trim().is_empty() && File::open(Path::new("/dev").join(&part)).is_err() {
            label = Some(tr("unknown").into());
            fs = Some(tr("doas edel install --list shows it").into());
        }
        partitions.push(Partition {
            name: part,
            label,
            fs,
            bytes: read_number(&path.join("size"))? * 512,
        });
    }
    partitions.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Disk {
        name: name.to_string(),
        model,
        bytes: read_number(&sys.join("size"))? * 512,
        partitions,
    })
}

/// Whether the disk `name` is removable or on USB, as a stick is; a USB
/// stick that tells the kernel it is fixed still counts, so `disk = "auto"`
/// never takes it.
fn removable(name: &str) -> bool {
    edel::install::is_removable(Path::new("/sys"), name)
}

/// The disks the installer could offer, as `/sys/block` lists them, each
/// described as the plan describes it.
fn candidates() -> Result<Vec<Disk>> {
    let mut names: Vec<String> = fs::read_dir("/sys/block")
        .with_context(|| could_not_read(Path::new("/sys/block")))?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| is_candidate(n))
        .collect();
    names.sort();
    names.iter().map(|n| describe(n)).collect()
}

/// `edel install --list` (M6.6a): every disk the installer could use, the
/// one Edel OS runs from marked and never offered.
pub fn list() -> Result<()> {
    let running = update::Disk::find().ok().map(|d| d.name);
    let disks = candidates()?;
    if disks.is_empty() {
        println!("{}", tr("no disk found to install on"));
    }
    for disk in &disks {
        let here = running.as_deref() == Some(disk.name.as_str());
        println!("{}\n", list_entry(disk, removable(&disk.name), here));
    }
    Ok(())
}

/// `edel install --find-settings` (M6.6b): the settings file to install
/// with, from another disk: each partition (or a disk with none, as a FAT
/// image written whole) not mounted and not on the disk Edel OS runs from,
/// mounted read-only in turn, holding the settings file
/// at its top or in `EFI/edel` (`install::file_in`). The first one found is
/// printed on standard output and where it was on standard error, so
/// Settings' installer page can read both; nothing is changed.
pub fn find_settings() -> Result<()> {
    let running = update::Disk::find().ok().map(|d| d.name);
    let mounts = fs::read_to_string("/proc/mounts").unwrap_or_default();
    let mut disks: Vec<String> = fs::read_dir("/sys/block")
        .with_context(|| could_not_read(Path::new("/sys/block")))?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| is_candidate(n) && running.as_deref() != Some(n.as_str()))
        .collect();
    disks.sort();
    for disk in disks {
        let mut devices: Vec<String> = fs::read_dir(Path::new("/sys/block").join(&disk))
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .filter(|e| e.path().join("partition").exists())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        devices.sort();
        if devices.is_empty() {
            devices.push(disk.clone());
        }
        for name in devices {
            let device = Path::new("/dev").join(&name);
            let shown = device.display().to_string();
            if mounts
                .lines()
                .any(|l| l.split_whitespace().next() == Some(shown.as_str()))
            {
                continue;
            }
            let found = crate::machine::with_mounted(&device, None, true, |dir| {
                edel::install::file_in(dir).map(|(path, text)| {
                    let inner = path
                        .strip_prefix(dir)
                        .unwrap_or(&path)
                        .display()
                        .to_string();
                    (inner, text)
                })
            });
            if let Some(Some((inner, text))) = found {
                eprintln!(
                    "edel install: {}",
                    trf(
                        "found {file} on {device}",
                        &[("file", &inner), ("device", &shown)]
                    )
                );
                print!("{text}");
                return Ok(());
            }
        }
    }
    bail!(
        "{}",
        trf(
            "no settings file was found on another disk; put {file} at the top of a USB stick, plug it in and look again",
            &[("file", edel::places::SETTINGS)]
        )
    )
}

/// One answer typed on the terminal after `prompt`; an empty line takes
/// `suggestion` when there is one.
fn ask_line(prompt: &str, suggestion: Option<&str>) -> Result<String> {
    match suggestion {
        Some(s) => print!("{prompt} [{s}]: "),
        None => print!("{prompt}: "),
    }
    io::stdout().flush()?;
    let mut line = String::new();
    if io::stdin().lock().read_line(&mut line)? == 0 {
        bail!(
            "{}",
            tr("the questions were not answered; nothing was changed")
        );
    }
    let line = line.trim();
    Ok(match (line.is_empty(), suggestion) {
        (true, Some(s)) => s.to_string(),
        _ => line.to_string(),
    })
}

/// Asks until `valid` takes the answer, saying why it did not.
pub(crate) fn ask_until(
    prompt: &str,
    suggestion: Option<&str>,
    valid: impl Fn(&str) -> std::result::Result<String, String>,
) -> Result<String> {
    loop {
        match valid(&ask_line(prompt, suggestion)?) {
            Ok(answer) => return Ok(answer),
            Err(why) => println!("  {why}"),
        }
    }
}

/// The password, asked twice with the echo off until both agree and it is
/// not empty.
fn ask_password() -> Result<Password> {
    loop {
        let first = privilege::read_secret(tr("Your password: "))?;
        if first.letters() == 0 {
            println!("  {}", tr("a password cannot be empty"));
            continue;
        }
        let again = privilege::read_secret(tr("The same password again: "))?;
        if first.text() == again.text() {
            return Ok(first);
        }
        println!("  {}", tr("the two passwords differ; type them again"));
    }
}

/// Hashes `password` as `/etc/shadow` keeps it (SHA-512 crypt), with
/// the base's hasher reading it on standard input, so it is never on a
/// command line.
pub(crate) fn hash(password: &Password) -> Result<String> {
    let tool = [("tool", base::PASSWORD_HASHER)];
    let mut child = base::hash_password()
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .with_context(|| trf("could not start {tool}, which hashes the password", &tool))?;
    child
        .stdin
        .take()
        .with_context(|| trf("{tool} has no input", &tool))?
        .write_all(password.text().unwrap_or_default().as_bytes())?;
    let out = child.wait_with_output()?;
    let hash = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() || !hash.starts_with("$6$") {
        bail!(
            "{}",
            trf(
                "the password could not be prepared for the new disk ({tool} failed), so nothing was changed; this stick may be damaged: write it again from the download",
                &tool
            )
        );
    }
    Ok(hash)
}

/// The questions on the terminal (M6.6b), in `install::QUESTIONS`' order,
/// up to the disk, which the caller asks; More options only when asked
/// for. Returns the answers and the password's hash.
fn ask_questions() -> Result<(Answers, Option<Password>)> {
    println!(
        "{}\n",
        tr(
            "Edel OS asks a few questions, then shows the plan before it changes anything. Press Return to take the suggestion in brackets."
        )
    );
    let mut answers = Answers::default();
    let mut password = None;
    let questions = asked();
    let more = ask_line(
        tr("More options (computer name, layout)? yes or no"),
        Some("no"),
    )?
    .starts_with('y');
    for q in questions {
        if q.more && !more {
            continue;
        }
        let title = tr(q.title);
        match q.key {
            "region.language" => {
                answers.language = Some(ask_until(
                    title,
                    Some("en_US.UTF-8"),
                    edel::install::language_answer,
                )?)
            }
            "region.keyboard" => {
                let suggestion = suggested_keyboard(answers.language.as_deref().unwrap_or("C"));
                let rules = fs::read_to_string(edel::keyboard::RULES).ok();
                answers.keyboard = Some(ask_until(title, Some(&suggestion), |v| {
                    edel::keyboard::normalize(v, rules.as_deref())
                })?)
            }
            "region.timezone" => {
                let suggestion =
                    edel::timezones::suggested(answers.language.as_deref().unwrap_or("C"));
                answers.timezone =
                    Some(ask_until(title, Some(suggestion), edel::timezones::answer)?)
            }
            "user" => answers.user = Some(ask_until(title, None, edel::install::user_answer)?),
            "password" => password = Some(ask_password()?),
            "network.hostname" => {
                let suggestion = suggested_hostname(
                    answers.user.as_deref().unwrap_or("edel"),
                    edel::install::form_factor(Path::new("/sys")),
                );
                answers.hostname = Some(ask_until(
                    title,
                    Some(&suggestion),
                    edel::install::hostname_answer,
                )?)
            }
            "layout.preset" => {
                let names = settings::PRESETS.join(", ");
                answers.preset = Some(ask_until(
                    &format!("{title} ({names})"),
                    Some("classic"),
                    |v| {
                        if settings::PRESETS.contains(&v) {
                            Ok(v.to_string())
                        } else {
                            Err(trf("one of {names}", &[("names", &names)]))
                        }
                    },
                )?)
            }
            "system.profiles" => {
                let names = edel::profiles::NAMES.join(", ");
                answers.profile = Some(ask_until(
                    &format!("{title} ({}, {names})", edel::install::NO_PROFILE),
                    Some(edel::install::NO_PROFILE),
                    edel::install::profile_answer,
                )?)
            }
            _ => {}
        }
    }
    Ok((answers, password))
}

/// Asks which disk, after showing the list; the running disk is refused
/// later, as a disk named on the command line is.
fn ask_disk() -> Result<String> {
    list()?;
    ask_until(tr("Disk to install on, such as sda"), None, |v| {
        let name = disk_name(v);
        if is_candidate(name) && Path::new("/sys/block").join(name).exists() {
            Ok(name.to_string())
        } else {
            Err(tr("one of the disks listed above, such as sda").to_string())
        }
    })
}

/// `edel install [DISK] [--settings FILE]`: without a settings file, the
/// questions are asked on the terminal and their answers become one
/// (M6.6b); without a disk, the list is shown and the disk asked.
pub fn install(
    disk: Option<&str>,
    system_file: Option<&Path>,
    dry_run: bool,
    yes: bool,
    new_password: bool,
    encrypt: Encrypt,
) -> Result<()> {
    let asking = system_file.is_none() || disk.is_none();
    if asking && !io::stdin().is_terminal() {
        bail!(
            "{}",
            tr(
                "edel install asks its questions on a terminal; run it in one, use Install Edel OS in Settings, or name the disk and give --settings FILE"
            )
        );
    }
    // A plan reads no password: it only says one will come.
    let (written, password) = match system_file {
        Some(file) if new_password && !dry_run => {
            let (person, password) = password_from_stdin(file)?;
            (file.to_path_buf(), Some((person, password)))
        }
        Some(file) => (file.to_path_buf(), None),
        None => {
            let (answers, password) = ask_questions()?;
            let text = answers.settings_text()?;
            let file = PathBuf::from(edel::places::INSTALL_ANSWERS);
            edel::settings::write_text(&file, &text)?;
            (
                file,
                password.map(|p| (answers.user.unwrap_or_default(), p)),
            )
        }
    };
    let hashed = match &password {
        Some((person, clear)) => Some((person.clone(), hash(clear)?)),
        None => None,
    };
    let data = Data {
        asked: encrypt,
        given: new_password || password.is_some(),
        password: password.map(|(_, clear)| clear),
    };
    let disk = match disk {
        Some(d) => d.to_string(),
        None => ask_disk()?,
    };
    install_on(&disk, &written, hashed.as_ref(), data, dry_run, yes)
}

/// What the person asked of the data partition, and the password the
/// install gives, which encrypts it on the machines whose default that is.
struct Data {
    asked: Encrypt,
    /// Whether the install gives a password (a plan only says it will).
    given: bool,
    password: Option<Password>,
}

/// The one person of `file` and the hash of the password standard input
/// gives them (`--new-password-stdin`, M6.6a): Settings' installer page asks
/// the password and hands it over this way, never on a command line.
fn password_from_stdin(file: &Path) -> Result<(String, Password)> {
    let text = fs::read_to_string(file).with_context(|| could_not_read(file))?;
    let people: Vec<String> = settings::read(&text)?.file.users.into_keys().collect();
    let [person] = people.as_slice() else {
        bail!(
            "{}",
            trf(
                "--new-password-stdin gives one person a password, and {path} names {count}; name one person in it",
                &[
                    ("path", &file.display().to_string()),
                    ("count", &people.len().to_string()),
                ]
            )
        );
    };
    let password = privilege::read_password_line(io::stdin().lock())?;
    if password.letters() == 0 {
        bail!(
            "{}",
            tr("the new password on standard input is empty; nothing was changed")
        );
    }
    Ok((person.clone(), password))
}

/// Installs on `disk` with the settings file `system_file` and, when the
/// questions asked one, the person's password hash.
fn install_on(
    disk: &str,
    system_file: &Path,
    password: Option<&(String, String)>,
    data: Data,
    dry_run: bool,
    yes: bool,
) -> Result<()> {
    let name = disk_name(disk);
    let running = update::Disk::find()?;
    if name == running.name {
        bail!(
            "{}",
            trf(
                "/dev/{name} is the disk this system runs from; install to another disk",
                &[("name", name)]
            )
        );
    }
    let mounts = fs::read_to_string("/proc/mounts")?;
    let swaps = fs::read_to_string("/proc/swaps").unwrap_or_default();
    if in_use(Path::new("/sys"), &mounts, &swaps, name) {
        bail!(
            "{}",
            trf(
                "/dev/{name} is in use, as one of its parts is open (in Files, say, or unlocked, or used for swap); eject or close it there, or choose another disk; nothing was changed",
                &[("name", name)]
            )
        );
    }
    let text = fs::read_to_string(system_file).with_context(|| could_not_read(system_file))?;
    let problems = settings::check(&text).with_context(|| {
        trf(
            "could not check {path}",
            &[("path", &system_file.display().to_string())],
        )
    })?;
    if !problems.is_empty() {
        bail!(
            "{}\n{}",
            trf(
                "{path} has problems, so nothing was changed; fix these (edel settings check shows them again):",
                &[("path", &system_file.display().to_string())]
            ),
            problems.join("\n")
        );
    }
    let slot = running.running_device()?;
    let slot_name = slot
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let running_mib =
        read_number(&Path::new("/sys/class/block").join(&slot_name).join("size"))? * 512 / MIB;
    let (encrypted, why_plain) = encrypt_data(
        data.asked,
        edel::install::form_factor(Path::new("/sys")),
        edel::services::screen_now(),
        data.given,
    );
    // The person's password is the data's passphrase (one to remember);
    // a console types only plain letters for sure at the start. Settled
    // here, so the plan says what the install then does.
    let (encrypted, why_plain, passphrase) = match (encrypted, data.password) {
        (false, _) => (false, why_plain, None),
        (true, Some(clear)) if edel::install::typable(clear.text().unwrap_or_default()) => {
            (true, None, Some(clear))
        }
        (true, Some(_)) if data.asked == Encrypt::Default => (false, Some(NOT_TYPABLE), None),
        (true, Some(_)) => bail!(
            "{}",
            tr(
                "the password has letters other than a-z, A-Z, digits and signs, which the start's prompt may not be able to type, so it cannot encrypt the data; choose another, or leave out --encrypt; nothing was changed"
            )
        ),
        (true, None) => (true, Some(PASSPHRASE_ASKED), None),
    };
    let plan = Plan {
        disk: describe(name)?,
        slot_mib: SLOT_MIB.max(running_mib),
        system_file: system_file.display().to_string(),
        encrypted,
    };
    plan.check()?;
    print!("{plan}");
    if let Some(why) = why_plain {
        println!("{}", tr(why));
    }
    if dry_run {
        println!(
            "edel install: {}",
            tr("this was only the plan; nothing was changed; leave out --plan to install")
        );
        return Ok(());
    }
    if !yes {
        if !io::stdin().is_terminal() {
            println!("{}", tr(NO_ANSWER));
            io::stdout().flush()?;
            std::process::exit(NOT_CONFIRMED);
        }
        print!(
            "{}",
            trf(
                "Type {name} to erase it and install, or anything else to stop: ",
                &[("name", name)]
            )
        );
        io::stdout().flush()?;
        let mut answer = String::new();
        io::stdin().lock().read_line(&mut answer)?;
        if answer.trim() != name {
            bail!(
                "{}",
                trf(
                    "stopped, because {name} was not typed; nothing was changed",
                    &[("name", name)]
                )
            );
        }
    }
    let missing = missing_tools();
    if !missing.is_empty() {
        bail!(
            "{}",
            trf(
                "this system lacks {tools}, which installing needs, so nothing was changed; this stick is damaged or not an Edel OS stick: write it again from the download",
                &[("tools", &missing.join(", "))]
            )
        );
    }
    // A passphrase of its own when the install gives no password.
    let passphrase = match passphrase {
        Some(clear) => Some(clear),
        None if encrypted => {
            let typed = ask_passphrase()?;
            if !edel::install::typable(typed.text().unwrap_or_default()) {
                bail!(
                    "{}",
                    tr(
                        "a passphrase may hold only a-z, A-Z, digits, spaces and signs, which the start's prompt can always type; nothing was changed"
                    )
                );
            }
            Some(typed)
        }
        None => None,
    };
    let _lock = Lock::take("install", "edel install")?;
    // Every failure once the disk is touched ends with what to do.
    let written = write(
        &plan,
        &slot,
        running_mib,
        system_file,
        password,
        passphrase.as_ref(),
    )
    .map_err(|err| {
        let line = format!("{err:#}");
        if line.contains("edel install again") {
            err
        } else {
            anyhow::anyhow!(
                "{line}; {}",
                tr("the disk is only partly installed, so run edel install again")
            )
        }
    });
    // Settings shows the failure on its page; the system log keeps it,
    // where `edel report` and a later look find it.
    if let Err(err) = &written {
        let msg = format!("edel install: {err:#}");
        let _ = Command::new("logger").args(["-t", "edel", &msg]).status();
    }
    written
}

/// How long the device manager may take over a new partition table's
/// events before `write` scans for the nodes itself.
const SETTLE: Duration = Duration::from_secs(1);

/// Waits for the kernel's device node of a new partition; after a second
/// without one, asks mdev to scan, in case nothing handles hotplug events.
fn wait_for(device: &Path) -> Result<()> {
    for tick in 0..100 {
        if device.exists() {
            return Ok(());
        }
        if tick == 10 {
            let _ = base::scan_devices().status();
        }
        sleep(Duration::from_millis(100));
    }
    bail!(
        "{}",
        trf(
            "{device} did not appear after partitioning, so the disk is only partly installed; run edel install again",
            &[("device", &device.display().to_string())]
        )
    )
}

/// Mounts `device` at `dir` for as long as the guard lives.
struct Mounted(PathBuf);

impl Mounted {
    fn new(device: &Path, dir: &Path, args: &[&str]) -> Result<Mounted> {
        fs::create_dir_all(dir).with_context(|| could_not_write(Path::new(&dir)))?;
        run(Command::new("mount").args(args).arg(device).arg(dir)).with_context(|| {
            trf(
                "could not open {device} to copy files onto it",
                &[("device", &device.display().to_string())],
            )
        })?;
        Ok(Mounted(dir.to_path_buf()))
    }
}

impl Drop for Mounted {
    fn drop(&mut self) {
        let _ = Command::new("umount").arg(&self.0).status();
    }
}

/// Writes the disk: `running_mib` MiB of the running slot become the start
/// of slot A, byte for byte; the rest of the slot stays unused, as a
/// read-only root needs no room.
fn write(
    plan: &Plan,
    slot: &Path,
    running_mib: u64,
    system_file: &Path,
    password: Option<&(String, String)>,
    passphrase: Option<&Password>,
) -> Result<()> {
    let name = &plan.disk.name;
    let disk = Path::new("/dev").join(name);
    let part = |n: u32| Path::new("/dev").join(partition_name(name, n));
    let layout = Layout {
        slot_mib: plan.slot_mib,
        data_mib: plan.data_mib(),
    };

    println!(
        "edel install: {}",
        trf(
            "partitioning {disk}",
            &[("disk", &disk.display().to_string())]
        )
    );
    let mut sfdisk = Command::new("sfdisk")
        .args(["--quiet", "--wipe", "always", "--wipe-partitions", "always"])
        .arg(&disk)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .context(trf(
            "could not partition {disk}: the partitioning tool did not start; this stick may be damaged: write it again from the download",
            &[("disk", &disk.display().to_string())],
        ))?;
    sfdisk
        .stdin
        .take()
        .context(trf(
            "could not partition {disk}: the partitioning tool did not start; this stick may be damaged: write it again from the download",
            &[("disk", &disk.display().to_string())],
        ))?
        .write_all(layout.sfdisk_script().as_bytes())?;
    if !sfdisk.wait()?.success() {
        bail!(
            "{}",
            trf(
                "could not partition {disk}: it may be in use or locked against writing (some SD cards have a switch); it may be half written, so run edel install again once that is fixed",
                &[("disk", &disk.display().to_string())]
            )
        );
    }
    // A disk that had partitions still shows their old device nodes, which
    // mdev takes out and makes again while it handles the kernel's events
    // for the new table, so a node seen at once may be gone a moment later.
    // Let those events pass, then scan, so every node is the new one.
    sleep(SETTLE);
    let _ = base::scan_devices().status();
    for n in 1..=4 {
        wait_for(&part(n))?;
    }

    println!(
        "edel install: {}",
        tr("copying the running system into slot A")
    );
    let mut half = HalfCopy::new(part(2));
    let mut from = File::open(slot).with_context(|| could_not_read(slot))?;
    let mut to = fs::OpenOptions::new().write(true).open(part(2))?;
    // The first block, which holds the file system's UUID, goes last.
    from.seek(SeekFrom::Start(HEAD))?;
    to.seek(SeekFrom::Start(HEAD))?;
    io::copy(
        &mut io::Read::take(&mut from, running_mib * MIB - HEAD),
        &mut to,
    )?;
    to.sync_all()?;
    let mut head = vec![0; HEAD as usize];
    from.seek(SeekFrom::Start(0))?;
    from.read_exact(&mut head)?;
    to.seek(SeekFrom::Start(0))?;
    to.write_all(&head)?;
    to.sync_all()?;
    drop(to);
    // Byte for byte as the running slot, read back from the disk (M1.12).
    let length = running_mib * MIB;
    run(Command::new("blockdev").arg("--flushbufs").arg(part(2))).with_context(|| {
        trf(
            "could not finish writing {part}",
            &[("part", &part(2).display().to_string())],
        )
    })?;
    if update::hash_prefix(&part(2), length)? != update::hash_prefix(slot, length)? {
        bail!(
            "{}",
            tr(
                "slot A on the new disk differs from the running slot after copying, so the copy is damaged and the disk cannot start yet; run edel install again"
            )
        );
    }
    println!(
        "edel install: {}",
        trf(
            "slot A is the running slot byte for byte ({mib} MiB)",
            &[("mib", &running_mib.to_string())]
        )
    );
    let status = Command::new("e2fsck").arg("-fn").arg(part(2)).status()?;
    if status.code() != Some(0) {
        bail!(
            "{}",
            trf(
                "the copy of Edel OS on {disk} is damaged and cannot start yet (the file system check stopped with code {code}); run edel install again, and if it fails again the disk may be failing",
                &[
                    ("disk", &disk.display().to_string()),
                    (
                        "code",
                        &status.code().map_or("none".into(), |c| c.to_string())
                    ),
                ]
            )
        );
    }
    half.done();

    println!("edel install: {}", tr("creating the data partition"));
    // Before the mount below, so the unmount comes first.
    let mut _opened = None;
    let data_device = match passphrase {
        Some(passphrase) => {
            println!(
                "edel install: {}",
                tr("encrypting the data partition with the password")
            );
            crate::crypt::close(INSTALL_MAPPER);
            let keyboard = settings::read(&fs::read_to_string(system_file)?)
                .ok()
                .and_then(|read| read.file.region.keyboard)
                .unwrap_or_default();
            let keymap = crate::crypt::keymap_for(&keyboard, |path| Path::new(path).exists());
            crate::crypt::format(&part(4), passphrase, keymap.as_deref())?;
            if !crate::crypt::open(&part(4), INSTALL_MAPPER, passphrase)? {
                bail!(
                    "{}",
                    tr(
                        "the new encrypted data partition did not open with its passphrase; run edel install again"
                    )
                );
            }
            _opened = Some(crate::crypt::Opened(INSTALL_MAPPER.to_string()));
            crate::crypt::mapper(INSTALL_MAPPER)
        }
        None => part(4),
    };
    run(Command::new("mkfs.ext4")
        .args(["-q", "-L", DATA_LABEL])
        .arg(&data_device))
    .with_context(|| {
        trf(
            "could not format the data partition {part}",
            &[("part", &part(4).display().to_string())],
        )
    })?;
    {
        let data = Mounted::new(
            &data_device,
            &Path::new(edel::places::INSTALL_DIR).join("data"),
            &["-t", "ext4"],
        )?;
        // The machine's settings file, where the installed machine finds
        // it once the data partition is mounted at /data.
        let machine = edel::places::machine_settings();
        let target = edel::places::on_data(&data.0, &machine);
        edel::settings::copy_file(system_file, &target)?;
        // The mark that this disk was installed, so a USB disk holding it
        // is no stick: no live login, no install without a password
        // (M6.17).
        let installed = Path::new(edel::places::INSTALLED);
        fs::write(
            edel::places::on_data(&data.0, installed),
            "Installed by edel install; this disk is no stick.\n",
        )?;
        // The person's password, hashed, for the first boot's apply to give
        // them; root's only (M6.6b).
        if let Some((user, hash)) = password {
            let first = edel::places::on_data(&data.0, Path::new(edel::places::FIRST_PASSWORDS));
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&first)
                .with_context(|| could_not_write(&first))?;
            file.write_all(format!("{user}:{hash}\n").as_bytes())?;
            file.sync_all()?;
        }
        // The add-ons the file chooses, tried at the first boot (M7.2b),
        // before the default apps, whose copy can fail with the system
        // installed: the desktop is one of them (M8.17c).
        crate::addons::stage_for_install(
            &fs::read_to_string(system_file).unwrap_or_default(),
            &data.0,
        );
        copy_default_apps(&data.0)?;
    }
    // The boot loader last, so the firmware starts only a disk the
    // install finished: one cut before here has no start-up partition
    // and is never taken for a machine that will not start.
    println!("edel install: {}", tr("writing the boot loader"));
    run(Command::new("mkfs.vfat")
        .args(["-F", "32", "-n", ESP_LABEL])
        .arg(part(1)))
    .with_context(|| {
        trf(
            "could not format the start-up partition {part}",
            &[("part", &part(1).display().to_string())],
        )
    })?;
    {
        let esp = Mounted::new(
            &part(1),
            &Path::new(edel::places::INSTALL_DIR).join("esp"),
            &["-t", "vfat", "-o", "iocharset=iso8859-1"],
        )?;
        let (_, efi_name) = boot::efi_target(std::env::consts::ARCH)?;
        let boot_dir = esp.0.join("EFI/BOOT");
        let edel_dir = esp.0.join(GRUB_PREFIX.trim_start_matches('/'));
        fs::create_dir_all(&boot_dir).with_context(|| could_not_write(Path::new(&boot_dir)))?;
        fs::create_dir_all(&edel_dir).with_context(|| could_not_write(Path::new(&edel_dir)))?;
        let slot_dir = Path::new(SLOT_DIR);
        fs::copy(slot_dir.join(efi_name), boot_dir.join(efi_name))
            .with_context(|| could_not_write(Path::new(&boot_dir.join(efi_name))))?;
        for file in ["grub.cfg", "loader.toml"] {
            fs::copy(slot_dir.join(file), edel_dir.join(file))
                .with_context(|| could_not_write(Path::new(&edel_dir.join(file))))?;
        }
        fs::write(edel_dir.join("grubenv"), boot::initial_grubenv())
            .with_context(|| could_not_write(Path::new(&edel_dir.join("grubenv"))))?;
    }

    run(&mut Command::new("sync")).with_context(|| {
        trf(
            "could not finish writing {part}",
            &[("part", &disk.display().to_string())],
        )
    })?;
    println!(
        "edel install: {}",
        trf(
            "Edel OS is installed on {disk}; restart, and take the stick out when the screen goes dark",
            &[("disk", &disk.display().to_string())]
        )
    );
    Ok(())
}

/// Copies the default apps (M6.2), when this system has them, onto the
/// new data partition mounted at `data`, so the installed machine starts
/// with them offline. `tar` keeps Flatpak's hard links between its
/// repository and its checkouts, which a plain copy would double.
fn copy_default_apps(data: &Path) -> Result<()> {
    let apps = Path::new(edel::places::DEFAULT_APPS);
    if !apps.is_dir() {
        return Ok(());
    }
    println!("edel install: {}", tr("copying the default apps"));
    // On the new disk's data partition, never the stick's own folder,
    // which a path left absolute would name (the copy went onto itself).
    let target = edel::places::on_data(data, apps);
    fs::create_dir_all(&target).with_context(|| could_not_write(Path::new(&target)))?;
    let mut pack = Command::new("tar")
        .arg("-C")
        .arg(apps)
        .args(["-cf", "-", "."])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .context(tr("could not copy the default apps: the copying tool did not start; the system is installed without them, and they come with the first edel update"))?;
    let stream = pack.stdout.take().context(tr("could not copy the default apps: the copying tool did not start; the system is installed without them, and they come with the first edel update"))?;
    run(Command::new("tar")
        .arg("-C")
        .arg(&target)
        .args(["-xf", "-"])
        .stdin(stream))?;
    let packed = pack.wait()?;
    if !packed.success() {
        bail!(
            "{}",
            trf(
                "could not read the default apps in {path} ({status}); the system is installed, without them",
                &[
                    ("path", &apps.display().to_string()),
                    ("status", &packed.to_string())
                ]
            )
        );
    }
    Ok(())
}

/// Slot A while the running system is copied into it. Until `done`, once
/// the copy is read back and checked, it carries the running root's file
/// system UUID; GRUB and the initramfs find a root by its partition since
/// M1.12, but a copy that stops half way must still never look whole (M2
/// and M3 review). The first block is written last and cleared again when
/// the copy fails, leaving a window of the check's seconds for a power
/// cut.
struct HalfCopy {
    partition: PathBuf,
    done: bool,
}

impl HalfCopy {
    fn new(partition: PathBuf) -> HalfCopy {
        HalfCopy {
            partition,
            done: false,
        }
    }

    fn done(&mut self) {
        self.done = true;
    }
}

impl Drop for HalfCopy {
    fn drop(&mut self) {
        if self.done {
            return;
        }
        if let Ok(mut part) = fs::OpenOptions::new().write(true).open(&self.partition) {
            let _ = part.write_all(&[0; HEAD as usize]);
            let _ = part.sync_all();
        }
        eprintln!(
            "edel install: {}",
            trf(
                "stopped before the copy was checked, so {partition} was made unable to start on purpose; run edel install again",
                &[("partition", &self.partition.display().to_string())]
            )
        );
    }
}

/// How long `erase = "yes"` counts down on the console before it erases a
/// disk that is not empty (M7.3).
const COUNTDOWN_SECONDS: u32 = 10;
/// How much of a disk must be zeros for `erase = "empty-only"`: the
/// partition tables and any file system's superblock are in it.
const BLANK_BYTES: usize = 1024 * 1024;

/// `edel install --unattended` (M7.3, docs/FLEET.md), which the
/// `edel-install` service runs at every start: with an `[install]` table in
/// the machine's settings file, it waits until this slot is confirmed,
/// then installs onto the disk the table names with that file less the
/// table, keeps the disk's id in `places::INSTALLED_TO` and powers off or
/// restarts. Without the table, or on a machine `edel install` made, it
/// does nothing.
pub fn unattended() -> Result<()> {
    let path = edel::places::machine_settings();
    let Ok(text) = fs::read_to_string(&path) else {
        return Ok(());
    };
    let Some(asked) = fleet::unattended(&text)? else {
        return Ok(());
    };
    // An installed machine's slot is the stick's, its own settings file
    // and all; only the stick acts on the table.
    if Path::new(edel::places::INSTALLED).exists() {
        return Ok(());
    }
    if let Some(id) = fs::read_to_string(edel::places::INSTALLED_TO)
        .ok()
        .and_then(|t| fleet::installed_to_id(&t))
        .filter(|id| Path::new(edel::places::DISKS_BY_ID).join(id).exists())
    {
        println!(
            "edel install: {}",
            trf("already installed to {id}; nothing to do", &[("id", &id)])
        );
        return Ok(());
    }
    let file = settings::read(&text)?.file;
    let shown = path.display().to_string();
    if !fleet::someone_can_log_in(&file) {
        bail!(
            "{}",
            trf(
                "install refused: {path}: {why}",
                &[("path", &shown), ("why", fleet::nobody_can_log_in())]
            )
        );
    }
    wait_until_confirmed();
    let (name, id) = unattended_disk(&asked)?;
    let device = format!("/dev/{name}");
    if asked.erase_any {
        count_down(&device, &id)?;
    } else if !is_empty_disk(&name)? {
        bail!(
            "{}",
            trf(
                "install refused: {device} is not empty; erase = \"yes\" with its /dev/disk/by-id/ name erases a disk that is not",
                &[("device", &device)]
            )
        );
    }
    println!(
        "edel install: {}",
        trf(
            "installing onto {device} ({id}) with nobody at the keyboard",
            &[("device", &device), ("id", &id)]
        )
    );
    let target = PathBuf::from(edel::places::INSTALL_UNATTENDED);
    fs::create_dir_all(edel::places::RUN_DIR)
        .with_context(|| could_not_write(Path::new(&edel::places::RUN_DIR)))?;
    fs::write(&target, fleet::without_install(&text)?)?;
    // Nobody is there to type a passphrase at its starts.
    let data = Data {
        asked: Encrypt::Off,
        given: false,
        password: None,
    };
    install_on(&name, &target, None, data, false, true)?;
    let date = crate::release::today_text();
    crate::machine::write_whole(
        Path::new(edel::places::INSTALLED_TO),
        fleet::installed_to_text(&id, &date).as_bytes(),
    )?;
    let action = if asked.reboot {
        crate::system_power::Action::Restart
    } else {
        crate::system_power::Action::ShutDown
    };
    println!(
        "edel install: {}",
        trf(
            "installed to {id}; {next}",
            &[
                ("id", &id),
                (
                    "next",
                    if asked.reboot {
                        tr("restarting")
                    } else {
                        tr("powering off")
                    }
                )
            ]
        )
    );
    crate::system_power::run(action)
}

/// Waits until the guard confirmed this boot's slot, so a stick that
/// cannot start well never writes a disk.
fn wait_until_confirmed() {
    let confirmed = Path::new(edel::places::CONFIRMED);
    if confirmed.exists() {
        return;
    }
    println!(
        "edel install: {}",
        tr("waiting until this slot is confirmed before installing")
    );
    while !confirmed.exists() {
        sleep(Duration::from_secs(1));
    }
}

/// The disk `[install]` names, as its kernel name and its id under
/// `/dev/disk/by-id/`.
fn unattended_disk(asked: &fleet::Unattended) -> Result<(String, String)> {
    let links = by_id_links();
    if let Some(id) = &asked.by_id {
        let Some((_, name)) = links.iter().find(|(link, _)| link == id) else {
            bail!(
                "{}",
                trf(
                    "install refused: {path} is not in this machine",
                    &[(
                        "path",
                        &Path::new(edel::places::DISKS_BY_ID)
                            .join(id)
                            .display()
                            .to_string()
                    )]
                )
            );
        };
        return Ok((name.clone(), id.clone()));
    }
    let running = update::Disk::find()?.name;
    let disks: Vec<fleet::AutoDisk> = candidates()?
        .into_iter()
        .map(|d| fleet::AutoDisk {
            removable: removable(&d.name),
            id: fleet::by_id_of(&links, &d.name),
            bytes: d.bytes,
            name: d.name,
        })
        .collect();
    let need = (2 + ESP_MIB + 2 * SLOT_MIB + MIN_DATA_MIB) * MIB;
    let disk = fleet::pick_auto(&disks, &running, need)?;
    let Some(id) = disk.id else {
        bail!(
            "{}",
            trf(
                "install refused: /dev/{name} has no /dev/disk/by-id/ name to remember it by, so starting from this stick again could install it twice",
                &[("name", &disk.name)]
            )
        );
    };
    Ok((disk.name, id))
}

/// Every link in `/dev/disk/by-id/` with the kernel name it points at.
fn by_id_links() -> Vec<(String, String)> {
    let Ok(entries) = fs::read_dir(edel::places::DISKS_BY_ID) else {
        return Vec::new();
    };
    entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let target = fs::canonicalize(e.path()).ok()?;
            Some((
                e.file_name().to_string_lossy().into_owned(),
                target.file_name()?.to_string_lossy().into_owned(),
            ))
        })
        .collect()
}

/// Whether the disk `name` has no partitions and only zeros in its first
/// MiB.
fn is_empty_disk(name: &str) -> Result<bool> {
    if !describe(name)?.partitions.is_empty() {
        return Ok(false);
    }
    let device = Path::new("/dev").join(name);
    let mut head = Vec::with_capacity(BLANK_BYTES);
    File::open(&device)
        .with_context(|| could_not_read(&device))?
        .take(BLANK_BYTES as u64)
        .read_to_end(&mut head)?;
    Ok(fleet::is_blank(&head))
}

/// Counts down on the console before `erase = "yes"` erases a disk, so
/// someone at the machine can still power it off.
fn count_down(device: &str, id: &str) -> Result<()> {
    for left in (1..=COUNTDOWN_SECONDS).rev() {
        println!(
            "edel install: {}",
            trf(
                "erasing {device} ({id}) in {seconds} s; power the machine off to stop",
                &[
                    ("device", device),
                    ("id", id),
                    ("seconds", &left.to_string())
                ]
            )
        );
        io::stdout().flush()?;
        sleep(Duration::from_secs(1));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_disk_mounted_swapped_or_held_is_in_use() {
        let sys = std::env::temp_dir().join(format!("edel-in-use-{}", std::process::id()));
        let _ = fs::remove_dir_all(&sys);
        for dir in [
            "block/sdb/sdb1",
            "block/sdb/sdb4",
            "class/block/sdb1/holders",
            "class/block/sdb4/holders",
            "block/nvme0n1/nvme0n1p2",
            "class/block/nvme0n1p2/holders",
        ] {
            fs::create_dir_all(sys.join(dir)).unwrap();
        }
        let none = "";
        assert!(!in_use(&sys, "/dev/sda2 / ext4 ro 0 0\n", none, "sdb"));
        assert!(in_use(
            &sys,
            "/dev/sdb1 /media/x vfat rw 0 0\n",
            none,
            "sdb"
        ));
        assert!(!in_use(
            &sys,
            "/dev/sdbb1 /media/x vfat rw 0 0\n",
            none,
            "sdb"
        ));
        let swaps = "Filename Type Size Used Priority\n/dev/sdb3 partition 1024 0 -2\n";
        assert!(in_use(&sys, none, swaps, "sdb"));
        assert!(!in_use(&sys, none, none, "sdb"));
        fs::create_dir_all(sys.join("class/block/sdb4/holders/dm-0")).unwrap();
        assert!(in_use(&sys, none, none, "sdb"));
        assert!(in_use(
            &sys,
            "/dev/nvme0n1p2 /x ext4 rw 0 0\n",
            none,
            "nvme0n1"
        ));
        assert!(!in_use(&sys, none, none, "nvme0n1"));
        fs::create_dir_all(sys.join("class/block/nvme0n1p2/holders/dm-1")).unwrap();
        assert!(in_use(&sys, none, none, "nvme0n1"));
        fs::remove_dir_all(&sys).unwrap();
    }

    #[test]
    fn laptops_and_screens_are_encrypted_when_the_install_gives_a_password() {
        let d = Encrypt::Default;
        assert_eq!(encrypt_data(d, "laptop", false, true), (true, None));
        assert_eq!(encrypt_data(d, "pc", true, true), (true, None));
        assert_eq!(encrypt_data(d, "pc", false, true), (false, None));
        assert_eq!(encrypt_data(d, "vm", true, true), (false, None));
        let (on, why) = encrypt_data(d, "laptop", true, false);
        assert!(!on && why.is_some_and(|w| w.contains("--encrypt")));
        assert_eq!(encrypt_data(Encrypt::On, "vm", false, false), (true, None));
        assert_eq!(
            encrypt_data(Encrypt::Off, "laptop", true, true),
            (false, None)
        );
    }

    #[test]
    fn names_partitions_the_way_the_kernel_does() {
        assert_eq!(partition_name("sda", 2), "sda2");
        assert_eq!(partition_name("vdb", 4), "vdb4");
        assert_eq!(partition_name("nvme0n1", 1), "nvme0n1p1");
        assert_eq!(partition_name("mmcblk0", 3), "mmcblk0p3");
        assert_eq!(disk_name("/dev/vdb"), "vdb");
    }

    /// Install without `--yes` and without a terminal changes nothing and
    /// says how to go on; CI's install test checks the exit code (3).
    #[test]
    fn install_without_a_yes_says_nothing_was_changed() {
        assert_eq!(
            NO_ANSWER,
            "edel install: no terminal to confirm on and no --yes, so nothing was changed; run it in a terminal, or read the plan above and add --yes"
        );
        assert_eq!(NOT_CONFIRMED, 3);
    }

    #[test]
    fn every_bootable_image_has_every_tool_install_runs() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for name in ["images/edel", "ci/vm/vm"] {
            let def = crate::def::ImageDef::load(&root.join(format!("{name}.toml"))).unwrap();
            for (tool, package) in TOOLS {
                assert!(
                    def.packages.iter().any(|p| p == package),
                    "{tool} needs {package} in a feature of {name}.toml"
                );
            }
        }
    }
}
