//! `edel`: the Edel OS system tool.
//!
//! It builds images, updates and rolls back A/B slots and checks system
//! files. Add-ons (ADR-007) will live here too, so there is one tool to
//! learn. The system file parser is the library half (`edel::system`).

mod boot;
mod data;
mod def;
mod grubenv;
mod guard;
mod image;
mod installer;
mod loader;
mod machine;
mod release;
mod report;
mod run;
mod shell;
mod update;

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use edel::system;

use crate::def::ImageDef;
use crate::run::Runner;

#[derive(Parser)]
#[command(name = "edel", version, about = "The Edel OS system tool")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Install a newer Edel OS into the other slot from RELEASE; it starts
    /// at the next restart, and falls back on its own if it fails
    Update {
        /// The release: its release.toml, a path or an http(s) URL (its .sig
        /// and image beside it), or with --unsigned a slot image or a block
        /// device holding one
        #[arg(value_name = "RELEASE")]
        location: String,
        /// Only show the version RELEASE holds next to this one; change
        /// nothing
        #[arg(long, conflicts_with = "unsigned")]
        dry_run: bool,
        /// Install even when the release is not newer than this system
        #[arg(long)]
        allow_downgrade: bool,
        /// Install a slot image without a signed release.toml (for testing)
        #[arg(long)]
        unsigned: bool,
    },
    /// Go back to the version in the other slot, the one before the last
    /// update; it starts at the next restart
    Rollback,
    /// Show the version running now and the one in the other slot
    Status,
    /// Steps the boot services run
    #[command(hide = true)]
    Boot {
        #[command(subcommand)]
        command: BootCommands,
    },
    /// Make, sign and check release manifests
    #[command(hide = true)]
    Release {
        #[command(subcommand)]
        command: ReleaseCommands,
    },
    /// Install Edel OS on another disk, erasing it: the running system
    /// becomes slot A, and FILE the new machine's system file
    Install {
        /// The disk, such as /dev/sda
        disk: String,
        /// The system file the new machine starts with
        #[arg(long, value_name = "FILE")]
        system: PathBuf,
        /// Show what would be erased and written, and change nothing
        #[arg(long)]
        dry_run: bool,
        /// Erase the disk without asking, for unattended installs; without
        /// it and without a terminal, install shows its plan and exits 3
        #[arg(long)]
        yes: bool,
    },
    /// Print what an issue about this machine needs, as TOML: the release,
    /// kernel, boot time, memory in use, PCI devices and the kernel log
    Report {
        /// Write it to /EFI/edel/report.toml on the EFI system partition
        /// instead, where any computer can read it from the disk or stick
        #[arg(long)]
        esp: bool,
    },
    /// This machine's settings, kept in one file (system.toml): check,
    /// change, apply and export them
    System {
        #[command(subcommand)]
        command: SystemCommands,
    },
    /// What the desktop session is doing
    Shell {
        #[command(subcommand)]
        command: ShellCommands,
    },
    /// Build and inspect Edel OS images
    Image {
        #[command(subcommand)]
        command: ImageCommands,
    },
}

#[derive(Subcommand)]
enum ShellCommands {
    /// Print the effect tier the compositor runs at: lite, balanced or
    /// full
    Tier,
}

#[derive(Subcommand)]
enum SystemCommands {
    /// Check a system file strictly: every unknown key, value or format,
    /// and every key this release does not act on yet, is refused
    Check {
        /// The system file, for example /data/edel/system.toml
        file: PathBuf,
    },
    /// Make this machine match a system file: hostname, users, their ssh
    /// keys and developer mode. A given file becomes the machine's own
    Apply {
        /// The system file; without one, the machine's own
        /// (/data/edel/system.toml), seeded first if it is missing
        file: Option<PathBuf>,
        /// Leave setting the running hostname to the hostname service; for
        /// the edel-system boot service
        #[arg(long)]
        boot: bool,
    },
    /// Show what apply would change, changing nothing; exits 1 when
    /// there is anything to change
    Diff {
        /// The system file; without one, the machine's own
        file: Option<PathBuf>,
    },
    /// Print this machine as a system file, and the /etc files it changed
    Export,
    /// Change one key in the machine's system file, keeping everything
    /// else in it byte for byte. Window, screen and shortcut settings take
    /// effect at once; apply makes the rest take effect
    Set {
        /// KEY=VALUE, such as network.hostname=lab-1
        assignment: String,
    },
    /// Remove one key from the machine's system file, so the release
    /// decides it again. Window, screen and shortcut settings take effect
    /// at once; apply makes the rest take effect
    Unset {
        /// The key, such as network.hostname
        key: String,
    },
}

