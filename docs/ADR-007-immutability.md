# ADR-007: How immutable the system should be

**Status:** Proposed
**Date:** 2026-10-01, amended 2026-10-02 (customization levels; unlocked mode renamed developer mode)
**Deciders:** Alimardon
**Related:** ADR-003 (immutable base), ADR-006 (atomic updates), [AerynOS review](REVIEW-aerynos.md), [roadmap](ROADMAP.md)

## Context

Alimardon asked whether we really need an immutable system, perhaps with some settings to control it. On 2026-10-02 they added that users must be able to customize the system the way they want, in how it looks and in how it works, and that a change deep enough to alter the operating system itself should only be allowed in a mode made for that, named developer mode.

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

### What a user may change, and where it goes

Customization comes in three levels. The rule that sorts a change into a level: **if it can be written as a value in the system file, it is a setting and anyone may make it; if it changes the OS's own bits, it is developer mode.**

| Level | Who | What | Where it lives | Survives updates | Copied to another machine by the system file |
|-------|-----|------|----------------|------------------|----------------------------------------------|
| 1. Settings | Everyone, from the Settings app or `edel system set` | Look: wallpaper, light or dark, accent colour, fonts, cursor and icon sizes, motion. Layout: presets, tiling, title bars, outputs, keyboard shortcuts. Behaviour: default apps, startup apps, power actions (lid, idle, power button, on battery), login, language and keyboard, update policy, optional services such as ssh, printing or Bluetooth on or off | `system.toml`, per machine and per user, plus the `/etc` overlay | Yes | Yes |
| 2. Add-ons and profiles | Everyone, one click | Drivers, tools and services built and signed by us or the community (virtualization, containers, printing, gaming) | Signed images on the data partition, named in the system file | Yes, they roll back with the slot | Yes, by name |
| 3. Developer mode | Tinkerers and OS developers, behind a warning | Anything: OS files, packages installed with `apk`, custom services | A separate layer over `/usr` on the data partition | Yes, until reset; it may break an update, and reset is the fix | No: the export lists how much was changed, it does not carry it |

Adding a setting is always cheaper than adding a part, and a setting that fifteen people want differently becomes a preset (DESIGN-PRINCIPLES, "Simple over Versatile"). Nothing in level 1 or 2 ever changes the OS's bits, so every machine on a version still runs identical code.

### The three escape hatches

1. **Add-ons (system extensions).** Signed layers that add to the base without breaking immutability: extra drivers (for example a future NVIDIA add-on), virtualization tools, special hardware support. They install with one click, become part of the next deployment and roll back with it.
2. **Developer mode (a setting).** A switch in Settings, behind a clear warning, that lets you change OS files directly, install packages with `apk` and add your own services, for tinkerers and OS developers. Changes go into a separate layer, so:
   - you can see exactly what you changed,
   - one button resets the OS to clean,
   - rollback still works.

   Steam Deck uses the same idea: its OS is read-only, with an explicit command to unlock it. We call it developer mode, because that is who it is for.
3. **Dev containers.** Any toolchain or even another distro runs inside a container, so developers never need to touch the OS. This is the recommended path for development work.

### Per image

| Image | Immutable base? | Why |
|-------|-----------------|-----|
| Container | No A/B machinery needed | A container image is already immutable by nature; inside it you use plain `apk` as on Alpine today |
| Server / VM | Yes | Fleets benefit most from identical, verifiable machines |
| Desktop | Yes, with the three escape hatches | |
| Phone | Yes, no developer mode by default | A phone must never be left unbootable |

## Options Considered

| Option | Verdict |
|--------|---------|
| A. Immutable base, writable settings and data, three escape hatches | **Recommended** |
| B. Traditional mutable system with snapshots (for example btrfs snapshots before each update) | Rollback works, but machines drift apart over time, replication is weaker and a partial change can still slip in between snapshots |
| C. Strictly immutable with no way out | Safest, but power users and developers would leave |

## Consequences

- **Easier:** everything in principles 1 to 3; support and replication.
- **Harder:** we must build the add-on mechanism and developer mode; a few apps that expect to install files into the OS need an add-on or a container; every new setting is a table in the system file with an apply, an export and a test.
- **Revisit:** which drivers and tools should be add-ons we provide ourselves.

## Action Items

1. [ ] Define the add-on format: signed, versioned, tied to a base version, rolls back with it (roadmap M7.2).
2. [ ] Design developer mode: separate change layer, `apk` inside it, reset button, warning text (roadmap M7.1).
3. [ ] Pick the first add-ons: virtualization, and an NVIDIA driver path once NVK or a glibc-compatible option is proven.
4. [ ] Define the level 1 settings as tables of the system file: appearance, shortcuts, default apps, startup, power, services (roadmap M2.1, M5.12, M5.13, M6.10).

## Principles check

Reliable keeps the OS bits identical on every machine, with the only exception, developer mode, isolated in its own layer and one command from clean. Simple holds the whole of customization to one file and one rule for what goes where. Versatile is served by settings, presets and add-ons rather than by a theme engine or an extension API.
