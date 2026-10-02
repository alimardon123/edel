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
mod loader;
mod machine;
mod release;
mod run;
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
    /// Build and inspect Edel OS images
    Image {
        #[command(subcommand)]
        command: ImageCommands,
    },
    /// Steps the boot services run
    Boot {
        #[command(subcommand)]
        command: BootCommands,
    },
    /// Make, sign and check release manifests
    Release {
        #[command(subcommand)]
        command: ReleaseCommands,
    },
    /// Install updates into the A/B slots and roll back
    Update {
        #[command(subcommand)]
        command: UpdateCommands,
    },
    /// Check the file that describes a whole machine (system.toml)
    System {
        #[command(subcommand)]
        command: SystemCommands,
    },
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
    /// Print this machine as a system file
    Export,
}

#[derive(Subcommand)]
enum BootCommands {
    /// Grow the data partition on first boot and mount it at /data
    MountData,
    /// Watch this slot until it is healthy, then confirm it; a slot that
    /// hangs is restarted by the watchdog and falls back
    Guard,
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
enum UpdateCommands {
    /// Show both slots and which one is running
    Status,
    /// Install a signed release into the slot that is not running; it
    /// starts next
    Install {
        /// The release's release.toml, a path or an http(s) URL (its .sig
        /// and image beside it), or with --unsigned a slot image or a block
        /// device holding one
        location: String,
        /// Install even when the release is not newer than this system
        #[arg(long)]
        allow_downgrade: bool,
        /// Install a slot image without a signed release.toml (for testing)
        #[arg(long)]
        unsigned: bool,
    },
    /// Show the version of the release at a path or URL next to this one
    Check {
        /// The release's release.toml, a path or an http(s) URL
        location: String,
    },
    /// Confirm that the running slot works (run once the system is up)
    MarkGood,
    /// Start the other slot again at the next boot
    Rollback,
}

#[derive(Subcommand)]
enum ImageCommands {
    /// Build an image from its definition file (run as root on Alpine)
    Build {
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
        /// Another public key that may sign updates, after the definition's
        /// own; for test images. Can be given more than once.
        #[arg(long = "public-key", value_name = "FILE")]
        public_keys: Vec<PathBuf>,
        /// A tag written into grub.cfg so the boot loader differs from an
        /// untagged build; for test images
        #[arg(long, value_name = "TAG")]
        loader_tag: Option<String>,
        /// Print every step without changing anything
        #[arg(long)]
        dry_run: bool,
    },
    /// Check an image definition without building it
    Check {
        /// Image definition, for example images/vm.toml
        definition: PathBuf,
    },
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Commands::Image { command } => match command {
            ImageCommands::Build {
                definition,
                out,
                extra_files,
                health_timeout,
                public_keys,
                loader_tag,
                dry_run,
            } => {
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
                image::Build {
                    def: &def,
                    def_dir,
                    extra_files,
                    health_timeout,
                    extra_keys: public_keys,
                    loader_tag,
                    out,
                    runner: Runner { dry_run },
                }
                .run()
            }
            ImageCommands::Check { definition } => {
                let def = ImageDef::load(&definition)?;
                println!(
                    "{}: ok ({} packages)",
                    def.stem(),
                    def.packages.install.len()
                );
                Ok(())
            }
        },
        Commands::Boot { command } => match command {
            BootCommands::MountData => data::mount_data(),
            BootCommands::Guard => guard::guard(),
        },
        Commands::Release { command } => match command {
            ReleaseCommands::Keygen { dir, name } => release::keygen(&dir, &name),
            ReleaseCommands::Make {
                version,
                channel,
                images,
            } => release::make(&version, &channel, &images).map(|_| ()),
            ReleaseCommands::Sign { key, file } => release::sign(&key, &file),
            ReleaseCommands::Verify { keys, file } => release::verify(&keys, &file),
        },
        Commands::Update { command } => match command {
            UpdateCommands::Status => update::status(),
            UpdateCommands::Install {
                location,
                allow_downgrade,
                unsigned,
            } => update::install(&location, allow_downgrade, unsigned),
            UpdateCommands::Check { location } => release::check(&location),
            UpdateCommands::MarkGood => update::mark_good(),
            UpdateCommands::Rollback => update::rollback(),
        },
        Commands::System { command } => match command {
            SystemCommands::Check { file } => check_system_file(&file),
            SystemCommands::Apply { file, boot } => machine::apply(file.as_deref(), boot),
            SystemCommands::Export => machine::export(),
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
