# ADR-003: Base fork, release model and long-term app compatibility

**Status:** Proposed
**Date:** 2026-10-01, amended 2026-10-02 (a seed volume instead of cloud-init, roadmap M2.2)
**Deciders:** Alimardon
**Supersedes:** the base decision in ADR-001 (postmarketOS downstream). postmarketOS stays the source for phone device ports later.

## Context

Alimardon wants to work on Alpine directly and probably fork it, because Alpine is a good base that needs polish. The goals:

1. **Stable but updatable.** No surprise breakage, but current features.
2. **Windows-style app compatibility.** An app built five or more years apart from the OS should still install and run, in both directions (old app on new OS, new app on old OS).
3. **One OS everywhere.** Laptop, phone, cloud VM, container and, later, servers, so developers get the same system on their laptop and in the cloud.
4. **Efficient.** Full use of CPU and GPU for gamers and creative work, small enough for containers and edge devices.

### Correction: Alpine is not rolling

Only **edge** is rolling. Alpine cuts a stable branch every May and November. The **main** repository gets about two years of fixes; the **community** repository only gets fixes until the next stable release, about six months. Current branches: v3.24 (June 2026, main and community), v3.23, v3.22 and v3.21 (main only).

Most desktop software lives in community, which is why Alpine *feels* rolling on a desktop. That short window is the real problem to solve, not the base itself.

### Why Linux app compatibility is hard today

The Linux kernel has a strict rule never to break existing programs, so the kernel interface is already as stable as Windows'. What breaks old apps is the **user-space libraries** shipped by the distro (GTK, Qt, OpenSSL, libc versions): each distro release swaps them, and apps built against the old ones stop working. Windows avoids this by keeping old library versions installed side by side and by letting apps bring their own.

## Decision

### 1. Soft fork of Alpine

Track Alpine stable as upstream and keep **our own overlay repository** containing only what we change: the shell, defaults, kernel configuration, branding, image tooling and any patched packages. Keep musl and apk.

Become a hard fork only if Alpine's policy blocks something we need. A hard fork means owning security fixes for thousands of packages, which is a full-time team's job.

### 2. Small immutable base with A/B updates

The base image holds only: kernel, firmware, drivers, musl user-space, system services and our shell. It is read-only, updated as a whole image into the inactive slot, and rolls back automatically if the new image fails to boot.

**Rule: apps never link against the base.** That one rule is what makes the base safe to update often.

### 3. One base, several images

```
              +-------------------------------+
              |   Base (kernel, musl, apk)    |
              +-------------------------------+
               /        |          |          \
     Container     Server/VM     Desktop       Phone
     (minirootfs)  (+ ssh,       (+ shell,     (+ desktop,
                    seed file)    Flatpak)      modem stack,
                                                device ports)
```

Same packages and versions in every image, so a developer's laptop, their cloud VM and their containers match exactly.

### 4. Apps bring their own libraries (the Windows-style part)

| Kind of app | Format | How compatibility works |
|-------------|--------|-------------------------|
| Desktop apps | Flatpak | Apps run on versioned runtimes that install side by side. An old app keeps its old runtime; a new app on an old system downloads its new runtime. |
| Server apps | OCI containers | Each container carries its own user-space; only the kernel is shared. |
| Developer tools | Dev containers (distrobox style) | Any toolchain or distro inside, same on laptop and cloud. |
| System parts | apk, inside the base image | Only for the OS itself. |

This also removes the musl problem for consumers: Flatpak runtimes carry their own glibc, so glibc-only apps (Steam, Zoom and similar) run fine on a musl base.

### 5. The real gap, and where this project can beat everyone

**GPU drivers.** In Flatpak, graphics drivers ship as an extension tied to each runtime version. When a runtime reaches end of life, its driver stops being updated, so an old app on a new GPU can break. Old runtimes still install, but with a warning.

Two ways to close the gap, to be chosen with a spike:

- **Keep building current Mesa for old runtimes** for a long, published window (for example, every runtime from the last seven years). Costs CI time per runtime, but it is mechanical work.
- **Inject the system's driver into the app's runtime**, the approach Steam's own runtime takes. More elegant, but harder because our base is musl and the runtimes are glibc.