#[derive(Subcommand)]
enum BootCommands {
    /// Grow the data partition on first boot and mount it at /data
    MountData,
    /// Watch this slot until it is healthy, then confirm it; a slot that
    /// hangs is restarted by the watchdog and falls back
    Guard,
    /// Confirm that the running slot works, as the guard does once the
    /// system is up
    MarkGood,
}

#[derive(Subcommand)]
enum ReleaseCommands {
    /// Make a new signing key pair, NAME.key (secret) and NAME.pub
    Keygen {
        /// Directory for the two files
        dir: PathBuf,
        /// Name of the key, for example edel-1
        name: String,
    },
    /// Write release.toml beside the images it lists
    Make {
        /// The release's version, for example 2026.10.1
        #[arg(long)]
        version: String,
        /// The channel, for example stable
        #[arg(long, default_value = "stable")]
        channel: String,
        /// Name each image by its URL under this address instead of beside
        /// the manifest, for a channel served apart from the images
        #[arg(long, value_name = "URL")]
        base_url: Option<String>,
        /// Write release.toml here instead of beside the images
        #[arg(long, value_name = "DIR")]
        out: Option<PathBuf>,
        /// The update images, all in one directory
        #[arg(required = true)]
        images: Vec<PathBuf>,
    },
    /// Sign a file with a secret key, writing FILE.sig
    Sign {
        /// The secret key file (NAME.key)
        #[arg(long)]
        key: PathBuf,
        file: PathBuf,
    },
    /// Check FILE.sig against public keys
    Verify {
        /// Directory of public keys (*.pub)
        #[arg(long, default_value = release::KEYS_DIR)]
        keys: PathBuf,
        file: PathBuf,
    },
}

#[derive(Subcommand)]
enum ImageCommands {
    /// Build an image from its definition file (run as root on Alpine)
    Build(Box<BuildArgs>),
    /// Check an image definition without building it
    Check {
        /// Image definition, for example images/vm.toml
        definition: PathBuf,
    },
}

