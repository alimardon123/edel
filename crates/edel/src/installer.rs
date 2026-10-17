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
];

const MIB: u64 = 1024 * 1024;
/// The first block of a slot: it holds the ext4 superblock and its UUID.
const HEAD: u64 = 4096;
/// What is said when the plan was shown and nobody could confirm it.
const NO_ANSWER: &str = n_(
    "edel install: no terminal to confirm on and no --yes, so nothing was changed; run it in a terminal, or read the plan above and add --yes",
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
            fs = Some(tr("run as root to see").into());
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

/// Whether the disk named `name` says it is removable, as a USB stick does.
fn removable(name: &str) -> bool {
    fs::read_to_string(Path::new("/sys/block").join(name).join("removable"))
        .is_ok_and(|v| v.trim() == "1")
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
                "{tool} could not hash the password; nothing was changed",
                &tool
            )
        );
    }
    Ok(hash)
}

/// The questions on the terminal (M6.6b), in `install::QUESTIONS`' order,
/// up to the disk, which the caller asks; More options only when asked
/// for. Returns the answers and the password's hash.
fn ask_questions() -> Result<(Answers, Option<String>)> {
    println!(
        "{}\n",
        tr(
            "Edel OS asks a few questions, then shows the plan before it changes anything. Press Return to take the suggestion in brackets."
        )
    );
    let mut answers = Answers::default();
    let mut hashed = None;
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
            "password" => hashed = Some(hash(&ask_password()?)?),
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
    Ok((answers, hashed))
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
) -> Result<()> {
    let asking = system_file.is_none() || disk.is_none();
    if asking && !io::stdin().is_terminal() {
        bail!(
            "{}",
            tr(
                "edel install asks its questions on a terminal; run it in one, or name the disk and give --settings FILE"
            )
        );
    }
    let (written, hashed) = match system_file {
        Some(file) if new_password => (file.to_path_buf(), Some(password_from_stdin(file)?)),
        Some(file) => (file.to_path_buf(), None),
        None => {
            let (answers, hashed) = ask_questions()?;
            let text = answers.settings_text()?;
            let file = PathBuf::from(edel::places::INSTALL_ANSWERS);
            fs::create_dir_all(edel::places::RUN_DIR)?;
            fs::write(&file, text)?;
            (file, hashed.map(|h| (answers.user.unwrap_or_default(), h)))
        }
    };
    let disk = match disk {
        Some(d) => d.to_string(),
        None => ask_disk()?,
    };
    install_on(&disk, &written, hashed.as_ref(), dry_run, yes)
}

/// The one person of `file` and the hash of the password standard input
/// gives them (`--new-password-stdin`, M6.6a): Settings' installer page asks
/// the password and hands it over this way, never on a command line.
fn password_from_stdin(file: &Path) -> Result<(String, String)> {
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
    Ok((person.clone(), hash(&password)?))
}

