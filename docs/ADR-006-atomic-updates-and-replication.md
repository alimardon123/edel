# ADR-006: Atomic updates and system replication

**Status:** Proposed
**Date:** 2026-10-01, amended 2026-10-06 (the file and its commands take the Settings app's names, ADR-008)
**Deciders:** Alimardon
**Extends:** ADR-003 (immutable base with A/B updates)

## Context

Two wishes:

1. **Updates that never break the system.** If an update fails, the machine goes back to the version that worked, on its own. Atomic distros (Fedora Silverblue, postmarketOS Duranium and others) already do this.
2. **Replication.** A person moves their exact setup from one laptop to another; a system administrator rolls the same setup out to many machines.

Both must stay simple and pleasant to use, not a puzzle for experts.

## Decision

### 1. Every change is a new bootable version

Base updates, system add-ons (for example extra drivers or virtualization tools) and applied configuration changes all produce a **new deployment**. The running system is never modified in place.

- The new deployment is written to the inactive slot (A/B, as in ADR-003).
- On the next boot it gets a few tries. If it fails to boot, or the desktop fails to start, the bootloader falls back to the previous deployment automatically and tells the user what happened.
- The last few deployments stay in the boot menu, so the user can also roll back by hand.

**A/B over more complex schemes:** two slots are easy to reason about and use twice the base size, which is cheap because the base is small. Simple wins.

**Tooling:** Alpine uses OpenRC, not systemd. Alpine is exploring an *optional* systemd compatibility layer (reported May 2026) but is not switching. Candidates for the updater:

- **RAUC**, an established A/B updater from the embedded Linux world with signed update bundles and HTTP streaming. Needs a spike to confirm it fits Alpine and OpenRC.
- **ostree**, used by Fedora's atomic editions.
- A **small updater of our own**, if both turn out heavier than the problem.

**Spike result (2026-10-01).** RAUC is not packaged for Alpine stable: it is only in edge's `testing` repository (version 1.10.1, its test suite allowed to fail, no OpenRC service), and it brings glib, D-Bus and json-glib into the base. The A/B boot side was built and tested without it:

- The VM disk is GPT with an EFI system partition (GRUB) and two root slots. Each slot holds a whole system, kernel included.
- GRUB gives each slot three tries and counts them in its environment block, using RAUC's own variable names (`ORDER`, `<slot>_OK`, `<slot>_TRY`), so RAUC could still take over later.
- An OpenRC service confirms the slot once the system is up. A slot that fails three times is passed over, and the slot that does start switches it off.
- A small script, `edel-update`, writes the other slot, checks it against the image and gives it a fresh filesystem UUID (the kernel finds its root by UUID).
- CI installs an update and starts it, then installs a broken one and checks that the machine falls back on its own.

**Updater decision (2026-10-01):** we keep our own updater. `edel-update` becomes `edel update` in the edel tool, and RAUC is not packaged. Packaging RAUC would bring glib, D-Bus and json-glib into the base and leave us maintaining a package Alpine itself only carries in testing, while our updater only has to write a slot, check it and flip a few boot variables. Because GRUB uses RAUC's variable names, RAUC remains a fallback if our updater ever outgrows itself. Known gaps:

- **Hangs:** a slot that hangs instead of crashing never restarts, so it never falls back. A watchdog fixes this.
- **Signing:** updates are checked against the image's checksum but not yet signed.
- **Boot writes:** GRUB writes its try counter on every start, as RAUC's scheme does. Counting only after an update would write less often.
- **Bootloader updates:** GRUB itself is not updated through the slots yet.

### 2. One readable file describes a whole machine

A plain data file, not a programming language:

```toml
format = 1

[system]
channel  = "stable"      # later: "lts"
version  = "2027.1"      # optional pin; omit to follow the channel
variant  = "desktop"     # container | server | desktop | phone

[shell]
preset = "classic"       # classic | mac-like | windows-like | tiling | ...
tiling = false

[apps]
flatpak = [
  "org.mozilla.firefox",
  "org.libreoffice.LibreOffice",
  "com.valvesoftware.Steam",
]

[addons]                 # signed add-ons (ADR-007)
add = ["virtualization"]

[users.ali]
admin = true

[network]
hostname = "ali-laptop"
```

Three commands:

| Command | What it does |
|---------|--------------|
| `edel system export` | Writes the current machine's setup to a file |
| `edel system diff file` | Shows what applying the file would change |
| `edel system apply file` | Makes this machine match the file, as a new deployment that can roll back |

From M5.25 the file is `settings.toml` and the commands are `edel settings export`, `edel settings diff FILE` and `edel settings import FILE`, the Settings app's own names (ADR-008).

The same file works at install time: the installer accepts it from a USB stick or a URL, so a new machine comes up already set up. The settings app can do export and apply with a button, so nobody has to touch a terminal.

**For fleets:** a shared base file plus a tiny per-device file that overrides a few values (hostname, user). For larger fleets, administrators can also **build their own image** from the same file in CI and roll that image out, so every machine runs identical bits.

### 3. What the file does not hold

- **Personal files** (documents, photos): that is backup and sync, a separate feature.
- **Secrets** (Wi-Fi passwords, keys): the file refers to them; it never contains them, so it is safe to share.

### 4. Reproducible builds (later goal)

Anyone who builds an image from the same source gets the exact same bits, so users and companies can verify that a download wasn't tampered with. Worth designing for from the start, even if it is checked later.

## Options Considered

| Option | Verdict |
|--------|---------|
| A. Data-only system file plus A/B deployments | **Recommended.** Simple to read, simple to share. |
| B. NixOS-style configuration in a programming language | Very powerful, but the learning curve is steep. Fails "easy to use". |
| C. OS defined as a container image (Fedora bootc style) | Great for administrators; kept as the fleet option in section 2. |
| D. Configuration scripts (Ansible style) | Machines drift over time. Rejected as the main path. |
| E. Content store plus live atomic `/usr` swap (AerynOS moss style) | Elegant and disk-efficient, but more to understand, harder to verify, and needs a fully merged `/usr`. Kept as a future option; see the [AerynOS review](REVIEW-aerynos.md). |

## Additions from the AerynOS review

- **Stateless configuration:** the OS ships its defaults in `/usr`; `/etc` holds only what the administrator changed. Rollback stays clean, and `edel system export` only needs the system file plus that small difference.
- **Versioned formats:** the deployment layout, the system file and presets carry a format version from version 1, and the `edel` tool migrates old formats on update, so no machine is ever stranded by a format change.
- **Fresh installs are tested as seriously as updates** (see principle 1).

## Consequences

- **Easier:** fearless updates; moving to a new laptop is one file; fleets stay identical.
- **Harder:** every setting the file can express must be applied the same way every time, so the file format needs versioning and tests.
- **Revisit:** whether to include more of the user's desktop settings in the file over time.

## Action Items

1. [x] Spike A/B slots with automatic fallback on an Alpine VM image (see the spike result above; RAUC itself is not packaged for Alpine stable).
2. [x] Choose the updater: our own `edel update` (decided 2026-10-01; RAUC is not packaged).
3. [x] Sign updates, and add a watchdog so a hanging slot also falls back (M1.5 and M1.6, PRs #15 and #16).
4. [ ] Define version 1 of the system file format.
5. [ ] Implement export, diff and apply against the VM image.
6. [ ] Teach the installer to accept a system file.

## Sources

- [RAUC](https://rauc.io/)
- [Alpine Linux experiments with systemd compatibility (Linux Journal, May 2026)](https://www.linuxjournal.com/content/alpine-linux-experiments-systemd-compatibility-while-keeping-its-lightweight-identity)
