//! `edel`: the Edel OS system tool.
//!
//! It builds images, updates and rolls back A/B slots and checks system
//! files. Add-ons (ADR-007) will live here too, so there is one tool to
//! learn. The settings file parser is the library half (`edel::settings`).

mod boot;
mod completion;
mod data;
mod def;
mod grubenv;
mod guard;
mod image;
mod installer;
mod live;
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
use edel::settings;

use crate::def::ImageDef;
use crate::run::Runner;

#[derive(Parser)]
#[command(name = "edel", version, about = "The Edel OS system tool")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

/// `edel`'s command table, for what is written from it: the completion
/// (M5.26) and the command reference (M8.10a).
pub(crate) fn cli_command() -> clap::Command {
    use clap::CommandFactory;
    Cli::command()
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
        /// Only check the version RELEASE holds against this one; change
        /// nothing
        #[arg(long, conflicts_with = "unsigned")]
        check: bool,
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
    /// Show the version running now and the one in the other slot, the
    /// desktop's effect tier while a session runs, and which log to read
    /// when the last boot or desktop session went wrong
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
    /// becomes slot A, and FILE the new machine's settings
    Install {
        /// The disk, such as /dev/sda
        disk: String,
        /// The settings file the new machine starts with
        #[arg(long, value_name = "FILE")]
        settings: PathBuf,
        /// Only show the plan: what would be erased and written; change
        /// nothing
        #[arg(long)]
        plan: bool,
        /// Erase the disk without asking, for unattended installs; without
        /// it and without a terminal, install shows its plan and exits 3
        #[arg(long)]
        yes: bool,
    },
    /// Print what an issue about this machine needs, as TOML: the release,
    /// kernel, boot time, memory in use, PCI devices, the kernel log, and
    /// the last lines of the system log and of the desktop sessions' logs
    Report {
        /// Write it to /EFI/edel/report.toml on the EFI system partition
        /// instead, where any computer can read it from the disk or stick
        #[arg(long)]
        esp: bool,
    },
    /// settings: this machine's settings (its configuration), the same
    /// as in the Settings app; alone, it lists the pages
    Settings {
        #[command(subcommand)]
        command: Option<SettingsCommands>,
    },
    /// Build and inspect Edel OS images
    Image {
        #[command(subcommand)]
        command: ImageCommands,
    },
}

