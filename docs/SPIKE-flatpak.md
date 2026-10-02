# Spike: Flatpak on the musl base

**Date:** 2026-10-02 (roadmap M1.9, PR #19)

## Verdict

Flathub's glibc runtimes run on our musl base, as an ordinary user, with no polkit daemon. The newest `org.freedesktop.Platform` (26.08) and the oldest branch Flathub still serves (1.6) both installed and ran a shell that printed `hello from glibc`. The risk ADR-003 and ADR-005 rest on, apps bringing their own C library, is retired for the command line; graphics, portals and windows are checked in M6.1.

## What CI does

`ci/flatpak-test.sh` boots the CI-only definition `ci/flatpak/vm.toml` (the VM image plus `flatpak`, `bubblewrap` and `dbus`) on a copy of the disk 6 GiB larger, with 2 GiB of memory. The service `edel-flatpak-test` reports the kernel facts, then runs `flatpak-steps.sh` as user `ci`:

1. `flatpak --user remote-add flathub`, the way every later app install works (M6.1), so runtimes live in `/home` on the data partition.
2. List the `org.freedesktop.Platform` branches Flathub serves.
3. Install the newest within 900 seconds and run `flatpak run --command=sh org.freedesktop.Platform//BRANCH -c 'echo hello from glibc'`. This decides the job.
4. Do the same for the oldest within 600 seconds. This only reports: an old runtime that fails is a finding for M10, not a fault in the base.

## Results

Measured in CI on 2026-10-02 (commit `234b0a6`), Alpine 3.24 with flatpak 1.16.6, bubblewrap 0.12.0 and kernel 6.18 (`linux-virt`).

| Question | Answer |
|---|---|
| Branches on Flathub | 1.6, 18.08, 19.08, 20.08, 21.08, 22.08, 23.08, 24.08, 25.08, 26.08 |
| Newest, 26.08 | ran: `hello from glibc`, 24 s for download, install and run |
| Oldest, 1.6 | ran: `hello from glibc`, 12 s for download, install and run |
| Space for both | 2.7 GB (`du -sh ~/.local/share/flatpak`) |
| Unprivileged user namespaces | yes: `unshare -U true` works as `ci`, and `max_user_namespaces` is 7830, so bubblewrap needs no setuid bit |
| Polkit | `apk info -R flatpak` lists only polkit's libraries (`so:libpolkit-agent-1.so.0`, `so:libpolkit-gobject-1.so.0`, from `polkit-noelogind-libs`); the `polkitd` daemon was not running |
| Whole test | 50 s from boot to verdict (54 s on an earlier run) |

Download sizes were not measured on their own; the times above include the downloads on a GitHub runner, and the 2.7 GB on disk is the most both took.

## What it means for the plan

- **M6.1** keeps `flatpak --user` per admin user: it needs no root and no polkit daemon, and its data sits on `/data` with the rest of `/home`, so it survives every update and rollback.
- **M6.2** (default apps in the slot) and **M3.2** (budgets) must count runtimes: these two took 2.7 GB on disk, and old apps keep old branches around.
- **M10** has a working start: the oldest runtime still runs on today's base. Graphics drivers for old runtimes (M10.2) remain the open question, since this spike drew nothing.

## Principles check

- **Reliable:** the app path is proven before the compositor depends on it, and user installs live on `/data`, outside the A/B slots.
- **Simple:** no polkit daemon and no setuid helper; one install path for everyone (`flatpak --user`).
- **Efficient:** a few CI minutes, nothing shipped. Traded off: runtimes are large, so budgets must count them, and CI downloads them on every run until a measurement shows a cache pays.
