# The `edel` tool

The one command-line tool of Edel OS and the library `edel::system`: the only crate of the workspace. Edition 2024, `rust-version = "1.85"`, GPL-3.0-or-later, `publish = false`.

## What belongs here

- Every system function lands here as a subcommand (`edel update`, `edel boot`, `edel release`, `edel system`, `edel install`), never as a separate tool: "one tool to learn" (`main.rs`). Add-ons, the live USB and fleet images are built by the same `edel image build`, not by a new builder.
- Settings change only through `edel system set` and `unset`. Never add a per-domain verb that changes settings (no `edel addon add`), shell completion scripts, telemetry, runtime-loaded code or a plugin API (ADR-008).
- The library target (`lib.rs`, today only `edel::system`) is the one parser for system files. The compositor (M4.5), shell-ui (M5.1) and Settings (M5.6) link it, so it has no network or signing dependencies: `clap`, `ed25519-dalek`, `flate2`, `sha2` and `ureq` are optional and sit behind the binary's default `cli` feature (ADR-008). CI builds the library with `--no-default-features`; code the library needs never uses them.

## Fast checks

Run the four "Rust checks" commands from the root CLAUDE.md before every push. After touching `image.rs`, `run.rs` or `def.rs`, also run `cargo run -- image build images/vm.toml --dry-run` and the same for `images/container.toml`. Run the tool with `cargo run -- ...`, never a binary left in `target/`: a stale build rejects newer definitions with errors such as "unknown field `cmdline`".

## Layout

- `lib.rs`: the library; `system.rs`: `system.toml` format 1, the key table `KEYS` (path, kind, supported), the structs, `read` (lenient: unknown keys and wrong values are dropped and reported), `check` (strict, also refuses keys not supported yet) and `read_on_machine` (a newer format reads `system.toml.v<N>` beside it, else fails).
- `install.rs` (library): the install plan, `edel::install::Plan`: the target disk's model, size and partitions (`sda3: Windows (NTFS, 420 GB)`) and the layout to write; Settings' installer page shows the same plan (M6.6a).
- `installer.rs`: `edel install DISK --system FILE` (M2.4): refuses the running disk, a mounted disk and a system file with any problem; shows the plan for `--dry-run`, before asking for the disk's name on a terminal, and before exit 3 with neither a terminal nor `--yes`; then partitions with `boot::Layout`, copies the running slot to slot A (`e2fsck -fp`, `tune2fs -U random`), writes the ESP from `/usr/lib/edel/boot` with an initial grubenv, and creates `edel-data` holding the system file. `TOOLS` lists the tools it runs; a test holds `images/vm.toml` to it.
- `main.rs`: the clap derive CLI and its dispatch. Doc comments on variants and fields are the `--help` text: write them for users.
- `def.rs`: image definitions (`ImageDef`, `FORMAT`, validation).
- `image.rs`: `edel image build`; `Build::run` calls `prepare_root`, `install_packages`, `copy_files`, `enable_services`, `configure`, then `pack_container` or `pack_vm`.
- `boot.rs`: the A/B disk layout and `grub.cfg`.
- `grubenv.rs`: GRUB's 1024-byte environment block and the `Slot` type, shared by `boot.rs` and `update.rs`.
- `machine.rs`: `edel system apply|diff|export|set|unset` (M2.2, M2.3): `plan` (pure) turns the file and a `Machine` snapshot into `Change`s, which diff prints and apply executes; seeds `/data/edel/system.toml` on first boot (`EDEL-SEED` volume, the ESP's `/EFI/edel/system.toml`, the slot's `/usr/share/edel/system.toml`), applies the hostname, users (busybox `adduser`, the `admin` group, `*` for no password, never `!`), `~/.ssh/authorized_keys` and the developer flag `/data/edel/developer`; pure helpers for passwd, group and shadow lines; `describe` builds the export; set and unset edit through `system::set` and `system::unset` (`toml_edit`, byte for byte) and never apply.
- `update.rs`: `edel update status|install|mark-good|rollback`; pure transitions (`before_install`, `after_install`, `confirm`, `roll_back`) and slot discovery (`find_disk`, tested on a fake sysfs tree).
- `data.rs`: `edel boot mount-data`, which grows the data partition on first boot, mounts it at `/data` and overlays `/etc` (merging new system users first), binds `/home` and `/var` onto it after topping up `/data/var` from the slot, and mounts `/tmp` as tmpfs.
- `guard.rs`: `edel boot guard`, which pets the watchdog until the health files in `EDEL_HEALTH` exist, then confirms the slot and stops the watchdog with the magic close, or lets it reset the machine after `EDEL_HEALTH_TIMEOUT`.
- `release.rs`: `release.toml` (format 1, read leniently), ed25519 keys and signatures, `edel release keygen|make|sign|verify`, and `checked_image`, which `edel update install` calls before writing a slot.
- `loader.rs`: the boot loader riding along (M1.8): `loader.toml`, `swap_file` (`.prev`, `.new`, rename) and `update_esp`, which `mark-good` calls once the slot is confirmed.
- `run.rs`: `Runner` (dry run, external tools), kernel filesystem mounts with `MountGuard`, `ensure_nothing_mounted_under`.

