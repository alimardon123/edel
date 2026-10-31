# Edel: architecture notes

Start with the design principles, then the decisions in order. The principles govern every decision here, and each new write-up ends with a short principles check.

| File | Topic | Status |
|------|-------|--------|
| [DESIGN-PRINCIPLES.md](DESIGN-PRINCIPLES.md) | Nine ranked principles with checkable rules, fewest-parts map | 1 to 5 confirmed, 6 to 9 proposed |
| [ADR-001](ADR-001-base-and-shell.md) | First take: base and shell options | Superseded in part by 002 and 003 |
| [ADR-002](ADR-002-own-desktop-shell.md) | Own desktop shell: compositor, presets, tiling, smoothness, title bars | Proposed |
| [ADR-003](ADR-003-base-releases-app-compatibility.md) | Alpine soft fork, release model, apps decoupled from the base | Proposed |
| [ADR-004](ADR-004-adaptive-apps.md) | One app on every device, adaptive layouts | Proposed |
| [ADR-005](ADR-005-compatibility-promise.md) | 10 to 20 year compatibility promise, platform levels | Proposed |
| [ADR-006](ADR-006-atomic-updates-and-replication.md) | Atomic updates with rollback, one file to replicate a machine | Proposed |
| [ADR-007](ADR-007-immutability.md) | Immutable base, three customization levels, add-ons, developer mode and dev containers | Proposed |
| [ADR-008](ADR-008-features-defaults-and-install.md) | Features as one file each, files read across releases, defaults as the absence of a key, one way in from Settings, the terminal or a file | Proposed |
| [ADR-009](ADR-009-enterprise-the-same-system-at-work.md) | Enterprise as the same system at home and at work, appliances first, the same security for everyone, ten years as the long channel's goal, a systemd adapter only as an add-on | Proposed |
| [ADR-010](ADR-010-parts-that-stand-alone.md) | Every part can be changed, dropped or stand on its own: seams of files and protocols, one module for Edel OS's places, one owner file for every fact, checked in CI | Proposed |
| [ADR-011](ADR-011-your-devices-as-one.md) | Your devices as one: pairing without an account, every app's window moving between devices, the strongest machine lending its power, one pointer, clipboard and files, phones of other systems through KDE Connect | Proposed |
| [ADR-012](ADR-012-hardware-that-works-out-of-the-box.md) | Hardware that works out of the box: small profiles in the base, an always list to start and get online, hardware packs on the stick and online, matched on the machine | Proposed |
| [REVIEW-apps-and-lessons.md](REVIEW-apps-and-lessons.md) | How Windows, macOS, HarmonyOS and Flatpak keep apps working for decades; a 2014 Alpine app run today; why postmarketOS, AerynOS and COSMIC took years | Review |
| [REVIEW-aerynos.md](REVIEW-aerynos.md) | What we can learn from AerynOS's atomic updates | Review |
| [REVIEW-experience.md](REVIEW-experience.md) | The paths a newcomer takes, from the stick to a broken update, walked in CI with budgets of time and steps (M8.15) | Review |
| [REVIEW-bugs.md](REVIEW-bugs.md) | Other projects' bugs sorted into kinds of mistake and checked against ours, with the test or fix each led to (M8.11) | Review |
| [REVIEW-shells.md](REVIEW-shells.md) | What we take from Hyprland, niri, the Quickshell shells, GNOME, macOS and Windows 11 to look beautiful and feel instant, and what we skip | Review |
| [REVIEW-postmarketos.md](REVIEW-postmarketos.md) | What we can learn from postmarketOS (now Nura) and its immutable Duranium | Review |
| [REVIEW-project-2026-10-13.md](REVIEW-project-2026-10-13.md) | Edel OS itself, by four reviewers: the principles, the base and the desktop as independent parts, three dangers, the fixes (M6.17) | Review |
| [REVIEW-signing-2026-10-25.md](REVIEW-signing-2026-10-25.md) | The narrow review before the first preview: the updater, the guard and the signing path, what was fixed and the settings that wait for Alimardon | Review |
| [PROPOSAL-undo-keys-and-key-table.md](PROPOSAL-undo-keys-and-key-table.md) | The Undo bar from the keyboard (a Super+Z shortcut) and one entry per setting key: options and a recommendation, accepted and built | Accepted |
| [SPIKE-app-runtimes.md](SPIKE-app-runtimes.md) | Apps that carry everything but musl and the drivers, a Flatpak runtime built from Alpine, and Flathub, measured on an older and a newer base (M10.9, M10.10) | Spike |
| [SPIKE-flatpak.md](SPIKE-flatpak.md) | Flatpak's glibc runtimes on the musl base: the verdict and what CI measured | Spike |

The guide, the reference pages and the roadmap are in the docs site's table of contents, [SUMMARY.md](SUMMARY.md). The plan is [ROADMAP.md](ROADMAP.md), and [TRY-IT.md](TRY-IT.md) shows how to try Edel OS.
