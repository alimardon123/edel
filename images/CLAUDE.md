# Image definitions

An image definition is a TOML file here; `edel image build` turns it into an image. The schema and its checks are in `crates/edel/src/def.rs`, the build steps in `image.rs`. Two exist, both format 1, Alpine `v3.24` (`main` and `community`), `x86_64`:

- `container.toml`: `edel-container`, no hostname, no services, no `[vm]`; packed as `out/edel-container-x86_64.tar.gz` (load it with `docker import`).
- `vm.toml`: `edel-vm`, hostname `edel`, kernel `virt`, `slot_mib = 1024`, serial console; packed as `out/edel-vm-x86_64.img` (GPT disk: EFI system partition, slot A filled, slot B empty, and a 64 MiB `edel-data` partition that grows to the end of the disk on first boot) and `out/edel-vm-x86_64.ext4` (one slot, which is also the update image).

Check a definition with `cargo run --quiet --locked -- image check images/vm.toml` (it prints `<name>-<arch>: ok (<n> packages)`); list its build steps with `cargo run -- image build images/vm.toml --dry-run`.

## Schema (strict: an unknown key fails)

- Top level: `format = 1`, `name`, `variant` (`"container"` or `"vm"`), `arch`, optional `hostname`, optional `files` (directories relative to the definition, copied in order). Outputs are named `<name>-<arch>`.
- `[alpine]`: `branch`, `mirror`, `repositories` (not empty). `[packages]`: `install` (not empty). `[services]`: optional `sysinit`, `boot`, `default`, `shutdown` lists of OpenRC services.
- `[vm]`, required for `vm` and forbidden for `container`: `kernel` (the Alpine kernel flavour; `linux-<kernel>` must be in `install`), `slot_mib` (at least 64), optional `cmdline`, optional `data_mib` (default 64, at least 16).
- `name` and `hostname` use only a-z, 0-9 and `-` and do not start with `-`. `cmdline` is pasted into `grub.cfg`, so it may not contain `"`, `'`, `\`, `$`, `;`, `{`, `}`, `#` or a backtick.
- The code accepts `aarch64` for a VM, but no aarch64 definition exists until M9 and CI builds only `x86_64`.

## Rules

- A new package or service needs a written reason (principle 3) in the PR. Keep `e2fsprogs-extra` in `vm.toml`: it gives `edel update` its `tune2fs`, and the A/B test failed without it. Keep `libgcc` too: the `edel` that Alpine's cargo builds links against `libgcc_s.so.1`.
- A service in `[services]` is linked into its runlevel and needs `/etc/init.d/NAME` from a package or an overlay, or the build fails. Release images enable services through `[services]`, never through committed runlevel links.
- `files/` holds the overlays, copied over the image with `cp -R` in the order a definition lists them, after packages and before services. Copies are owned by root and keep their git mode, so commit scripts and init scripts as executable (100755; `git ls-files -s` shows it).
  - `common/` (both images): the identity. `usr/lib/os-release` is required (`NAME="Edel OS"`, `PRETTY_NAME="Edel OS 0.1 (development)"`); the build makes `/etc/os-release` a link to it. Also `etc/issue` and `etc/motd`.
  - `vm/`: `etc/fstab` (root is whichever slot GRUB started, mounted read-only), `etc/inittab` (adds a getty on the serial console `ttyS0`), `etc/network/interfaces` (lo, and eth0 by DHCP).
  - `ab/` (VM only): the `edel-data` sysinit service (`edel boot mount-data`: grow partition 4 on first boot, mount it at `/data`, add new system users from the slot to the machine's account files, overlay `/etc` with its upper layer on `/data`, top up `/data/var` from the slot, bind `/data/home` and `/data/var` over `/home` and `/var`, and mount `/tmp` as tmpfs) and the `edel-boot-ok` OpenRC service, which runs `edel update mark-good` last in the default runlevel to confirm the slot. If it never runs, GRUB gives the slot three tries and then starts the other slot if that one is OK; if no slot is OK with tries left, it starts the first slot in ORDER anyway. `edel image build` copies itself to `/usr/bin/edel` in every VM image (the path M6.5's doas rule names).
- ADR-006 aims for OS defaults under `/usr` and an `/etc` that holds only what an administrator changed. Today only `os-release` follows it; `etc/issue`, `motd`, `fstab`, `inittab`, `network/interfaces` and the init scripts still live in `/etc`. They move only in a roadmap step (M1.4 notes: "later, our own defaults move to `/usr`"), never as a side change.
- The build locks root's password (`*`); access comes later from keys or the system file. Never ship a user or a default password (ADR-008).
- CI greps text from here: `Welcome to Edel OS` (`etc/issue`), a `PRETTY_NAME` starting `Edel OS `, the hostname `edel` (`edel login:`) and the `edel update` output. Change them only together with the `ci/` scripts.
- Test-only files and CI-only definitions never go here: they live under `ci/` and reach a test image through `--files` or a definition under `ci/`.
- Shell code here is busybox POSIX sh: `#!/bin/sh` (or `#!/sbin/openrc-run`), `set -eu` in scripts, tabs.

## Coming changes (roadmap)

Make each change only in its step, and take the details from the step itself, not from here. Steps that touch this directory:

- M1.5 adds `[image] health` and `health_timeout` and adds `i6300esb` and `softdog` to `modules=`; M1.6 adds `[release] public_keys`; M2.4 adds `sfdisk`, `dosfstools` and `mtools` to `vm.toml`.
- M3.3 moves `vm.toml` to `linux-lts`, adds `images/laptop.toml` and sets `slot_mib = 4096` for every bootable image. M4.0 moves definitions to format 2, which list features and image facts only (ADR-008).
- Definitions stay strict in every format.