## Conventions

- Today the dependencies are `anyhow`, `serde`, `serde_ignored`, `toml` and `toml_edit` for the library, and `clap`, `ed25519-dalek`, `flate2`, `sha2` and `ureq` (rustls with ring, webpki roots) for the command, with no dev-dependencies. The roadmap names the next ones. Every new one, those included, needs a one-paragraph reason in the PR (principle 3) and its `Cargo.lock` change (CI uses `--locked`).
- Release binaries are built by Alpine 3.24's `cargo` package inside `ci/build.sh` (musl), not only by the runner's Rust. Code must build with both. Crates the roadmap marks "check in CI" (the `ed25519-dalek` musl build in M1.6, rustls with the ring provider in M1.7, `serde_ignored` in M2.1) are proven by that step's first CI run.
- Errors: `anyhow` only, no custom error types. Return `Result`, fail with `bail!`, add `.context(...)` or `.with_context(...)`. Messages are lower case, have no final period, quote values with `{:?}` and say what to do: "the image has no /usr/lib/os-release; add one to the files directories". Progress goes to stdout with `println!`; non-fatal problems go to stderr as `warning: ...`.
- No `unwrap` outside tests; the one `expect` (in `boot.rs`) says why it cannot fail. No `unsafe`, no `#[allow]`, no lint config: CI's `clippy -D warnings` and `fmt --check` are the rules.
- Each file opens with a `//!` doc, public items have `///` docs, and comments say why and cite the ADR they implement ("(stateless /etc, ADR-006)"). Imports come in three groups separated by blank lines: `std`, external crates, `crate::`. Use inline format captures (`{FORMAT}`, `{hostname:?}`).

## Rules that protect real machines

- In `edel image build`, and in any other command that has `--dry-run`, run every external tool through `Runner::run` or `Runner::run_with_input`; print work edel does itself with `Runner::step` and guard it with `if !self.runner.dry_run` or an early return. A dry run changes nothing and needs no root, Alpine or network. `Runner::run` captures no output and treats any non-zero exit as failure, so commands without a dry run, such as `edel update`, call tools with `std::process::Command` directly and check the exit status themselves.
- Image builds use no loop devices and mount no images (`mkfs.ext4 -d`, `mkfs.vfat` with mtools, `sfdisk`, sparse copies at byte offsets). The only mounts are proc, sys and dev from `mount_kernel_fs`, undone by `MountGuard`. Never delete a work directory without `ensure_nothing_mounted_under`: a live bind of `/dev` inside it would delete the host's devices.
- Disk layout: partition 1 is the EFI system partition (`EDEL-ESP`, 64 MiB), 2 is slot A, 3 is slot B, 4 is `edel-data` (64 MiB in the image, grown with `sfdisk` and `partx` on first boot); GRUB and the updater rely on it. No two filesystems a machine can see may share a UUID: slot A gets its own mkfs, and a written slot gets `tune2fs -U random`.
- The grubenv block is exactly 1024 bytes, rewritten in place, and keeps RAUC's names: `ORDER`, `A_OK`, `A_TRY`, `B_OK`, `B_TRY`. GRUB menu entries are numbered because GRUB 2.12 ignores an entry id in `fallback`. `vm.cmdline` is pasted into `grub.cfg`; keep it behind `boot::is_safe_cmdline`.

## Updater rules (M1.1 and every later updater step)