#[derive(Subcommand)]
enum SettingsCommands {
    /// Show settings with their values and where each comes from: one
    /// key, one page (such as layout), or every page
    Get {
        /// A key such as network.hostname, or a page such as layout
        #[arg(value_name = "KEY|PAGE")]
        what: Option<String>,
        /// Print the settings file's own TOML, for scripts
        #[arg(long)]
        toml: bool,
    },
    /// Change settings, keeping everything else in the file as it was;
    /// the desktop follows its settings at once, apply makes the rest
    /// take effect
    Set {
        /// KEY=VALUE, one or more, such as network.hostname=lab-1
        #[arg(value_name = "KEY=VALUE", required = true)]
        assignments: Vec<String>,
    },
    /// Give settings back to the release, as the Settings app's Reset
    /// does
    Reset {
        /// The keys, such as network.hostname
        #[arg(value_name = "KEY", required = true)]
        keys: Vec<String>,
    },
    /// Show what apply would change, changing nothing; exits 1 when
    /// there is anything to change
    Diff {
        /// A settings file; without one, the machine's own
        file: Option<PathBuf>,
    },
    /// Make this machine match its settings: the hostname, people, their
    /// ssh keys and developer mode
    Apply {
        /// Leave setting the running hostname to the hostname service; for
        /// the edel-settings boot service
        #[arg(long, hide = true)]
        boot: bool,
    },
    /// Make FILE this machine's settings and apply it, such as a file
    /// exported on another machine
    Import {
        /// The settings file
        file: PathBuf,
    },
    /// Print this machine as a settings file, to keep or to import on
    /// another machine, and the /etc files it changed
    Export,
    /// Check a settings file strictly: every unknown key, value or format,
    /// and every key this release does not act on yet, is refused
    Check {
        /// The settings file
        file: PathBuf,
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
    /// On a USB stick, or with nobody set up yet, log the person called
    /// live in to the desktop by itself
    Live,
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
    /// desktop stick's small slots
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
    /// Only show the plan: every step, changing nothing
    #[arg(long)]
    plan: bool,
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
                    plan,
                } = *args;
                if let Some(v) = version.as_deref().filter(|v| !image::is_version(v)) {
                    anyhow::bail!("version {v:?} is not numbers joined by dots, such as 2026.10.3");
                }
                let def = ImageDef::load(&definition)?;
                let def_dir = definition.parent().map(PathBuf::from).unwrap_or_default();
                // apk and mount get absolute paths, so nothing depends on
                // which directory a tool happens to run in.
                let out = if plan {
                    out
                } else {
                    std::fs::create_dir_all(&out)?;
                    std::fs::canonicalize(&out)?
                };
                // apk opens a relative cache directory inside the new root.
                let apk_cache = match apk_cache {
                    Some(cache) if !plan => {
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
                    runner: Runner { dry_run: plan },
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
            BootCommands::Live => live::live(),
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
            check: true,
            ..
        } => release::check(&location),
        Commands::Update {
            location,
            check: false,
            allow_downgrade,
            unsigned,
        } => update::install(&location, allow_downgrade, unsigned),
        Commands::Rollback => update::rollback(),
        Commands::Status => update::status(),
        Commands::Install {
            disk,
            settings,
            plan,
            yes,
        } => installer::install(&disk, &settings, plan, yes),
        Commands::Report { esp } => report::report(esp),
        Commands::Settings { command } => match command {
            None => {
                machine::pages();
                Ok(())
            }
            Some(SettingsCommands::Get { what, toml }) => machine::get(what.as_deref(), toml),
            Some(SettingsCommands::Set { assignments }) => machine::set(&assignments),
            Some(SettingsCommands::Reset { keys }) => machine::reset(&keys),
            Some(SettingsCommands::Diff { file }) => {
                if machine::diff(file.as_deref())? {
                    std::process::exit(1);
                }
                Ok(())
            }
            Some(SettingsCommands::Apply { boot }) => machine::apply(boot),
            Some(SettingsCommands::Import { file }) => machine::import(&file),
            Some(SettingsCommands::Export) => machine::export(),
            Some(SettingsCommands::Check { file }) => check_system_file(&file),
        },
    }
}