#[derive(clap::Args)]
struct BuildArgs {
    /// Image definition, for example images/vm.toml
    definition: PathBuf,
    /// Directory for the finished images and the work area
    #[arg(long, default_value = "out")]
    out: PathBuf,
    /// Another directory to copy over the image, after the definition's
    /// own files; for test images. Can be given more than once.
    #[arg(long = "files", value_name = "DIR")]
    extra_files: Vec<PathBuf>,
    /// Seconds the boot guard waits for health before the watchdog
    /// restarts the machine, instead of the definition's; for test images
    #[arg(long, value_name = "SECS")]
    health_timeout: Option<u64>,
    /// Size of each slot in MiB, instead of the definition's; for test
    /// images, such as the install test's live disk, which has the
    /// laptop stick's small slots
    #[arg(long, value_name = "MIB", value_parser = clap::value_parser!(u64).range(64..))]
    slot_mib: Option<u64>,
    /// Another public key that may sign updates, after the definition's
    /// own; for test images. Can be given more than once.
    #[arg(long = "public-key", value_name = "FILE")]
    public_keys: Vec<PathBuf>,
    /// A tag written into grub.cfg so the boot loader differs from an
    /// untagged build; for test images
    #[arg(long, value_name = "TAG")]
    loader_tag: Option<String>,
    /// The release version written into the image, such as 2026.10.3;
    /// without it the image keeps its development version
    #[arg(long, value_name = "VERSION")]
    version: Option<String>,
    /// The channel written into the image, such as preview or stable
    #[arg(long, default_value = "dev")]
    channel: String,
    /// An apk cache to share between the images of one build, so they
    /// all install from one package index
    #[arg(long, value_name = "DIR")]
    apk_cache: Option<PathBuf>,
    /// Skip the .gz copy of a VM image's disk (the slot's .gz, the
    /// update image, is always made); for test images
    #[arg(long)]
    no_compress: bool,
    /// Print every step without changing anything
    #[arg(long)]
    dry_run: bool,
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Commands::Image { command } => match command {
            ImageCommands::Build(args) => {
                let BuildArgs {
                    definition,
                    out,
                    extra_files,
                    health_timeout,
                    slot_mib,
                    public_keys,
                    loader_tag,
                    version,
                    channel,
                    apk_cache,
                    no_compress,
                    dry_run,
                } = *args;
                if let Some(v) = version.as_deref().filter(|v| !image::is_version(v)) {
                    anyhow::bail!("version {v:?} is not numbers joined by dots, such as 2026.10.3");
                }
                let def = ImageDef::load(&definition)?;
                let def_dir = definition.parent().map(PathBuf::from).unwrap_or_default();
                // apk and mount get absolute paths, so nothing depends on
                // which directory a tool happens to run in.
                let out = if dry_run {
                    out
                } else {
                    std::fs::create_dir_all(&out)?;
                    std::fs::canonicalize(&out)?
                };
                // apk opens a relative cache directory inside the new root.
                let apk_cache = match apk_cache {
                    Some(cache) if !dry_run => {
                        std::fs::create_dir_all(&cache)?;
                        Some(std::fs::canonicalize(&cache)?)
                    }
                    other => other,
                };
                image::Build {
                    def: &def,
                    def_dir,
                    extra_files,
                    health_timeout,
                    slot_mib,
                    extra_keys: public_keys,
                    loader_tag,
                    version,
                    channel,
                    apk_cache,
                    compress: !no_compress,
                    out,
                    runner: Runner { dry_run },
                }
                .run()
            }
            ImageCommands::Check { definition } => {
                let def = ImageDef::load(&definition)?;
                println!(
                    "{}: ok ({} features, {} packages)",
                    def.stem(),
                    def.features.len(),
                    def.packages.len()
                );
                Ok(())
            }
        },
        Commands::Boot { command } => match command {
            BootCommands::MountData => data::mount_data(),
            BootCommands::Guard => guard::guard(),
            BootCommands::MarkGood => update::mark_good(),
        },
        Commands::Release { command } => match command {
            ReleaseCommands::Keygen { dir, name } => release::keygen(&dir, &name),
            ReleaseCommands::Make {
                version,
                channel,
                base_url,
                out,
                images,
            } => release::make(
                &version,
                &channel,
                base_url.as_deref(),
                out.as_deref(),
                &images,
            )
            .map(|_| ()),
            ReleaseCommands::Sign { key, file } => release::sign(&key, &file),
            ReleaseCommands::Verify { keys, file } => release::verify(&keys, &file),
        },
        Commands::Update {
            location,
            dry_run: true,
            ..
        } => release::check(&location),
        Commands::Update {
            location,
            dry_run: false,
            allow_downgrade,
            unsigned,
        } => update::install(&location, allow_downgrade, unsigned),
        Commands::Rollback => update::rollback(),
        Commands::Status => update::status(),
        Commands::Install {
            disk,
            system,
            dry_run,
            yes,
        } => installer::install(&disk, &system, dry_run, yes),
        Commands::Report { esp } => report::report(esp),
        Commands::Shell { command } => match command {
            ShellCommands::Tier => shell::tier(),
        },
        Commands::System { command } => match command {
            SystemCommands::Check { file } => check_system_file(&file),
            SystemCommands::Apply { file, boot } => machine::apply(file.as_deref(), boot),
            SystemCommands::Diff { file } => {
                if machine::diff(file.as_deref())? {
                    std::process::exit(1);
                }
                Ok(())
            }
            SystemCommands::Export => machine::export(),
            SystemCommands::Set { assignment } => machine::set(&assignment),
            SystemCommands::Unset { key } => machine::unset(&key),
        },
    }
}

/// `edel system check FILE`: prints one line per problem and fails when
/// there is any (ADR-008, section 2).
fn check_system_file(file: &std::path::Path) -> Result<()> {
    let text =
        std::fs::read_to_string(file).with_context(|| format!("reading {}", file.display()))?;
    let problems = system::check(&text).with_context(|| format!("checking {}", file.display()))?;
    if problems.is_empty() {
        println!("{}: ok", file.display());
        return Ok(());
    }
    for problem in &problems {
        println!("{problem}");
    }
    bail!("{} has {} problems", file.display(), problems.len())
}

#[cfg(test)]
mod tests {
    use clap::error::ErrorKind;
    use clap::{CommandFactory, Parser};

    use super::Cli;

    /// The help lists what people run (ADR-008's easy to use decision);
    /// the boot services' and CI's own commands still work, unlisted.
    #[test]
    fn help_lists_what_people_run() {
        let help = Cli::command().render_help().to_string();
        for shown in [
            "update", "rollback", "status", "install", "report", "system", "shell", "image",
        ] {
            assert!(
                help.contains(&format!("  {shown} ")),
                "{shown} missing:\n{help}"
            );
        }
        for hidden in ["boot", "release"] {
            assert!(
                !help.contains(&format!("  {hidden} ")),
                "{hidden} listed:\n{help}"
            );
            let asked = Cli::try_parse_from(["edel", hidden, "--help"]);
            assert!(asked.is_err_and(|e| e.kind() == ErrorKind::DisplayHelp));
        }
    }
}
