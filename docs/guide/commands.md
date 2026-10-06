<!-- Written by a cargo test from edel's own command table,
     crates/edel/src/main.rs; never edit it by hand: change the
     table and run EDEL_WRITE_DOCS=1 cargo test -p edel command_reference. -->

# Commands

`edel` is the one tool of Edel OS: it updates and rolls back the system, applies and describes the system file, installs Edel OS on a disk and reports on the hardware. Each command below says what `edel COMMAND --help` says. Commands that change the machine need root until `doas` arrives (roadmap M6.5).

| Command | What it does |
|---|---|
| `edel update` | Install a newer Edel OS into the other slot from RELEASE; it starts at the next restart, and falls back on its own if it fails |
| `edel rollback` | Go back to the version in the other slot, the one before the last update; it starts at the next restart |
| `edel status` | Show the version running now and the one in the other slot |
| `edel install` | Install Edel OS on another disk, erasing it: the running system becomes slot A, and FILE the new machine's system file |
| `edel report` | Print what an issue about this machine needs, as TOML: the release, kernel, boot time, memory in use, PCI devices and the kernel log |
| `edel system` | This machine's settings, kept in one file (system.toml): check, change, apply and export them |
| `edel shell` | What the desktop session is doing |
| `edel image` | Build and inspect Edel OS images |

## `edel update RELEASE [OPTIONS]`

Install a newer Edel OS into the other slot from RELEASE; it starts at the next restart, and falls back on its own if it fails.

| | What it does |
|---|---|
| `RELEASE` | The release: its release.toml, a path or an http(s) URL (its .sig and image beside it), or with --unsigned a slot image or a block device holding one |
| `--check` | Only check the version RELEASE holds against this one; change nothing |
| `--allow-downgrade` | Install even when the release is not newer than this system |
| `--unsigned` | Install a slot image without a signed release.toml (for testing) |

## `edel rollback`

Go back to the version in the other slot, the one before the last update; it starts at the next restart.

## `edel status`

Show the version running now and the one in the other slot.

## `edel install DISK [OPTIONS]`

Install Edel OS on another disk, erasing it: the running system becomes slot A, and FILE the new machine's system file.

| | What it does |
|---|---|
| `DISK` | The disk, such as /dev/sda |
| `--system FILE` | The system file the new machine starts with |
| `--plan` | Only show the plan: what would be erased and written; change nothing |
| `--yes` | Erase the disk without asking, for unattended installs; without it and without a terminal, install shows its plan and exits 3 |

## `edel report [OPTIONS]`

Print what an issue about this machine needs, as TOML: the release, kernel, boot time, memory in use, PCI devices and the kernel log.

| | What it does |
|---|---|
| `--esp` | Write it to /EFI/edel/report.toml on the EFI system partition instead, where any computer can read it from the disk or stick |

## `edel system`

This machine's settings, kept in one file (system.toml): check, change, apply and export them.

### `edel system check FILE`

Check a system file strictly: every unknown key, value or format, and every key this release does not act on yet, is refused.

| | What it does |
|---|---|
| `FILE` | The system file, for example /data/edel/system.toml |

### `edel system apply [FILE] [OPTIONS]`

Make this machine match a system file: hostname, users, their ssh keys and developer mode. A given file becomes the machine's own.

| | What it does |
|---|---|
| `FILE` | The system file; without one, the machine's own (/data/edel/system.toml), seeded first if it is missing |
| `--boot` | Leave setting the running hostname to the hostname service; for the edel-system boot service |

### `edel system diff [FILE]`

Show what apply would change, changing nothing; exits 1 when there is anything to change.

| | What it does |
|---|---|
| `FILE` | The system file; without one, the machine's own |

### `edel system export`

Print this machine as a system file, and the /etc files it changed.

### `edel system set ASSIGNMENT`

Change one key in the machine's system file, keeping everything else in it byte for byte. Window, screen and shortcut settings take effect at once; apply makes the rest take effect.

| | What it does |
|---|---|
| `ASSIGNMENT` | KEY=VALUE, such as network.hostname=lab-1 |

### `edel system unset KEY`

Remove one key from the machine's system file, so the release decides it again. Window, screen and shortcut settings take effect at once; apply makes the rest take effect.

| | What it does |
|---|---|
| `KEY` | The key, such as network.hostname |

## `edel shell`

What the desktop session is doing.

### `edel shell tier`

Print the effect tier the compositor runs at: lite, balanced or full.

## `edel image`

Build and inspect Edel OS images.

### `edel image build DEFINITION [OPTIONS]`

Build an image from its definition file (run as root on Alpine).

| | What it does |
|---|---|
| `DEFINITION` | Image definition, for example images/vm.toml |
| `--out OUT` | Directory for the finished images and the work area |
| `--files DIR` | Another directory to copy over the image, after the definition's own files; for test images. Can be given more than once |
| `--health-timeout SECS` | Seconds the boot guard waits for health before the watchdog restarts the machine, instead of the definition's; for test images |
| `--slot-mib MIB` | Size of each slot in MiB, instead of the definition's; for test images, such as the install test's live disk, which has the laptop stick's small slots |
| `--public-key FILE` | Another public key that may sign updates, after the definition's own; for test images. Can be given more than once |
| `--loader-tag TAG` | A tag written into grub.cfg so the boot loader differs from an untagged build; for test images |
| `--version VERSION` | The release version written into the image, such as 2026.10.3; without it the image keeps its development version |
| `--channel CHANNEL` | The channel written into the image, such as preview or stable |
| `--apk-cache DIR` | An apk cache to share between the images of one build, so they all install from one package index |
| `--no-compress` | Skip the .gz copy of a VM image's disk (the slot's .gz, the update image, is always made); for test images |
| `--plan` | Only show the plan: every step, changing nothing |

### `edel image check DEFINITION`

Check an image definition without building it.

| | What it does |
|---|---|
| `DEFINITION` | Image definition, for example images/vm.toml |
