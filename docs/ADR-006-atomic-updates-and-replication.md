# ADR-006: Atomic updates and system replication

**Status:** Proposed
**Date:** 2026-10-01
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

### 2. One readable file describes a whole machine

A plain data file, not a programming language:

```toml
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

[extensions]             # system-level add-ons
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

1. [ ] Spike RAUC on an Alpine VM image with A/B slots and automatic fallback.
2. [ ] Define version 1 of the system file format.
3. [ ] Implement export, diff and apply against the VM image.
4. [ ] Teach the installer to accept a system file.

## Sources

- [RAUC](https://rauc.io/)
- [Alpine Linux experiments with systemd compatibility (Linux Journal, May 2026)](https://www.linuxjournal.com/content/alpine-linux-experiments-systemd-compatibility-while-keeping-its-lightweight-identity)