/// `edel settings check FILE`: prints one line per problem and fails when
/// there is any (ADR-008, section 2).
fn check_system_file(file: &std::path::Path) -> Result<()> {
    let text =
        std::fs::read_to_string(file).with_context(|| format!("reading {}", file.display()))?;
    let problems =
        settings::check(&text).with_context(|| format!("checking {}", file.display()))?;
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
            "update", "rollback", "status", "install", "report", "settings", "image",
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

    /// Showing first reads plainly (Alimardon's choice): update checks,
    /// install and image build show their plan.
    #[test]
    fn show_first_flags() {
        for args in [
            &["edel", "update", "--check", "release.toml"][..],
            &[
                "edel",
                "install",
                "/dev/sda",
                "--settings",
                "s.toml",
                "--plan",
            ],
            &["edel", "image", "build", "images/vm.toml", "--plan"],
        ] {
            assert!(Cli::try_parse_from(args).is_ok(), "{args:?}");
        }
        let both = Cli::try_parse_from(["edel", "update", "--check", "--unsigned", "slot.img"]);
        assert!(both.is_err());
    }

    /// The docs site's command reference (M8.10a), written from this
    /// command table, so the page says what `--help` says.
    const COMMANDS_FILE: &str =
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/guide/commands.md");

    /// One line of help text, its `|` escaped for a table cell.
    fn cell(text: impl ToString) -> String {
        text.to_string()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .replace('|', "\\|")
    }

    /// `edel NAME ARGS`: the command line a person types.
    fn usage(path: &str, cmd: &clap::Command) -> String {
        let mut line = path.to_string();
        for arg in cmd.get_positionals().filter(|a| !a.is_hide_set()) {
            let name = arg.get_value_names().and_then(|n| n.first()).map_or_else(
                || arg.get_id().to_string().to_uppercase(),
                |n| n.to_string(),
            );
            let required = arg.is_required_set();
            let name = if matches!(arg.get_action(), clap::ArgAction::Append) {
                format!("{name}...")
            } else {
                name
            };
            line.push_str(&if required {
                format!(" {name}")
            } else {
                format!(" [{name}]")
            });
        }
        if cmd.get_opts().any(|a| !a.is_hide_set()) {
            line.push_str(" [OPTIONS]");
        }
        line
    }

    /// The reference for `cmd`, called `path`, and for its subcommands.
    fn describe(path: &str, cmd: &clap::Command, depth: usize, out: &mut String) {
        let subcommands: Vec<&clap::Command> =
            cmd.get_subcommands().filter(|c| !c.is_hide_set()).collect();
        if depth > 0 {
            let hashes = "#".repeat(depth + 1);
            out.push_str(&format!("\n{hashes} `{}`\n\n", usage(path, cmd)));
            if let Some(about) = cmd.get_about() {
                out.push_str(&format!("{}.\n", cell(about)));
            }
            let args: Vec<&clap::Arg> = cmd.get_arguments().filter(|a| !a.is_hide_set()).collect();
            if !args.is_empty() {
                out.push_str("\n| | What it does |\n|---|---|\n");
                for arg in args {
                    let value = arg
                        .get_value_names()
                        .and_then(|n| n.first())
                        .map(|n| n.to_string());
                    let name = match (arg.get_long(), value) {
                        (Some(long), Some(value)) if arg.get_action().takes_values() => {
                            format!("--{long} {value}")
                        }
                        (Some(long), _) => format!("--{long}"),
                        (None, Some(value)) => value,
                        (None, None) => arg.get_id().to_string().to_uppercase(),
                    };
                    let help = arg.get_help().map(cell).unwrap_or_default();
                    out.push_str(&format!("| `{name}` | {help} |\n"));
                }
            }
        }
        for sub in subcommands {
            describe(&format!("{path} {}", sub.get_name()), sub, depth + 1, out);
        }
    }

    fn command_reference() -> String {
        let cli = Cli::command();
        let mut out = String::from(
            "<!-- Written by a cargo test from edel's own command table,\n     \
             crates/edel/src/main.rs; never edit it by hand: change the\n     \
             table and run EDEL_WRITE_DOCS=1 cargo test -p edel command_reference. -->\n\n\
             # Commands\n\n\
             `edel` is the one tool of Edel OS: it updates and rolls back the system, \
             applies and describes the settings file, installs Edel OS on a disk and \
             reports on the hardware. Each command below says what `edel COMMAND --help` \
             says. Commands that change the machine need root until `doas` arrives \
             (roadmap M6.5).\n\n\
             | Command | What it does |\n|---|---|\n",
        );
        for sub in cli.get_subcommands().filter(|c| !c.is_hide_set()) {
            let name = format!("edel {}", sub.get_name());
            let about = sub.get_about().map(cell).unwrap_or_default();
            out.push_str(&format!("| `{name}` | {about} |\n"));
        }
        describe("edel", &cli, 0, &mut out);
        out
    }

    #[test]
    fn the_command_reference_is_the_command_table() {
        let reference = command_reference();
        if std::env::var_os("EDEL_WRITE_DOCS").is_some() {
            std::fs::write(COMMANDS_FILE, &reference).unwrap();
        }
        let shipped = std::fs::read_to_string(COMMANDS_FILE).unwrap_or_default();
        assert!(
            shipped == reference,
            "docs/guide/commands.md differs from the command table; run EDEL_WRITE_DOCS=1 cargo test -p edel command_reference"
        );
        assert!(reference.contains("## `edel update RELEASE [OPTIONS]`"));
        assert!(
            !reference.contains("edel boot"),
            "a hidden command is listed"
        );
    }
}
