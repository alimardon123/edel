# Platform levels

**Date:** 2026-10-17 (roadmap M8.8)
**Related:** [ADR-005](ADR-005-compatibility-promise.md) (the compatibility promise), [ADR-003](ADR-003-base-releases-app-compatibility.md) (apps never link against the base), [roadmap](ROADMAP.md)

A platform level is a numbered promise to people who make apps: what every Edel OS release of that level, and every later one, gives an app to run on. It is the same idea as Android's API levels (ADR-005, section 2). An app says the lowest level it needs, and any system of that level or a newer one runs it.

## What a level promises

Each level is one file, `platform/level-N.toml`, at the root of the repository ([level 1 on GitHub](https://github.com/alimardon123/edel/blob/main/platform/level-1.toml)). That file is the contract; this page explains it and copies none of its numbers. A level names:

| Part | What it promises | How it is kept |
|---|---|---|
| The kernel | The oldest kernel an image of the level boots, so a runtime may use what that kernel offers | `edel image build` refuses an image whose kernel package is older |
| Flatpak | The oldest Flatpak that installs and runs the level's apps | The same build check, for every image that carries Flatpak |
| The portals | The oldest `xdg-desktop-portal` and the backends that answer it (the file chooser, screen sharing), the system's interfaces apps call; portals only ever add versions | The same build check, for every image with the feature that brings each one |
| The runtimes | The Flatpak runtimes apps of the level run on | CI's Flatpak test installs and runs the newest branch Flathub serves on every change |

A container image has no kernel of its own, so it promises only what it carries; a server or VM image without the desktop promises its kernel. The check runs where each image is built, from the packages it actually holds, so no image of a release can fall below the level it claims.

## How levels move

- The first release of a level sets its floors to the versions it ships, to the minor number. A later release of the same level may only ship the same versions or newer ones.
- A level never changes once a release has shipped it. A yearly base release (ADR-003) may start the next level, which promises at least what the one before it did.
- Every image says its level in `/usr/lib/os-release` as `EDEL_PLATFORM_LEVEL`, and a release's level is beside its version in `ci/levels.toml`, which the upgrade test reads.

## On a machine

`edel platform status` prints the machine's level, each floor beside the version the machine has, and the runtimes. It changes nothing and anyone may run it.

## Principles check

- **Reliable:** an image that would break the promise never builds, so nobody receives one.
- **Simple and modular:** one small file per level is the whole contract; the build, the command and this page read it, and nothing writes its numbers twice.
- **Efficient:** the check reads the package list the build already makes; nothing runs on machines.
- **Scalable:** the same level holds from the container to the desktop, each image checked for what it carries.
- **Traded off:** a level does not list each portal interface's version; the portal package's version stands for them, which is coarser but checkable.