- The running slot comes from the mounted root (partition 2 is A, 3 is B), never from the kernel command line, and the running slot is never written.
- Install switches the target off (`<slot>_OK=0`) before writing it, checks the image's size against the slot, writes it, drops the cache (`blockdev --flushbufs`), checks the write by sha256, then runs `e2fsck -fp` and `tune2fs -U random`. `e2fsck -fp` exiting 0 or 1 is success.
- IMAGE may be a block device (ab-test passes the second disk), so size it by seeking to its end (file metadata reports 0 for a device), and hash only the first <image size> bytes of the slot.
- `mark-good` detects a fallback: when ORDER does not start with the running slot but with the other one, GRUB passed over that slot. It then logs `slot X did not start, so slot Y is running instead; switching slot X off` and sets `X_OK=0`. In every case it then sets ORDER to the running slot first, with `OK=1` and `TRY=0`. It records that fallback in `/data/edel/last-fallback.toml` (format, from, to, date) for shell-ui to show once (M5.9).
- One updater at a time: install, mark-good and rollback take `/run/edel/update.lock` (`update::Lock`; `edel system apply` takes `system.lock` the same way). Mount the EFI system partition only while writing it, never in fstab, with `-o noatime,iocharset=iso8859-1` (the virt kernel lacks utf8 for FAT). Rewrite grubenv in place, as `dd ... bs=1024 count=1 conv=notrunc,fsync` does.

## Formats: strict for builders, lenient on machines

- Every file format has a top-level integer `format`, from version 1. Image definitions are format 1 (`def::FORMAT`); bump it only for a breaking change. Every definition lives in this repository, so the bump converts them in the same PR and refuses the old format (M4.0); migration (`edel migrate`) is for files on machines, such as `system.toml`.
- Image definitions and presets stay strict (`#[serde(deny_unknown_fields)]`): only CI and the slot that ships them read them.
- Files a machine reads from another release (`release.toml` from M1.6, `system.toml` from M2.1, feature files) are lenient. Before writing one, read the reader table in [ADR-008](../../docs/ADR-008-features-defaults-and-install.md), section 2. In short: no `deny_unknown_fields`; collect unknown keys with `serde_ignored` and report them; read enum values leniently; write with `toml_edit` so unknown keys and comments survive. Refuse only a format you do not know.
- A key is never removed or renamed within a format. From M2.1 a cargo test checks the structs against the append-only `crates/edel/tests/keys.txt`. A dropped key gets `retired = "VERSION"` in the help table next to the structs (M6.11): it is still parsed, reported as no longer used and refused by `set`, until `edel migrate` drops it at the next format bump.
- Name settings after what the user sees (`services.ssh`, `shell.tiling`). An absent key means the release decides. Resolve every key in one order: user file, machine file, profiles in listed order, preset, release default. Writers never write a default; apply enforces the release default for every absent key, so `unset` really undoes; `edel system get KEY` prints the value and its layer (ADR-008, M2.3, M5.6, M6.11).

## Must keep working offline

- `cargo test`, `edel image check` and `--dry-run` need no network. A failed fetch leaves the slot off and the running system untouched.
- `edel update install` keeps accepting a local path or a `file://` URL for USB installs next to network URLs (M1.7). USB and offline installs, add-ons included, keep working (M7.2a).
- The boot service `edel-system` applies only the offline sections of `system.toml`. Sections that need the network (`[apps]`) run only when a user or Settings calls apply, in the background, never from the boot service (M2.2, M6.1).

## Tests

- Unit tests close each file: `#[cfg(test)] mod tests { use super::*; ... }`. Names are sentences in snake_case: `rejects_unknown_fields`, `grub_counts_every_try`. `tests/` holds only `keys.txt`, the append-only list of system file keys that `system.rs` tests read; there are no integration tests.
- Put logic in small pure functions that take text and numbers, so tests need no root, disk, Alpine or network; what needs Alpine, root or a VM is proven by the `ci/` scripts in CI. `def.rs` tests parse a `const VM: &str` and vary it with `.replace(...)`.
- A test that needs files works under `std::env::temp_dir()` in a directory named after that test and the process id (`edel-copy-sparse-<pid>` in `copies_into_place_and_keeps_holes`), because cargo runs tests in parallel threads of one process, and removes it afterwards.