/// Installs on `disk` with the settings file `system_file` and, when the
/// questions asked one, the person's password hash.
fn install_on(
    disk: &str,
    system_file: &Path,
    password: Option<&(String, String)>,
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
    if mounts
        .lines()
        .any(|l| l.starts_with(&format!("/dev/{name}")))
    {
        bail!(
            "{}",
            trf(
                "/dev/{name} has a mounted file system; unmount it first",
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
    let plan = Plan {
        disk: describe(name)?,
        slot_mib: SLOT_MIB.max(running_mib),
        system_file: system_file.display().to_string(),
    };
    plan.check()?;
    print!("{plan}");
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
                "this system lacks {tools}, so nothing was changed; install them (apk add) and run edel install again",
                &[("tools", &missing.join(", "))]
            )
        );
    }
    let _lock = Lock::take("install", "edel install")?;
    write(&plan, &slot, running_mib, system_file, password)
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
        fs::create_dir_all(dir)?;
        run(Command::new("mount").args(args).arg(device).arg(dir))?;
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
        .context(tr("could not start sfdisk"))?;
    sfdisk
        .stdin
        .take()
        .context(tr("sfdisk has no input"))?
        .write_all(layout.sfdisk_script().as_bytes())?;
    if !sfdisk.wait()?.success() {
        bail!(
            "{}",
            trf(
                "sfdisk could not partition {disk}; the disk may be in use or write-protected, and it may be half written, so run edel install again once that is fixed",
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
    run(Command::new("blockdev").arg("--flushbufs").arg(part(2)))?;
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
                "e2fsck found errors in the copied slot ({status}), so the copy is damaged and the disk cannot start yet; run edel install again",
                &[("status", &status.to_string())]
            )
        );
    }
    half.done();

    println!("edel install: {}", tr("writing the boot loader"));
    run(Command::new("mkfs.vfat")
        .args(["-F", "32", "-n", ESP_LABEL])
        .arg(part(1)))?;
    {
        let esp = Mounted::new(
            &part(1),
            &Path::new(edel::places::INSTALL_DIR).join("esp"),
            &["-t", "vfat", "-o", "iocharset=iso8859-1"],
        )?;
        let (_, efi_name) = boot::efi_target(std::env::consts::ARCH)?;
        let boot_dir = esp.0.join("EFI/BOOT");
        let edel_dir = esp.0.join(GRUB_PREFIX.trim_start_matches('/'));
        fs::create_dir_all(&boot_dir)?;
        fs::create_dir_all(&edel_dir)?;
        let slot_dir = Path::new(SLOT_DIR);
        fs::copy(slot_dir.join(efi_name), boot_dir.join(efi_name))?;
        for file in ["grub.cfg", "loader.toml"] {
            fs::copy(slot_dir.join(file), edel_dir.join(file))?;
        }
        fs::write(edel_dir.join("grubenv"), boot::initial_grubenv())?;
    }

    println!("edel install: {}", tr("creating the data partition"));
    run(Command::new("mkfs.ext4")
        .args(["-q", "-L", DATA_LABEL])
        .arg(part(4)))?;
    {
        let data = Mounted::new(
            &part(4),
            &Path::new(edel::places::INSTALL_DIR).join("data"),
            &["-t", "ext4"],
        )?;
        // The machine's settings file, where the installed machine finds
        // it once the data partition is mounted at /data.
        let machine = edel::places::machine_settings();
        let target = data.0.join(
            machine
                .strip_prefix(edel::places::DATA_MOUNT)
                .unwrap_or(&machine),
        );
        fs::create_dir_all(target.parent().unwrap_or(&data.0))?;
        fs::copy(system_file, target)?;
        // The mark that this disk was installed, so a USB disk holding it
        // is no stick: no live login, no install without a password
        // (M6.17).
        let installed = Path::new(edel::places::INSTALLED);
        fs::write(
            data.0.join(
                installed
                    .strip_prefix(edel::places::DATA_MOUNT)
                    .unwrap_or(installed),
            ),
            "Installed by edel install; this disk is no stick.\n",
        )?;
        // The person's password, hashed, for the first boot's apply to give
        // them; root's only (M6.6b).
        if let Some((user, hash)) = password {
            let first = data.0.join(
                Path::new(edel::places::FIRST_PASSWORDS)
                    .strip_prefix(edel::places::DATA_MOUNT)
                    .unwrap_or(Path::new(edel::places::FIRST_PASSWORDS)),
            );
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&first)?;
            file.write_all(format!("{user}:{hash}\n").as_bytes())?;
            file.sync_all()?;
        }
        copy_default_apps(&data.0)?;
        // The add-ons the file chooses, tried at the first boot (M7.2b).
        crate::addons::stage_for_install(
            &fs::read_to_string(system_file).unwrap_or_default(),
            &data.0,
        );
    }
    run(&mut Command::new("sync"))?;
    println!(
        "edel install: {}",
        trf(
            "done; restart from {disk} to use it",
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
    let target = data.join(
        apps.strip_prefix(edel::places::DATA_MOUNT)
            .unwrap_or(apps)
            .strip_prefix("/")
            .unwrap_or(apps),
    );
    fs::create_dir_all(&target)?;
    let mut pack = Command::new("tar")
        .arg("-C")
        .arg(apps)
        .args(["-cf", "-", "."])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .context("could not start tar to copy the default apps")?;
    let stream = pack.stdout.take().context("tar gave no output to copy")?;
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
            "edel install: stopped; cleared the start of {} so it cannot be started",
            self.partition.display()
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
    fs::create_dir_all(edel::places::RUN_DIR)?;
    fs::write(&target, fleet::without_install(&text)?)?;
    install_on(&name, &target, None, false, true)?;
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
