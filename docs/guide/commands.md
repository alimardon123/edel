<!-- Written by a cargo test from edel's own command table,
     crates/edel/src/main.rs; never edit it by hand: change the
     table and run EDEL_WRITE_DOCS=1 cargo test -p edel command_reference. -->

# Commands

`edel` is the one tool of Edel OS: it updates and rolls back the system, applies and describes the settings file, installs Edel OS on a disk and reports on the hardware. Each command below says what `edel COMMAND --help` says. Commands that change the machine need root: an administrator runs them with `doas edel COMMAND`, and some ask the administrator's password first, as `edel rollback` does (roadmap M6.5).

| Command | What it does |
|---|---|
| `edel update` | Install a newer Edel OS into the other slot from RELEASE, or from this machine's channel when none is named; it starts at the next restart, and falls back on its own if it fails |
| `edel rollback` | Go back to the version in the other slot, the one before the last update; it starts at the next restart |
| `edel status` | Show the version running now and the one in the other slot, the desktop's effect tier while a session runs, and which log to read when the last boot or desktop session went wrong |
| `edel restart` | Restart this machine now: the system's own reboot, through doas when you are not root |
| `edel shutdown` | Shut this machine down now: the system's own poweroff, through doas when you are not root |
| `edel suspend` | Suspend this machine to memory now; a key or the power button wakes it |
| `edel install` | Install Edel OS on another disk, erasing it: the running system becomes slot A, and FILE the new machine's settings |
| `edel report` | Print what an issue about this machine needs, as TOML: the release, kernel, boot time, memory in use, PCI devices, the kernel log, and the last lines of the system log and of the desktop sessions' logs |
| `edel settings` | settings: this machine's settings (its configuration), the same as in the Settings app; alone, it lists the pages |
| `edel image` | Build and inspect Edel OS images |
| `edel theme` | theme: a look as a set of pictures and appearance values, built in or your own; the Appearance page of Settings shows the same ones |
| `edel preset` | preset: a layout, built in or your own, kept as the layout in use; the Layout page of Settings shows the same ones |
| `edel web-app` | web-app: a website made an app, with its own window, name and icon in the launcher; it runs in Firefox with a profile of its own |

## `edel update [RELEASE] [OPTIONS]`

Install a newer Edel OS into the other slot from RELEASE, or from this machine's channel when none is named; it starts at the next restart, and falls back on its own if it fails.

| | What it does |
|---|---|
| `RELEASE` | The release: its release.toml, a path or an http(s) URL (its .sig and image beside it), or with --unsigned a slot image or a block device holding one; without it, the list of the channel this machine follows (updates.channel), or of --channel |
| `--check` | Only check the version RELEASE holds against this one; change nothing |
| `--allow-downgrade` | Install even when the release is not newer than this system |
| `--channel CHANNEL` | Take a release list for this channel once, rather than the one this machine follows (updates.channel) |
| `--allow-http` | Read RELEASE over plain http (for a local test server) |
| `--unsigned` | Install a slot image without a signed release.toml (for testing) |

## `edel rollback`

Go back to the version in the other slot, the one before the last update; it starts at the next restart.

## `edel status`

Show the version running now and the one in the other slot, the desktop's effect tier while a session runs, and which log to read when the last boot or desktop session went wrong.

## `edel restart`

Restart this machine now: the system's own reboot, through doas when you are not root.

## `edel shutdown`

Shut this machine down now: the system's own poweroff, through doas when you are not root.

## `edel suspend`

Suspend this machine to memory now; a key or the power button wakes it.

## `edel install [DISK] [OPTIONS]`

Install Edel OS on another disk, erasing it: the running system becomes slot A, and FILE the new machine's settings.

| | What it does |
|---|---|
| `DISK` | The disk, such as /dev/sda; without it, the disks are listed and one is asked for |
| `--settings FILE` | The settings file the new machine starts with, a path or an https:// address; without it, a few questions are asked on the terminal and their answers become it |
| `--list` | List the disks Edel OS could install on, and change nothing |
| `--find-settings` | Look on every other disk for a settings file to install with (the settings file, at its top or in EFI/edel) and print it, changing nothing |
| `--plan` | Only show the plan: what would be erased and written; change nothing |
| `--yes` | Erase the disk without asking, for unattended installs; without it and without a terminal, install shows its plan and exits 3 |
| `--new-password-stdin` | Read the password of the settings file's one person from the first line of standard input (after the administrator's, with --password-stdin), as Settings' installer page gives it |

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
| `FILE` | The settings file, a path or an https:// address |

