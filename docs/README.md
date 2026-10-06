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
| [REVIEW-aerynos.md](REVIEW-aerynos.md) | What we can learn from AerynOS's atomic updates | Review |
| [REVIEW-shells.md](REVIEW-shells.md) | What we take from Hyprland, niri, the Quickshell shells, GNOME, macOS and Windows 11 to look beautiful and feel instant, and what we skip | Review |
| [REVIEW-postmarketos.md](REVIEW-postmarketos.md) | What we can learn from postmarketOS (now Nura) and its immutable Duranium | Review |
| [FEATURES.md](FEATURES.md) | How images are made of feature files: the features today, the file, format 2 definitions, the merge and its refusals, worked examples | Reference |
| [FORMATS.md](FORMATS.md) | Every file format edel owns: fields, signatures, leniency, the stepping-stone rule | Reference |
| [RELEASE.md](RELEASE.md) | What CI publishes, what is kept forever, the release key and the steps that wait for Alimardon | Reference |
| [SPIKE-flatpak.md](SPIKE-flatpak.md) | Flatpak's glibc runtimes on the musl base: the verdict and what CI measured | Spike |
| [SHORTCUTS.md](SHORTCUTS.md) | Every keyboard shortcut's action, default keys and whether it is a way out; generated from `crates/edel/src/shortcuts.rs` | Reference |
| [mockups/](mockups/README.md) | Pictures of the target look, each preset in light or dark, the effect tiers, a tablet and a phone; not binding, the real values are tokens | Reference |
| [system-file.md](system-file.md) | Every key of `system.toml`, its values and the step that acts on it | Reference |
| [TRY-IT.md](TRY-IT.md) | Trying a preview in a VM or on a laptop, and sending a hardware report | Guide |
| [ROADMAP.md](ROADMAP.md) | The plan from phase 0 to the first public release and beyond, one PR per step | Proposed |
