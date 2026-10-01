# ADR-005: The long-term compatibility promise (platform levels)

**Status:** Proposed
**Date:** 2026-10-01
**Deciders:** Alimardon
**Extends:** ADR-003 (which decouples apps from the base)

## Context

Releases should come often (every six or twelve months), yet systems from 10 to 20 years ago should stay compatible, including installing an app released yesterday on a system that is ten years old.

There are two directions, and they are very different in difficulty:

| Direction | Example | Difficulty |
|-----------|---------|------------|
| Backward | A 2026 app on a 2040 system | Achievable. The kernel never breaks existing programs, and ADR-003 keeps old runtimes installable. |
| Forward | A 2036 app on a 2026 system | Hard. The old system must understand the new app's package, fetch its runtime, and have a kernel with the features that runtime uses. |

A reality check: Windows 7 (2009) got new Chrome versions until early 2023 (Chrome 109), roughly 13 years, and Steam dropped it at the start of 2024. Even Windows' forward compatibility ends after a decade or so, and it usually ends because app makers stop testing old systems, not because the OS can't.

## Decision

### 1. Split the system into three layers with different update rules

| Layer | Contents | Update rule |
|-------|----------|-------------|
| Base | Kernel, drivers, musl user-space, shell | Yearly release, two years of fixes (ADR-003) |
| **App platform** | App installer (Flatpak), portals, runtimes, graphics drivers for runtimes | **Updated on every base back to the oldest supported level, independently of the base** |
| Apps | Everything the user installs | Any time |

The middle layer is the key. Because the app platform is self-contained and updates on its own, a ten-year-old base still runs today's installer and can download today's runtimes. That is what makes "yesterday's app on a ten-year-old system" possible.

### 2. Platform levels (the same idea as Android API levels)

Each yearly release defines a **platform level**: a numbered, published contract listing the minimum kernel features, the portal versions and the runtimes available.

- Apps declare the **minimum level** they need. A store on an old system only offers apps it can run, so nothing installs and then fails.
- New runtimes are built to work on the kernel of the oldest supported level, falling back gracefully when a newer kernel feature is missing.
- Portals (the system APIs apps call for files, screenshots, notifications) only ever **add** versions; nothing is removed.
- Old runtimes stay downloadable **forever** from an archive, even after their security fixes end.
- Graphics drivers for old runtimes keep being rebuilt for the promised window (ADR-003, section 5).

This puts the choice where it belongs: an app that wants to reach old systems targets an old level; an app that needs brand-new features targets a new level, exactly as Android developers do today.

### 3. The promise, in plain words

| Promise | Commitment | Design target |
|---------|-----------|---------------|
| Old apps keep working on new systems | 10 years guaranteed | 20 years |
| New apps can install on old systems | 10 years for apps that target the oldest supported level | Not promised beyond 10 |

Why not promise 20 years forward: no operating system has done it, and over 20 years the hardware itself changes (new CPU types, new graphics APIs). Promise what can be kept, and design for more.

### 4. Old *apps* yes, old *bases* only with care

A base past its two years of fixes still runs apps, but it no longer gets security fixes, and the system says so clearly. The real goal is that nobody *needs* to stay on an old base: because every update is atomic and can be rolled back (ADR-006), updating is safe, and because apps don't depend on the base (ADR-003), updating doesn't break them. The 10-year forward promise is a safety net for frozen fleets and for devices that can no longer get new kernels, such as some phones.

A longer-support base channel for servers and fleets (for example, five years) can be added later. It costs real maintenance, so it should wait until there are users asking for it.

### 5. Other CPU types

Over long periods apps meet new CPU types. On arm64 devices, x86_64-only apps (many games, some commercial apps) need an emulation layer such as FEX-Emu or box64. This belongs in the app platform layer too, so it can improve without a base release.

## Options Considered

| Option | Verdict |
|--------|---------|
| A. Three layers with an independent app platform and platform levels | **Recommended** |
| B. Freeze the base for 10 years (classic long-term support) | Rejected. Very expensive, and users are stuck with old features, which is the opposite of "fresh". |
| C. Ship every library version side by side in the base (the Windows side-by-side model) | Rejected for the base. Flatpak runtimes already do this, outside the base. |

## Consequences

- **Easier:** frequent releases without breaking anyone; a clear, checkable promise for developers and companies.
- **Harder:** the app platform must be tested on every supported base level, so CI needs one test machine image per level; the runtime archive and driver rebuilds grow over time.
- **Revisit:** whether to extend the forward window once there is data on how many users sit on old levels.

## Action Items

1. [ ] Write the platform level 1 contract (kernel floor, portals, runtimes).
2. [ ] Make the app platform installable and updatable separately from the base.
3. [ ] Add CI that installs the newest apps on the oldest supported base image.
4. [ ] Set up the permanent runtime archive.

## Sources

- [Chrome support for Windows 7 ending with Chrome 109 (gHacks)](https://www.ghacks.net/2023/01/18/here-is-what-happens-when-you-try-to-install-and-run-unsupported-browsers-on-windows-7/)
- [Steam ending Windows 7 support on 1 January 2024 (Steam community)](https://steamcommunity.com/discussions/forum/0/4030224296643120864)

The kernel compatibility rule, Android API levels and FEX-Emu/box64 come from general knowledge.