Either one, plus portals (the stable APIs apps use for files, screenshots and notifications) that are versioned and never removed, gives a compatibility promise most Linux systems can't make.

**Honest limits:** an old runtime eventually stops getting security fixes (Windows has the same issue with old libraries); a very new runtime may need a newer kernel, which is easy because base updates are cheap.

### 6. Release model

- **One base release per year**, built on the Alpine stable branch from that spring.
- **Monthly security image updates**, plus urgent fixes as needed.
- **Each yearly release supported for two years**, matching Alpine's main repository.
- For any base package that comes from Alpine's community repository, we backport security fixes ourselves for the full two years. Keeping the base small keeps this list short.
- A longer-support channel for servers can come later, once there are users who need it.

Because apps don't depend on the base, most users can simply take every update. Stability comes from testing, A/B rollback and decoupling, not from freezing everything.

### 7. Performance

What the distro controls: kernel configuration and scheduler choice (sched_ext allows tuned schedulers), current Mesa, sensible power profiles and no background services nobody asked for. Using the GPU to its limit is mostly the driver and the app; our job is to ship current drivers and stay out of the way.

**Reference hardware should use AMD or Intel GPUs at first.** As far as I know, NVIDIA's proprietary user-space driver only supports glibc, and the compositor needs it on the host, so a musl base plus proprietary NVIDIA is hard. NVK, Mesa's open NVIDIA driver, is the path to NVIDIA support later.

## Options Considered

| Option | Stability | Effort | Verdict |
|--------|-----------|--------|---------|
| A. Soft fork (overlay on Alpine stable) | Good, with our own backports | Medium | **Recommended** |
| B. Hard fork of Alpine | Fully ours | Very high | Only if A is blocked |
| C. Traditional distro (apps as apk packages, freeze per release) | Good inside a release, breaks apps across releases | High | Rejected: this is exactly the problem we want to fix |
| D. Nix-style side-by-side library store | Excellent | High, steep for users and packagers | Interesting, but Flatpak already covers desktop apps |

## Consequences

- **Easier:** frequent base updates without breaking apps; same system on laptop, VM and container; glibc apps on a musl base.
- **Harder:** we depend on Flatpak and Flathub for desktop apps; we own the driver-for-old-runtimes pipeline and the community-repo backports.
- **Revisit:** whether a longer server channel is needed; whether host driver injection beats rebuilding Mesa.

## Suggested phases

| Phase | Deliverable | Why this order |
|-------|-------------|----------------|
| 0 | Overlay repo and image builder producing the container and VM images from Alpine v3.24 | Proves the base and the pipeline with no hardware, testable in CI with QEMU |
| 1 | Desktop image on one reference laptop: compositor (floating and tiling), panel, launcher, Flatpak | First thing you can daily-drive |
| 2 | Presets (Classic, Mac-like, Windows-like), A/B updates, settings app, screen sharing | Makes it a consumer product |
| 3 | Tablet and 2-in-1 form factor; driver pipeline for old runtimes | Adaptive UI and the compatibility promise |
| 4 | Phone image using postmarketOS device ports | Hardware-heavy, so last |

## Action Items

1. [ ] Create the project repository with the overlay repo layout and an image builder.
2. [ ] Build the container and VM images from Alpine v3.24 and boot them in CI.
3. [ ] List which base packages come from Alpine community and will need our backports.
4. [ ] Pick the reference laptop (AMD or Intel GPU).
5. [ ] Spike the GPU driver approach for old Flatpak runtimes.

## Sources

- [Alpine Linux releases and support policy](https://alpinelinux.org/releases/)
- [Flatpak extensions documentation](https://docs.flatpak.org/en/latest/extension.html)
- [Flathub Discourse: updates trying to install end-of-life GL extensions](https://discourse.flathub.org/t/flatpak-update-tries-to-install-old-runtime-extensions/10447)
- [Linux Mint forums: GL.default 20.08 end-of-life warning](https://forums.linuxmint.com/viewtopic.php?t=383217)

The kernel compatibility rule, the NVIDIA glibc point and Steam's driver injection come from general knowledge, not from these sources.
