# Edel OS

A lightweight, fast and beautiful operating system for everyone: developers, creative workers, gamers, office users, and later servers, cloud VMs and containers. One base, built on a soft fork of [Alpine Linux](https://alpinelinux.org/), runs as a container, a VM, a desktop and a phone.

**Status:** early. The architecture is written down; the first images are being built.

## How decisions are made

Every decision follows the ranked [design principles](docs/DESIGN-PRINCIPLES.md):

1. Reliable
2. Instant
3. Simple
4. Efficient
5. Beautiful
6. Functional
7. Powerful
8. Scalable
9. Versatile

When two principles conflict, the higher one wins.

## Architecture

Start at the [docs index](docs/README.md). In short:

- **Base:** an immutable, musl-based image built from Alpine stable, updated as a whole with automatic rollback ([ADR-003](docs/ADR-003-base-releases-app-compatibility.md), [ADR-006](docs/ADR-006-atomic-updates-and-replication.md), [ADR-007](docs/ADR-007-immutability.md)).
- **Apps never depend on the base:** Flatpak for desktop apps, containers for servers, so the base can update often without breaking anything ([ADR-005](docs/ADR-005-compatibility-promise.md)).
- **Our own shell:** a small Rust compositor with floating and tiling, one-button layout presets and adaptive form factors ([ADR-002](docs/ADR-002-own-desktop-shell.md), [ADR-004](docs/ADR-004-adaptive-apps.md)).

We write four parts (compositor, shell-ui, settings and the `edel` tool) and reuse everything else.

## Building images

Images are described by small TOML files in [`images/`](images/) and built by the `edel` tool, which uses Alpine's own `apk`. Building needs root on Alpine, so the easiest way is the Alpine container:

```sh
docker run --rm --privileged -v "$PWD:/src" -w /src alpine:3.24 sh ci/build.sh
```

That produces, in `out/`:

| File | What it is |
|------|------------|
| `edel-container-x86_64.tar.gz` | Container image; load it with `docker import` |
| `edel-vm-x86_64.img` | Bootable VM disk: UEFI, GRUB, two root slots, A and B, and a data partition that grows to fill the disk |
| `edel-vm-x86_64.ext4` | One root slot; installing it into a running VM is an update |

To see every step without changing anything:

```sh
cargo run -- image build images/vm.toml --plan
```

To boot the VM disk (the firmware path differs between distributions):

```sh
qemu-system-x86_64 -machine q35,accel=kvm -m 512 -nographic \
  -bios /usr/share/ovmf/OVMF.fd -snapshot \
  -drive file=out/edel-vm-x86_64.img,format=raw,if=virtio
```

CI builds both images on every change, runs the container, boots the VM in QEMU until the Edel OS login prompt appears, tests updates and rollback (below), and reports image sizes.

## Updates and rollback

The VM disk has two root slots, A and B, each a complete system with its own kernel ([ADR-006](docs/ADR-006-atomic-updates-and-replication.md)). An update is written to the slot that is not running, so the running system is never changed. The system itself is read-only; `/home`, `/var` and every change to `/etc` live on the data partition, so updates and rollbacks keep people's files and settings:

```sh
edel update https://.../release.toml   # check its signature, write the other slot
reboot                                 # the new version starts
edel rollback                          # if you want the previous version back
reboot
```

Each slot carries its own boot loader, installed only once the slot is confirmed. GRUB gives a newly installed slot three tries. Once the system has started, the boot guard confirms the slot; if it hangs instead, a hardware watchdog restarts the machine and the try counts. If the slot fails to start three times, GRUB starts the previous slot again on its own, and that slot switches the failed one off. `edel status` shows both slots, and `edel rollback` starts the other slot again at the next boot.

CI tests the whole cycle in one VM: install an update and start it, then install a deliberately broken update and check that the machine comes back on the previous slot, with a rollback by command in between.

## License

Edel OS is free software: you can share and change it under the [GNU General Public License](LICENSE), version 3 or (at your option) any later version.