### `edel settings export`

Print this machine as a settings file, to keep or to import on another machine, and the /etc files it changed.

### `edel settings check FILE`

Check a settings file strictly: every unknown key, value or format, and every key this release does not act on yet, is refused.

| | What it does |
|---|---|
| `FILE` | The settings file, a path or an https:// address |

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
| `--apps-from URL` | Where the default apps come from: a .flatpakrepo file's address or a Flatpak repository's, such as file:///src/out/apps-repo for a test; Flathub when not given |
| `--apps-url URL` | The address machines update the default apps from, when it is not where the build took them, as for a test image built from a local repository that its VM reaches over HTTP |
| `--app ID` | A default app to install instead of the definition's list; for test images. Can be given more than once |
| `--dropped-app ID` | A default app this image drops, kept for each person who used it, instead of the definition's; for test images. Can be given more than once |
| `--no-compress` | Skip the .gz copy of a VM image's disk (the slot's .gz, the update image, is always made); for test images |
| `--plan` | Only show the plan: every step, changing nothing |

### `edel image check DEFINITION`

Check an image definition without building it.

| | What it does |
|---|---|
| `DEFINITION` | Image definition, for example images/vm.toml |

## `edel theme`

theme: a look as a set of pictures and appearance values, built in or your own; the Appearance page of Settings shows the same ones.

### `edel theme list`

List the built-in themes, then yours, with the one in use marked.

### `edel theme save NAME`

Keep the theme in use as a theme of yours called NAME, with your changes on top; the screen stays the same.

| | What it does |
|---|---|
| `NAME` | A name of letters, digits and hyphens, such as my-aurora |

### `edel theme delete NAME`

Delete one of your themes; the theme in use cannot be deleted.

| | What it does |
|---|---|
| `NAME` | The theme's name, as theme list shows it |

### `edel theme reset`

Take your changes off the theme in use, so it shows as it was made.

### `edel theme export NAME [FILE]`

Write a theme, built in or your own, as one NAME.edeltheme file with its pictures, to give to someone or keep; a file is never written over.

| | What it does |
|---|---|
| `NAME` | The theme's name, as theme list shows it |
| `FILE` | The file to write; NAME.edeltheme in this folder when none is given |

### `edel theme import FILE`

Add a theme from a .edeltheme file to your themes, with its pictures; the theme in use stays as it is, so choose it with settings set.

| | What it does |
|---|---|
| `FILE` | The .edeltheme file to read; a file that does not check is refused |

## `edel preset`

preset: a layout, built in or your own, kept as the layout in use; the Layout page of Settings shows the same ones.

### `edel preset list`

List the built-in presets, then yours, with the one in use marked.

### `edel preset save NAME`

Keep the layout in use as a preset of yours called NAME, with your changes on top; the screen stays the same.

| | What it does |
|---|---|
| `NAME` | A name of letters, digits and hyphens, such as my-hive |

### `edel preset delete NAME`

Delete one of your presets; the preset in use cannot be deleted.

| | What it does |
|---|---|
| `NAME` | The preset's name, as preset list shows it |

## `edel web-app`

web-app: a website made an app, with its own window, name and icon in the launcher; it runs in Firefox with a profile of its own.

### `edel web-app add URL [OPTIONS]`

Make a website an app with its own window, name and icon in the launcher.

| | What it does |
|---|---|
| `URL` | The website, such as example.org or https://example.org |
| `--name NAME` | The app's name in the launcher; the website's host when none is given |

### `edel web-app remove NAME`

Remove one of your web apps and its own profile, by its name.

| | What it does |
|---|---|
| `NAME` | The app's name, as web-app list shows it |

### `edel web-app list`

List your web apps, with their websites.
