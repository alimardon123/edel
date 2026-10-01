# ADR-001: Base system and desktop shell strategy for the new distro

**Status:** Superseded in part by ADR-002 (shell) and ADR-003 (base and releases)
**Date:** 2026-10-01
**Deciders:** Alimardon

## Context

The goal is one lightweight, fast, good-looking OS for consumers and developers that runs on laptops, tablets and phones (including foldables), and later on servers. The desktop should default to a traditional, Cinnamon-like layout, switch to macOS-like or Windows-like layouts with one button, adapt app window sizes between phone, tablet and laptop modes, and toggle dynamic tiling with one switch. Hardware support can be limited to a small set of devices. Updates should be reproducible.

What I found while checking the current state (as of October 2026):

- **postmarketOS already covers much of this.** It is Alpine-based, targets phones and desktops, shipped release 26.06 with GNOME 50 and Plasma 6.6, and added systemd as an option in 25.06. In March 2026 it started **Duranium**, an immutable variant with A/B image updates, mandatory LUKS2, Flatpak for apps, and Waydroid support. Duranium is still a work in progress, not daily-driver ready.
- **Alpine 3.23** (Dec 2025) ships kernel 6.18, apk-tools 3, GNOME 49, Plasma 6.5, LXQt and Sway. It stays musl-based.
- **COSMIC** (System76, Rust, Wayland) is at Epoch 1.4.0 (22 July 2026). It has a Rust compositor (cosmic-comp) with built-in tiling and is packaged on Fedora, Arch, NixOS, openSUSE and others. The 1.4.0 notes say nothing about touch or mobile. I did not verify whether COSMIC is packaged for Alpine.
- **Cinnamon** is a GTK/C desktop, historically X11-first, with Wayland support still maturing as far as I know.

Forces that decide this:

1. A distro is mostly ongoing maintenance (security updates, builds, signing, hardware testing), so the team size matters more than the architecture.
2. Phone support is a hardware problem (modem, camera, GPU, suspend), not a software-design problem.
3. Consumers expect proprietary apps (Steam, Zoom, Widevine video, NVIDIA drivers) that are typically glibc-only.
4. Desktop apps are unusable on a phone screen unless the toolkit is adaptive (GTK4/libadwaita, Qt/Kirigami).

## Decision

**Base:** build a downstream of Alpine/postmarketOS (own package repo, own images, own defaults) instead of a new distro. Use Flatpak for glibc-only apps and Waydroid for Android apps. Make the system image-based with A/B updates from the start.

**Shell:** do not rewrite Cinnamon in Rust. Build the "layout presets, adaptive sizing, tiling switch" layer on top of an existing shell. Run a time-boxed spike on COSMIC and Plasma 6 on the chosen first device, then pick one. My provisional lean is Plasma 6 if the first device is touch-first, COSMIC if it is a laptop.

## Options Considered

### Decision 1: Base

#### Option A: Downstream of Alpine/postmarketOS
| Dimension | Assessment |
|-----------|------------|
| Complexity | Medium. You maintain a small package set and images, not a whole distro. |
| Phone support | Best available. Reuses postmarketOS device ports and kernels. |
| App compatibility | Weak natively (musl), fixed with Flatpak and Waydroid. |
| Maintenance load | Low to medium. Track upstream, carry your patches. |

**Pros:** lightest base, phone groundwork exists, Duranium shows the image-based pattern, fast path to a first boot.
**Cons:** musl friction, OpenRC/systemd split to navigate, depends on upstream's direction.

#### Option B: New distro from scratch on Alpine
| Dimension | Assessment |
|-----------|------------|
| Complexity | High. Own package tree, build farm, signing, release process. |
| Phone support | Same as A, but you carry it alone. |
| App compatibility | Same as A. |
| Maintenance load | High. A security-update treadmill for one person. |

**Pros:** full control over naming, defaults and policy.
**Cons:** most of the extra control is not something users notice. High ongoing cost.

#### Option C: Minimal glibc base (Arch, Fedora bootc, Debian)
| Dimension | Assessment |
|-----------|------------|
| Complexity | Low to medium. |
| Phone support | Weak. Few phone ports, mostly vendor-specific. |
| App compatibility | Best. Steam, NVIDIA, proprietary apps just work. |
| Maintenance load | Low to medium. |

**Pros:** easiest laptop and gaming story, COSMIC is already packaged.
**Cons:** heavier, and you lose the phone groundwork that is the point of the project.

(Chimera Linux, musl plus LLVM plus apk, and NixOS are worth studying but I would not choose them over A.)

### Decision 2: Desktop shell

#### Option A: Fork Cinnamon and rewrite the core in Rust
Highest cost, no phone or adaptive story, and Cinnamon's toolkit is not touch-first. **Rejected.**

