# Features: how images are made

**Date:** 2026-10-02 (roadmap M4.0)

Every Edel OS image is a list of features. A feature is one file, `features/NAME.toml`, and an optional directory, `features/NAME/`, copied over the root; an image definition (`images/*.toml`) names the features it is made of and holds only facts about the image. The rules are ADR-008's; this page is the maintainer's how-to. Adding, swapping or dropping a feature is one PR, and machines meet it as one A/B update that rolls back.

## The features today

| Feature | What it gives | Packages | Services | In |
|---|---|---|---|---|
| `base` | The identity (os-release, issue, motd) and what apk needs; modules `ext4`, `overlay` | `alpine-keys`, `busybox-binsh`, `ca-certificates-bundle` | none | every image |
| `container` | busybox and apk with no kernel or init | `alpine-baselayout`, `apk-tools`, `busybox`, `musl-utils` | none | container |
| `machine` | A booted machine: OpenRC, devices, logs, console and serial logins, DHCP | `alpine-base` | devfs, dmesg, mdev, hwdrivers; modules, sysctl, hostname, bootmisc, syslog, networking; mount-ro, killprocs, savecache | vm, laptop |
| `ab-boot` | Two root slots, rollback, `/data`, the system file; the watchdog and disk modules, the mkinitfs features, health `default-runlevel` | `dosfstools`, `e2fsprogs`, `e2fsprogs-extra`, `libgcc`, `partx`, `sfdisk` | edel-guard, edel-data; edel-system; edel-boot-ok | vm, laptop |
| `ssh` | Log in from another computer; switchable | `openssh-server` | sshd | vm; laptop ships it off |
| `vm` | The small kernel for virtual machines | `linux-virt` | none | vm |
| `laptop` | The long-term kernel, firmware for graphics and Wi-Fi, CPU microcode, the hardware report on the stick | `linux-lts`, 12 `linux-firmware-*`, `amd-ucode`, `intel-ucode` | edel-report | laptop |

CI's Flatpak test adds `ci/flatpak/features/flatpak-test.toml` (dbus, flatpak and its test service) to the VM's list in `ci/flatpak/vm.toml`.

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
health = []         # health files it writes, which the boot guard waits for

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
| `flatpak`, `flatpak_dropped` | Only in the `apps` feature (M6.2) |

No dependencies, versions, scripts or alternatives between features: apk resolves packages. A package one image needs alone goes into a feature named after that image (`vm`, `laptop`, `container`).

## An image definition (format 2)

```toml
format = 2
name = "edel-laptop"
variant = "vm"
arch = "x86_64"
hostname = "edel"
features = ["base", "machine", "ab-boot", "ssh", "laptop"]
off = ["ssh"]

[alpine]
branch = "v3.24"
mirror = "https://dl-cdn.alpinelinux.org/alpine"
repositories = ["main", "community"]

[vm]
kernel = "lts"      # a listed feature must install linux-lts
slot_mib = 1024
cmdline = "console=tty0 console=ttyS0,115200"   # never modules=
```

Besides these, a definition has only `[image] health_timeout` and `[release] public_keys`. Format 1 (packages and services in the definition) is refused: every definition lives in this repository and was converted in M4.0.

## What `edel image build` does with them

- **Finds** features in `features/` beside the definition, if there is one (CI's test features under `ci/`), and in the repository's, the `features/` of the nearest directory above that also holds `images/`. It checks every feature file there strictly, so an unknown field, a `NAME/` without `NAME.toml` or any other file in a features directory stops the build even when no image lists it.
- **Merges** the listed features in order: packages, services, modules, health files and mkinitfs features as sets, so several features may list `dbus`; modules keep the order they are first named in (`modules=` loads them in that order), and the mkinitfs features are sorted.
- **Refuses** an unknown feature, one listed twice, a missing `why`, one service in two runlevels, a file path shipped by two features, an `off` entry that is not listed or not switchable, a VM image without health, modules, mkinitfs features or its kernel package, and a container image with health.
- **Ships** each listed feature's file as `/usr/share/edel/features/NAME.toml`, writes the union of health as `EDEL_HEALTH` and the `off` list as `EDEL_SERVICES_OFF` in os-release, installs the packages of every listed feature, and enables the services of those not in `off`.

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
- **Traded off:** two lookup places (beside a CI definition and the repository's) instead of one, so test features stay under `ci/`; seven features where one list per image was shorter to read.
