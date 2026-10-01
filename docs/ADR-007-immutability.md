# ADR-007: How immutable the system should be

**Status:** Proposed
**Date:** 2026-10-01
**Deciders:** Alimardon
**Related:** ADR-003 (immutable base), ADR-006 (atomic updates), [AerynOS review](REVIEW-aerynos.md)

## Context

Alimardon asked whether we really need an immutable system, perhaps with some settings to control it.

"Immutable" is often misunderstood. It does **not** mean the user can't configure or change their computer. It means the **operating system's own files** are read-only while the system runs, and change only through a whole, tested update.

| Part of the system | Location | Immutable? |
|--------------------|----------|-----------|
| The OS itself (programs, libraries, defaults) | `/usr` | **Yes**, read-only |
| Your system settings | `/etc` | No, writable (only holds what you changed) |
| Your files, apps and app data | `/home`, Flatpak storage | No, writable |
| Logs, caches, databases, containers | `/var` | No, writable |

## Decision

**Yes, the base is immutable, with three escape hatches**, so power users never feel locked in.

### Why immutable

- **Reliable** (principle 1): nothing can half-change the OS. Updates and rollbacks are whole and clean.
- **Replicable** (ADR-006): every machine on the same version runs identical OS bits, so copying a setup is just the system file plus your `/etc` changes.
- **Secure:** malware and mistakes can't quietly modify the OS.
- **Long compatibility** (ADR-005): apps can't come to depend on someone's local OS hacks.
- **Supportable:** when someone reports a bug, we know exactly which bits they run.

### The three escape hatches

1. **Add-ons (system extensions).** Signed layers that add to the base without breaking immutability: extra drivers (for example a future NVIDIA add-on), virtualization tools, special hardware support. They install with one click, become part of the next deployment and roll back with it.
2. **Unlocked mode (a setting).** A switch in Settings, behind a clear warning, that lets you change OS files directly, for tinkerers and OS developers. Changes go into a separate layer, so:
   - you can see exactly what you changed,
   - one button resets the OS to clean,
   - rollback still works.

   Steam Deck uses the same idea: its OS is read-only, with an explicit command to unlock it.
3. **Dev containers.** Any toolchain or even another distro runs inside a container, so developers never need to touch the OS. This is the recommended path for development work.

### Per image

| Image | Immutable base? | Why |
|-------|-----------------|-----|
| Container | No A/B machinery needed | A container image is already immutable by nature; inside it you use plain `apk` as on Alpine today |
| Server / VM | Yes | Fleets benefit most from identical, verifiable machines |
| Desktop | Yes, with the three escape hatches | |
| Phone | Yes, no unlocked mode by default | A phone must never be left unbootable |

## Options Considered

| Option | Verdict |
|--------|---------|
| A. Immutable base, writable settings and data, three escape hatches | **Recommended** |
| B. Traditional mutable system with snapshots (for example btrfs snapshots before each update) | Rollback works, but machines drift apart over time, replication is weaker and a partial change can still slip in between snapshots |
| C. Strictly immutable with no way out | Safest, but power users and developers would leave |

## Consequences

- **Easier:** everything in principles 1 to 3; support and replication.
- **Harder:** we must build the add-on mechanism and unlocked mode; a few apps that expect to install files into the OS need an add-on or a container.
- **Revisit:** which drivers and tools should be add-ons we provide ourselves.

## Action Items

1. [ ] Define the add-on format: signed, versioned, tied to a base version, rolls back with it.
2. [ ] Design unlocked mode: separate change layer, reset button, warning text.
3. [ ] Pick the first add-ons: virtualization, and an NVIDIA driver path once NVK or a glibc-compatible option is proven.
