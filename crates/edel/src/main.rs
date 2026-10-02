//! `edel`: the Edel OS system tool.
//!
//! It builds images and updates and rolls back A/B slots. Add-ons and
//! system files (ADR-006, ADR-007) will live here too, so there is one tool
//! to learn.

mod boot;
mod data;
mod def;
mod grubenv;
mod guard;
mod image;
mod run;
mod update;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

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
    /// Install updates into the A/B slots and roll back
    Update {
        #[command(subcommand)]
        command: UpdateCommands,
    },
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
enum UpdateCommands {
    /// Show both slots and which one is running
    Status,
    /// Write a slot image to the slot that is not running; it starts next
    Install {
        /// The update: an .ext4 slot image or a block device holding one
        image: PathBuf,
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
        Commands::Update { command } => match command {
            UpdateCommands::Status => update::status(),
            UpdateCommands::Install { image } => update::install(&image),
            UpdateCommands::MarkGood => update::mark_good(),
            UpdateCommands::Rollback => update::rollback(),
        },
    }
}
