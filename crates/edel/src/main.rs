//! `edel`: the Edel OS system tool.
//!
//! Today it builds images. Updates, rollback, add-ons and system files
//! (ADR-006, ADR-007) will live here too, so there is one tool to learn.

mod boot;
mod def;
mod image;
mod run;

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
    }
}
