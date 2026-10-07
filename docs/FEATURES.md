# Features: how images are made

**Date:** 2026-10-02 (roadmap M4.0), amended 2026-10-03 (M4.1: the desktop's features; M4.2b: `programs` and the compositor)

Every Edel OS image is a list of features. A feature is one file, `features/NAME.toml`, and an optional directory, `features/NAME/`, copied over the root; an image definition (`images/*.toml`) names the features it is made of and holds only facts about the image. The rules are ADR-008's; this page is the maintainer's how-to. Adding, swapping or dropping a feature is one PR, and machines meet it as one A/B update that rolls back.

## The features today

| Feature | What it gives | Packages | Services | In |
|---|---|---|---|---|
| `base` | The identity (os-release, issue, motd) and what apk needs; modules `ext4`, `overlay` | `alpine-keys`, `busybox-binsh`, `ca-certificates-bundle` | none | every image |
| `container` | busybox and apk with no kernel or init | `alpine-baselayout`, `apk-tools`, `busybox`, `musl-utils` | none | container |
| `machine` | A booted machine: OpenRC, logs, console and serial logins, DHCP, and the clock set from the network once at each start (`edel-clock`, busybox `ntpd`, M1.13) | `alpine-base` | devfs, dmesg; modules, sysctl, hostname, bootmisc, syslog, networking; edel-clock; mount-ro, killprocs, savecache | vm, desktop |
| `mdev` | Device events with busybox mdev, which loads the drivers for the hardware found | none | mdev, hwdrivers | vm |
| `udev` | Device events with udev, whose database libinput and the compositor read | `eudev`, `udev-init-scripts`, `udev-init-scripts-openrc` | udev, udev-trigger, udev-settle | desktop |
| `ab-boot` | Two root slots, rollback, `/data`, the settings file; the watchdog and disk modules, the mkinitfs features, health `default-runlevel` | `dosfstools`, `e2fsprogs`, `e2fsprogs-extra`, `libgcc`, `partx`, `sfdisk` | edel-guard, edel-data; edel-settings; edel-boot-ok | vm, desktop |
| `ssh` | Log in from another computer, with an ed25519 host key; switchable | `openssh-server` | sshd | vm; the desktop ships it off |
| `vm` | The small kernel for virtual machines | `linux-virt` | none | vm |
| `laptop` | The long-term kernel, firmware for graphics and Wi-Fi, CPU microcode, eMMC in the initramfs, the hardware report on the stick | `linux-lts`, 12 `linux-firmware-*`, `amd-ucode`, `intel-ucode` | edel-report | desktop |
| `graphics` | Mesa for every common GPU (llvmpipe where none loads; svga for VirtualBox's VMSVGA), DRM, libinput, keyboard layouts; modules `virtio_gpu` and `vmwgfx` | `mesa-dri-gallium`, `mesa-egl`, `mesa-gbm`, `mesa-vulkan-intel`, `mesa-vulkan-ati`, `mesa-va-gallium`, `libdrm`, `libinput`, `xkeyboard-config` | none | desktop |
| `seat` | The screen and input for the person at the machine; `/run/edel/session` for the files the session leaves for root | `seatd`, `seatd-openrc` | edel-rundir; seatd | desktop |
| `login` | greetd, whose greeter is our compositor running its text greeter in foot (M4.7b) and which starts the person's session, `/usr/libexec/edel-session`, after login; `/run/user/UID` from pam_rundir; on a USB stick, or with nobody set up yet, `edel-live` logs the person called `live` in by itself (M3.6); needs `terminal` and `compositor` | `greetd`, `greetd-openrc`, `greetd-agreety`, `pam-rundir` | edel-live; greetd | desktop |
| `fonts` | Inter for the interface; Noto Sans, Serif and Sans Mono, the terminal's font | `font-inter`, `font-noto` | none | desktop |
| `completion` | Tab completion: bash, bash-completion and new people's login shell bash, which completes `edel`'s commands, keys and values (M5.26) | `bash`, `bash-completion` | none | desktop |
| `terminal` | foot | `foot` | none | desktop |
| `compositor` | Our compositor, program `edel-compositor` (M4.2b), with the libraries it links and dbus for each session's bus; health `compositor` (M4.8): the slot is good once a compositor, the greeter's included, shows a frame or waits for a screen | `dbus`, `eudev-libs`, `libgcc`, `libinput-libs`, `libseat`, `libxkbcommon`, `mesa-gbm` | none | desktop |
| `xwayland` | X11 apps: XWayland through xwayland-satellite, which the compositor starts when the first X11 app connects (M4.7) | `xwayland`, `xwayland-satellite` | none | desktop |
| `settings` | Settings, program `edel-settings` (M5.6a), with GTK4, libadwaita and its icons, and its desktop file, so the launcher lists it and the presets pin it; nothing runs until a person opens it | `gtk4.0`, `libadwaita`, `adwaita-icon-theme` | none | desktop |

CI's Flatpak test adds `ci/flatpak/features/flatpak-test.toml` (dbus, flatpak and its test service) to the VM's list in `ci/flatpak/vm.toml`, and the desktop test adds `ci/desktop/features/desktop-test.toml` (weston-clients, wayland-utils for `wayland-info`, xclock for an X11 window, the program `edel-testclient`, its test service, an autologin of user ci in to the compositor) to the desktop's in `ci/desktop/vm.toml`.

## A feature file

```toml
# features/ssh.toml; features/ssh/, if present, is copied over the root
format = 1
summary = "Log in from another computer with ssh"   # one line for people
why = "Servers and virtual machines are run from afar, and ssh is how; ..."
packages = ["openssh-server"]
modules = []        # kernel modules the initramfs loads first (modules=)
initramfs = []      # mkinitfs features
switchable = true   # may ship off (off = [...]); services.ssh later (M6.10)
addon = false       # also built as a signed add-on (M7.2a)
health = []         # health checks it writes, which the boot guard waits for
programs = []       # our own programs it ships in /usr/bin (edel-compositor)

[services]
default = ["sshd"]  # also sysinit, boot, shutdown
```

| Field | Rule |
|---|---|
| `format` | 1 |
| `summary` | Required, one line |
| `why` | Required when the feature adds a package or a service: the written reason of principle 3, now a build check |
| `packages`, `services` | Alpine package names; OpenRC services by runlevel, each in one runlevel. A service's `/etc/init.d/NAME` comes from a package or a feature's directory |
| `modules`, `initramfs` | Only a-z, 0-9, `_` and `-`. Images without a kernel ignore them |
| `programs` | Programs of this repository's workspace the feature ships in `/usr/bin`, such as `edel-compositor`; `edel image build` copies each from beside itself, where `cargo build --release --workspace` leaves it, so list the libraries it links in `packages`, which apk cannot see. A program needs a `why` |
| `flatpak`, `flatpak_dropped` | Only in the `apps` feature (M6.2) |
| `[alpine]` | `branch`, `mirror` and `repositories`: only in the `base` feature, the one owner of the Alpine branch every image is built from and CI's build container follows (M5.27) |

No dependencies, versions, scripts or alternatives between features: apk resolves packages. A package one image needs alone goes into a feature named after that image (`vm`, `container`).

## An image definition (format 2)

```toml
format = 2
name = "edel-desktop"
variant = "vm"
arch = "x86_64"
hostname = "edel"
features = ["base", "machine", "udev", "ab-boot", "ssh", "laptop", "completion", "graphics", "seat", "login", "fonts", "terminal", "compositor", "shell", "settings", "xwayland"]
off = ["ssh"]       # installed, its service off

[vm]
kernel = "lts"      # a listed feature must install linux-lts
slot_mib = 1536
cmdline = "console=tty0 console=ttyS0,115200"   # never modules=
```

Besides these, a definition has only `[image] health_timeout` and `[release] public_keys`. Format 1 (packages and services in the definition) is refused: every definition lives in this repository and was converted in M4.0.

## What `edel image build` does with them

- **Finds** features in `features/` beside the definition, if there is one (CI's test features under `ci/`), and in the repository's, the `features/` of the nearest directory above that also holds `images/`. It checks every feature file there strictly, so an unknown field, a `NAME/` without `NAME.toml` or any other file in a features directory stops the build even when no image lists it.
- **Merges** the listed features in order: packages, services, modules, health files, programs and mkinitfs features as sets, so several features may list `dbus`; modules keep the order they are first named in (`modules=` loads them in that order), and the mkinitfs features are sorted.
- **Refuses** a health name that nothing writes: `default-runlevel` and `compositor` are edel's own, and any other name (a-z, 0-9 and '-', such as a server's `network`) is a feature's own check, its file `$health_dir/NAME` (`/run/edel/health/NAME`, from `places.sh`), which a file one of the image's features ships must write, as its service does once the check passes (M1.11). A check finishes well inside `health_timeout`, or every update falls back.
- **Refuses** an unknown feature, one listed twice, a missing `why`, one service in two runlevels, a file path shipped by two features, an `off` entry that is not listed or not switchable, a VM image without health, modules, mkinitfs features or its kernel package, and a container image with health.
- **Ships** each listed feature's file as `/usr/share/edel/features/NAME.toml`, writes the union of health as `EDEL_HEALTH` and the `off` list as `EDEL_SERVICES_OFF` in os-release, installs the packages of every listed feature, copies their programs to `/usr/bin`, and enables the services of those not in `off`.

On a machine, feature files are read leniently: `edel report` lists the features in `/usr/share/edel/features/` and notes any field it skipped (`edel::features::read`), so an older release can read a newer feature file. `edel image check` and the build use the strict `edel::features::check`.

## Worked examples

Each is one PR.

| Change | Files the PR touches | What machines see |
|---|---|---|
| Add a feature, say printing | `features/printing.toml` with `why`, `switchable = true` and later `addon = true`; `features/printing/` if it ships files; one line in each image's `features` | The packages and services arrive with the next update; with `addon = true`, only for those who pick it (M7.2a) |
| Replace a daemon (NetworkManager for busybox networking) | The feature's `packages` and `[services]` and its directory | Settings keys keep their names; the daemon's own state is converted in the PR or named in the release notes |
| Ship a feature off | The image's `off = ["NAME"]`; the feature must be `switchable` | Its packages are there, its services do not start; `EDEL_SERVICES_OFF` names it |
| Move a package between features | Both feature files | Nothing, if every image lists both |
| Drop a feature | Delete `features/NAME.toml` and `features/NAME/`, and its line in each definition; a leftover directory fails the build | Its packages and services are gone after the update; a rollback brings them back |
| A test-only feature | `ci/<test>/features/NAME.toml` beside a CI-only definition | Nothing: release images never list it |

A cargo test (`def::tests`) fails when a change makes an image install or enable something its features do not explain: it rebuilds main's format-1 definitions from `crates/edel/tests/format1/` and compares, and dropping `ssh` from the VM must drop exactly `sshd` and `openssh-server`.

## Principles check

- **Reliable:** strict checks at build time refuse a broken or ambiguous feature before any image exists; lenient reading on a machine never stops an older release; the merge was proven against the previous definitions.
- **Simple:** one file per feature, one way to put a package or service into an image, no dependencies or scripts between features, and the old `files` lists and the `boot::MODULES` and `INITRAMFS_FEATURES` constants are gone.
- **Efficient:** nothing runs at boot for this; each image carries a few small TOML files.
- **Scalable:** the desktop, phone and later images are lists of the same features.
- **Traded off:** two lookup places (beside a CI definition and the repository's) instead of one, so test features stay under `ci/`; a feature per piece (15 since M4.1) where one list per image was shorter to read.
