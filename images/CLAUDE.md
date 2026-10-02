# Image definitions

An image definition is a TOML file here; `edel image build` turns it into an image. The schema and its checks are in `crates/edel/src/def.rs`, the build steps in `image.rs`. Two exist, both format 1, Alpine `v3.24` (`main` and `community`), `x86_64`:

- `container.toml`: `edel-container`, no hostname, no services, no `[vm]`; packed as `out/edel-container-x86_64.tar.gz` (load it with `docker import`).
- `vm.toml`: `edel-vm`, hostname `edel`, kernel `virt`, `slot_mib = 1024`, serial console; packed as `out/edel-vm-x86_64.img` (GPT disk: EFI system partition, slot A filled, slot B empty, and a 64 MiB `edel-data` partition that grows to the end of the disk on first boot) and `out/edel-vm-x86_64.ext4` (one slot cut to its file system with `resize2fs -M`, the update image), each also as `.gz`.

Check a definition with `cargo run --quiet --locked -- image check images/vm.toml` (it prints `<name>-<arch>: ok (<n> packages)`); list its build steps with `cargo run -- image build images/vm.toml --dry-run`.

## Schema (strict: an unknown key fails)

- Top level: `format = 1`, `name`, `variant` (`"container"` or `"vm"`), `arch`, optional `hostname`, optional `files` (directories relative to the definition, copied in order). Outputs are named `<name>-<arch>`.
- `[alpine]`: `branch`, `mirror`, `repositories` (not empty). `[packages]`: `install` (not empty). `[services]`: optional `sysinit`, `boot`, `default`, `shutdown` lists of OpenRC services.
- `[vm]`, required for `vm` and forbidden for `container`: `kernel` (the Alpine kernel flavour; `linux-<kernel>` must be in `install`), `slot_mib` (at least 64), optional `cmdline`, optional `data_mib` (default 64, at least 16).
- `[image]`, required for `vm`: `health` (names the boot guard waits for: `default-runlevel`, later `compositor`) and optional `health_timeout` (seconds, default 120, at least 10), written into os-release as `EDEL_HEALTH` and `EDEL_HEALTH_TIMEOUT`; `edel image build --health-timeout` overrides it for test images. Every slot carries its boot loader (GRUB binary, `grub.cfg`, `loader.toml`) in `/usr/lib/edel/boot/`; `--loader-tag` makes a test loader differ. Every image's os-release gets the build's `VERSION_ID` (`edel image build --version`, numbers joined by dots; without it the overlay's `0.1` stays), `EDEL_CHANNEL` (`--channel`, default `dev`), `EDEL_ARCH` and `EDEL_PLATFORM_LEVEL` (0 until M8.8), and the installed packages land in `/usr/share/edel/packages` and `out/<image>.packages` (M3.1). VM images also get `EDEL_LOADER_VERSION` and `EDEL_IMAGE="<name>-<arch>"`, the name `release.toml` lists their image under, and `EDEL_HOSTNAME`, the hostname `edel system apply` goes back to when the system file names none.
- `[release]`, optional: `public_keys`, key files (relative to the definition) copied to `/usr/share/edel/keys/` in VM images; `edel image build --public-key FILE` adds more for test images. No definition names a key until the first preview (M3.4).
- `name` and `hostname` use only a-z, 0-9 and `-` and do not start with `-`. `cmdline` is pasted into `grub.cfg`, so it may not contain `"`, `'`, `\`, `$`, `;`, `{`, `}`, `#` or a backtick.
- The code accepts `aarch64` for a VM, but no aarch64 definition exists until M9 and CI builds only `x86_64`.

## Rules

- A new package or service needs a written reason (principle 3) in the PR. Keep `e2fsprogs-extra` in `vm.toml`: it gives `edel update` its `tune2fs`, and the A/B test failed without it. Keep `libgcc` too: the `edel` that Alpine's cargo builds links against `libgcc_s.so.1`.
- A service in `[services]` is linked into its runlevel and needs `/etc/init.d/NAME` from a package or an overlay, or the build fails. Release images enable services through `[services]`, never through committed runlevel links.
- `files/` holds the overlays, copied over the image with `cp -R` in the order a definition lists them, after packages and before services. Copies are owned by root and keep their git mode, so commit scripts and init scripts as executable (100755; `git ls-files -s` shows it).
  - `common/` (both images): the identity. `usr/lib/os-release` is required (`NAME="Edel OS"`, `PRETTY_NAME="Edel OS 0.1 (development)"`); the build makes `/etc/os-release` a link to it. Also `etc/issue` and `etc/motd`.
  - `vm/`: `etc/fstab` (root is whichever slot GRUB started, mounted read-only), `etc/inittab` (adds a getty on the serial console `ttyS0`), `etc/network/interfaces` (lo, and eth0 by DHCP).
  - `ab/` (VM only): the `edel-data` sysinit service (`edel boot mount-data`: grow partition 4 on first boot, mount it at `/data`, add new system users from the slot to the machine's account files, overlay `/etc` with its upper layer on `/data`, top up `/data/var` from the slot, bind `/data/home`, `/data/var` and `/data/root` over `/home`, `/var` and `/root` (root's home starts as a copy of the slot's, mode 0700), and mount `/tmp` as tmpfs) the `edel-guard` sysinit service (`edel boot guard` in the background: pets the watchdog until the health files exist, then confirms the slot and exits), the `edel-system` boot service (`edel system apply --boot`, before `hostname`: seeds `/data/edel/system.toml` on first boot and applies its hostname, users, ssh keys and developer mode), and the `edel-boot-ok` OpenRC service, which touches `/run/edel/default-reached` last in the default runlevel and prints the guard's confirmation, the version line and `Started in N s, M MiB of memory in use, R MiB used on the root` (the budgets read it, M3.2). If the health files never appear, the watchdog restarts the machine and GRUB gives the slot three tries and then starts the other slot if that one is OK; if no slot is OK with tries left, it starts the first slot in ORDER anyway. `edel image build` copies itself to `/usr/bin/edel` in every VM image (the path M6.5's doas rule names).
- ADR-006 aims for OS defaults under `/usr` and an `/etc` that holds only what an administrator changed. Today only `os-release` follows it; `etc/issue`, `motd`, `fstab`, `inittab`, `network/interfaces` and the init scripts still live in `/etc`. They move only in a roadmap step (M1.4 notes: "later, our own defaults move to `/usr`"), never as a side change.
- The build locks root's password (`*`); access comes later from keys or the system file. Never ship a user or a default password (ADR-008).
- CI greps text from here: `Welcome to Edel OS` (`etc/issue`), a `PRETTY_NAME` starting `Edel OS `, the hostname `edel` (`edel login:`) and the `edel update` output. Change them only together with the `ci/` scripts.
- Test-only files and CI-only definitions never go here: they live under `ci/` and reach a test image through `--files` or a definition under `ci/`.
- Shell code here is busybox POSIX sh: `#!/bin/sh` (or `#!/sbin/openrc-run`), `set -eu` in scripts, tabs.

## Coming changes (roadmap)

Make each change only in its step, and take the details from the step itself, not from here. Steps that touch this directory:

- M3.3 moves `vm.toml` to `linux-lts`, adds `images/laptop.toml` and sets `slot_mib = 4096` for every bootable image. M4.0 moves definitions to format 2, which list features and image facts only (ADR-008).
- Definitions stay strict in every format.
