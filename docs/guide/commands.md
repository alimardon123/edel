<!-- Written by a cargo test from edel's own command table,
     crates/edel/src/main.rs; never edit it by hand: change the
     table and run EDEL_WRITE_DOCS=1 cargo test -p edel command_reference. -->

# Commands

`edel` is the one tool of Edel OS: it updates and rolls back the system, applies and describes the settings file, installs Edel OS on a disk and reports on the hardware. Each command below says what `edel COMMAND --help` says. Commands that change the machine need root until `doas` arrives (roadmap M6.5).

| Command | What it does |
|---|---|
| `edel update` | Install a newer Edel OS into the other slot from RELEASE; it starts at the next restart, and falls back on its own if it fails |
| `edel rollback` | Go back to the version in the other slot, the one before the last update; it starts at the next restart |
| `edel status` | Show the version running now and the one in the other slot, the desktop's effect tier while a session runs, and which log to read when the last boot or desktop session went wrong |
| `edel install` | Install Edel OS on another disk, erasing it: the running system becomes slot A, and FILE the new machine's settings |
| `edel report` | Print what an issue about this machine needs, as TOML: the release, kernel, boot time, memory in use, PCI devices, the kernel log, and the last lines of the system log and of the desktop sessions' logs |
| `edel settings` | settings: this machine's settings (its configuration), the same as in the Settings app; alone, it lists the pages |
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

Show the version running now and the one in the other slot, the desktop's effect tier while a session runs, and which log to read when the last boot or desktop session went wrong.

## `edel install DISK [OPTIONS]`

Install Edel OS on another disk, erasing it: the running system becomes slot A, and FILE the new machine's settings.

| | What it does |
|---|---|
| `DISK` | The disk, such as /dev/sda |
| `--settings FILE` | The settings file the new machine starts with |
| `--plan` | Only show the plan: what would be erased and written; change nothing |
| `--yes` | Erase the disk without asking, for unattended installs; without it and without a terminal, install shows its plan and exits 3 |

## `edel report [OPTIONS]`

Print what an issue about this machine needs, as TOML: the release, kernel, boot time, memory in use, PCI devices, the kernel log, and the last lines of the system log and of the desktop sessions' logs.

| | What it does |
|---|---|
| `--esp` | Write it to /EFI/edel/report.toml on the EFI system partition instead, where any computer can read it from the disk or stick |

## `edel settings`

settings: this machine's settings (its configuration), the same as in the Settings app; alone, it lists the pages.

### `edel settings get [KEY|PAGE] [OPTIONS]`

Show settings with their values and where each comes from: one key, one page (such as layout), or every page.

| | What it does |
|---|---|
| `KEY|PAGE` | A key such as network.hostname, or a page such as layout |
| `--toml` | Print the settings file's own TOML, for scripts |

### `edel settings set KEY=VALUE...`

Change settings, keeping everything else in the file as it was; the desktop follows its settings at once, apply makes the rest take effect.

| | What it does |
|---|---|
| `KEY=VALUE` | KEY=VALUE, one or more, such as network.hostname=lab-1 |

### `edel settings reset KEY...`

Give settings back to the release, as the Settings app's Reset does.

| | What it does |
|---|---|
| `KEY` | The keys, such as network.hostname |

### `edel settings diff [FILE]`

Show what apply would change, changing nothing; exits 1 when there is anything to change.

| | What it does |
|---|---|
| `FILE` | A settings file; without one, the machine's own |

### `edel settings apply`

Make this machine match its settings: the hostname, people, their ssh keys and developer mode.

### `edel settings import FILE`

Make FILE this machine's settings and apply it, such as a file exported on another machine.

| | What it does |
|---|---|
| `FILE` | The settings file |

### `edel settings export`

Print this machine as a settings file, to keep or to import on another machine, and the /etc files it changed.

### `edel settings check FILE`

Check a settings file strictly: every unknown key, value or format, and every key this release does not act on yet, is refused.

| | What it does |
|---|---|
| `FILE` | The settings file |

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
| `--slot-mib MIB` | Size of each slot in MiB, instead of the definition's; for test images, such as the install test's live disk, which has the desktop stick's small slots |
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