#### Option B: Build on COSMIC
| Dimension | Assessment |
|-----------|------------|
| Complexity | Medium. You add presets and any missing features on a Rust codebase. |
| Fit to your wishes | Rust, light, Wayland, built-in tiling, good looks. |
| Gaps | No documented touch or mobile story. Needs packaging on Alpine (27 components). |

#### Option C: Build on Plasma 6
| Dimension | Assessment |
|-----------|------------|
| Complexity | Low to medium. Look-and-feel packages and panel layouts already exist. |
| Fit to your wishes | Ships on Alpine and postmarketOS, has Plasma Mobile and adaptive Kirigami apps. |
| Gaps | Heavier than COSMIC. KWin's built-in tiling is manual; automatic dynamic tiling relies on third-party scripts of varying maintenance (as far as I know). Not Rust. |

#### Option D: Own compositor and shell (smithay or wlroots)
Maximum control and the closest match to the vision, but it is a multi-year project on its own. **Defer** until A to C have clearly failed.

## Trade-off Analysis

- **Phones vs apps.** Alpine gives phone support and loses native app compatibility. Flatpak and Waydroid close most of that gap, so I would pay it.
- **COSMIC vs Plasma.** COSMIC matches your taste (Rust, tiling, polish) but has no touch story. Plasma matches your phone goal but misses on auto-tiling and weight. The first device decides which gap hurts less.
- **One adaptive UI vs two modes.** A single shell that is both a phone UI and a desktop UI is the hardest thing to do well. A phone mode when handheld and a desktop mode when docked or unfolded (the Samsung DeX pattern) is much more achievable and still feels like one OS.
- **Hardware breadth.** Limiting to one device family is the right instinct. It is the single biggest lever on scope.

## Consequences

- **Easier:** first boot in weeks, not years; phone path stays open; updates and rollbacks are safe.
- **Harder:** you depend on upstream postmarketOS and Alpine; musl quirks remain for anyone running non-Flatpak binaries.
- **To revisit:** whether COSMIC gets a touch story; whether Duranium matures enough to build on directly; whether systemd or OpenRC suits your shell choice.

## Suggested phases

| Phase | Goal | Exit check |
|-------|------|-----------|
| 0 (2-4 weeks) | Pick first device. Boot postmarketOS or Alpine on it. Try COSMIC and Plasma. Measure idle RAM, boot time, battery. | One shell chosen with numbers. |
| 1 (2-3 months) | Bootable downstream image with one shell, Flatpak, layout presets MVP (Cinnamon-like default plus one more), tiling toggle. | You can daily-drive it on the device. |
| 2 | A/B updates, secure boot, disk encryption, Waydroid. | Update and rollback tested on real hardware. |
| 3 | One phone family. Phone mode and dock mode. | Calls, SMS, camera and suspend work on one device. |
| 4 | Headless server image from the same base. | Boots and updates with the same tooling. |

## What I would cut or defer

- Servers: a by-product of a clean base, not part of the first product.
- Foldable-specific behavior: prototype adaptive layouts on a touch 2-in-1 or tablet first, since it exercises the same posture and rotation logic.
- Your own desktop environment: only if the spike shows both COSMIC and Plasma are unfixable for your needs.

## Action Items

1. [ ] Decide the first device (laptop, 2-in-1 or phone).
2. [ ] Run the phase 0 spike and record idle RAM, boot time and battery for COSMIC and Plasma 6.
3. [ ] Check how Flatpak and Waydroid behave on the chosen base.
4. [ ] Write ADR-002 for the layout-preset switcher and adaptive sizing design.
5. [ ] Write ADR-003 for the update and image strategy (A/B, secure boot, encryption).

## Sources

- [postmarketOS 26.06 release (9to5Linux)](https://9to5linux.com/postmarketos-26-06-alpen-avocado-released-with-gnome-50-and-kde-plasma-6-6)
- [postmarketOS June 2026 update](https://postmarketos.org/blog/2026/07/06/pmOS-update-2026-06/)
- [Introducing Duranium](https://postmarketos.org/blog/2026/03/17/introducing-duranium/)
- [postmarketOS 25.06 with systemd (9to5Linux)](https://9to5linux.com/postmarketos-25-06-linux-mobile-os-brings-support-for-new-devices-and-systemd)
- [Alpine 3.23.0 release notes](https://alpinelinux.org/posts/Alpine-3.23.0-released.html)
- [COSMIC Epoch 1.4.0](https://www.linuxcompatible.org/story/system76-releases-cosmic-epoch-140-with-stability-fixes-and-compositor-updates/)

Claims marked "as far as I know" and the musl/glibc app list come from general knowledge, not from these sources.
