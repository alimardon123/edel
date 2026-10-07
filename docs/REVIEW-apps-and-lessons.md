# Review: how other systems keep apps working, and why new systems take years

**Date:** 2026-10-07
**Asked by:** Alimardon, on 2026-10-07: whether an app could be installed "itself" and run on any Edel OS version for twenty years without a runtime step, how Windows and HarmonyOS do it, and why postmarketOS, AerynOS and COSMIC took years to reach people ("their mistake is their successes at their learning ... will help us to kind of do things better").
**Reviewed:** a test run in this session (Alpine v3.0 packages on Linux 6.18), Alpine's mirror listing, HarmonyOS NEXT developer write-ups, the postmarketOS article and FOSDEM talks, AerynOS's release news, and COSMIC's release news and founder's letter (links under Sources).

Two questions, one page: how an app keeps working for decades, and what slowed the three projects closest to ours.

## How systems keep old apps working

Every system that keeps apps running for decades does the same thing: a small part the system promises never to break, and everything else carried by the app or fetched beside it.

| System | What the system promises | What the app carries | What it costs |
|---|---|---|---|
| Windows | Win32, DirectX and the driver model, kept for about thirty years | its own DLLs in its folder; a shared runtime (Visual C++, .NET) installed beside it, often unseen | Microsoft carries old behaviour forever; apps update their own libraries |
| macOS | a small set of system libraries and Metal | everything else, inside the `.app` folder | Apple drops old interfaces now and then (32-bit apps in 2019), so its promise is shorter |
| Android and HarmonyOS | the framework and the language runtime, numbered as API levels | only the app's own code and libraries | the vendor keeps every API level working; an app names its lowest level (`compatibleSdkVersion` in HarmonyOS) and the level it was tested on (`targetSdkVersion`), and the store hides it from older systems |
| Flatpak | the kernel, the portals and the sandbox | a runtime it names, fetched once and shared | runtimes are large and must keep getting graphics drivers |
| Edel OS today (ADR-005) | the kernel, the portals, and old runtimes kept downloadable | Flatpak apps and their runtimes | as Flatpak, plus our runtime archive and driver rebuilds (M10.2, M10.3) |

HarmonyOS is the clearest example of the Android model. Its apps are packages of modules (HAP, with HSP modules shared inside one app), compiled to bytecode that the system's own Ark runtime runs; the system, not the app, carries the runtime and the frameworks. That makes apps small, but only because Huawei promises every API level for as long as it says it will. HarmonyOS NEXT (API 12) also shows the price of breaking that promise: it dropped Android apps altogether, and the apps had to be rewritten for it.

**What we found for Edel OS.** Alpine still serves every release from v3.0 (2014) on, and nothing older (its mirror listing, 2026-10-07). In this session, Lua 5.2 and `tree` from Alpine v3.0, with the musl 1.1.4 they were built with, ran on Linux 6.18 from one folder of 996 KB. Linux keeps old programs working, and musl promises that a newer musl never breaks an older program. So the small, frozen part could be Edel OS's own:
- the kernel;
- musl;
- the graphics drivers, which must be the system's own to know new hardware;
- the standard protocols (Wayland, PipeWire, D-Bus and the portals).

Everything else would travel inside each app, built from Alpine's packages. That is the Windows and macOS model without a runtime step.

**What is hard about it.**
- The graphics drivers load into the app's own process and need some libraries the app also carries, such as the C++ library; Valve's Steam Runtime solves the same clash by choosing the newer copy.
- Each app must update the libraries it carries itself, as on Windows.
- Apps built only for glibc (Steam, Chrome, VS Code) still need Flathub.
- It would loosen the decision that apps never link against the base (ADR-003, ADR-005) to "apps link only against the frozen part", which waits for Alimardon's words.

M10.9 and M10.10 measure it before anything changes.

## Why the three projects closest to ours took years

| Project | Started | Reached people | What slowed it | What went right |
|---|---|---|---|---|
| postmarketOS | 2017 | releases twice a year, but few phones are daily drivers | each phone's kernel is the vendor's fork; cameras, modems and fingerprint readers have drivers only there, so every phone needs years of porting to mainline Linux, by volunteers, one device at a time ("the missing 20%", FOSDEM 2025); the device list grew faster than the people | built on Alpine and mainline Linux; regular releases; moving to one generic kernel |
| AerynOS (Serpent OS) | about 2020, as far as we know | still alpha in 2026 (2026.02, 2026.05) | its own package manager, build tool and package collection, all written from scratch; a rename; a co-founder left in spring 2025; first installs failed | atomic updates that work; it now ships the desktops people want (GNOME, Plasma, COSMIC) instead of its own |
| COSMIC | 2021 | Epoch 1 on 2025-12-11, with Pop!_OS 24.04 | a whole desktop, its toolkit and its apps, written from scratch in Rust, took a paid team about four years; the first stable release still had rough edges | a proven base (Ubuntu) under the new desktop; a public alpha in 2024 for feedback; small, well-bounded parts; quick fixes after launch (about 1,200 merges from 172 contributors since 1.0) |

From memory, the older attempts fit the same pattern. Firefox OS, Ubuntu Touch and MeeGo each lost to the app gap (too few apps people need) and to depending on hardware makers and one sponsor.

## Lessons for our project

1. **Reuse instead of rewriting the world.** AerynOS wrote its packages and tools from scratch and is still in alpha; COSMIC wrote a toolkit and apps and took four funded years. We take Alpine's packages and apk, GTK, Mesa and Flathub, and write only four parts (ADR-002, ADR-003). Keep it that way: the app bundle idea stays a measured spike (M10.9), not a second app format, until the numbers say so.
2. **Few devices, done fully.** postmarketOS shows what every phone costs. We test on one reference laptop and QEMU, and phones come only in M9, on phones with mainline kernels.
3. **People can try it early.** COSMIC's public alpha brought feedback a year before 1.0. Our desktop stick already exists for trying (M3.6); the first preview (M3.4) waits only for the narrow review and Alimardon's release key, and the sooner it is out, the sooner real machines teach us.
4. **The app gap kills new systems.** Firefox OS and Ubuntu Touch died of it. Flathub's apps run on Edel OS from the start, which closes most of the gap without our own store.
5. **A bus factor of one is a risk.** AerynOS lost a co-founder; our project has one person and Claude sessions. Everything lives in the repository (CLAUDE.md, the roadmap, the ADRs), so any session can carry on, and nothing lives only in a chat.
6. **Measure, then promise.** Each of them had rough first releases. Our gates (budgets, boot, install, update and rollback on every PR) are the guard; keep them on and never relax a budget to merge.

**Verdict: keep Flatpak as the one app format, keep reusing instead of rewriting, and measure the self-contained bundle and an Alpine-built runtime in M10 (M10.9, M10.10) before anything changes.**

## Sources

- [Alpine's mirror](https://dl-cdn.alpinelinux.org/alpine/), listed in this session; the v3.0 test ran here.
- [Rich Felker on musl's ABI promise (2019)](https://www.openwall.com/lists/musl/2019/02/26/1).
- HarmonyOS: [package types and SDK versions (DEV Community)](https://dev.to/zhzlm/journey-of-harmonyos-next-deveco-studio-user-guide-3-26ho), [API levels and the store (DEV Community)](https://dev.to/harmonyos/how-can-a-mobile-app-market-with-a-low-api-version-search-for-apps-with-a-high-api-version-2i7c), [NEXT dropping AOSP (Alibaba Cloud)](https://www.alibabacloud.com/blog/explore-mobile-performance-monitoring-harmonyos-next-agent-architecture-and-technical-implementation_602571), [5.1.0 adaptation notes](https://dev.to/qingkouwei/harmonyos-next-510-release-update-notes-and-adaptation-guide-1fc1). These are community write-ups, not Huawei's own documentation.
- postmarketOS: [Wikipedia](https://en.wikipedia.org/wiki/PostmarketOS), [FOSDEM 2025, the missing 20%](https://fosdem.org/2025/events/attachments/fosdem-2025-4836-kernel-support-for-mobile-linux-the-missing-20-/slides/236975/FOSDEM_20_9m2KNpA.pdf), [FOSDEM 2026, mainline kernel for Fairphones](https://fosdem.org/2026/schedule/event/mainline_kernel_for_fairphones_-_2026_update).
- AerynOS: [2026.02 alpha (Linux Journal)](https://www.linuxjournal.com/content/aerynos-202602-alpha-released-advancing-modern-atomic-linux-vision), [new branding (Linuxiac)](https://linuxiac.com/aerynos-gets-new-branding-and-updated-desktop-stacks/), [distributions to watch in 2026 (Linuxiac)](https://linuxiac.com/two-linux-distributions-im-watching-closely-in-2026/).
- COSMIC: [Wikipedia](https://en.wikipedia.org/wiki/COSMIC_desktop), [Epoch 1 (The Register)](https://www.theregister.com/2025/12/22/popos_2404_cosmic_epoch_1/), [founder's letter (System76)](https://system76.com/blog/post/pop-os-letter-from-our-founder), [Epoch 1.1 (Phoronix)](https://www.phoronix.com/news/COSMIC-Epoch-1.1).
- From general knowledge, not checked in this session: Windows' and macOS's app models, Valve's Steam Runtime, AerynOS's start date, and the fates of Firefox OS, Ubuntu Touch and MeeGo.

## Principles check

- **Reliable:** the lessons favour reuse, few devices and gates over new parts; the bundle idea is measured before it can touch a decision.
- **Simple:** one app format (Flatpak) stays until a measurement shows a second is worth it.
- **Efficient:** the bundle and runtime spikes exist because both could make apps much smaller; they are measured, not assumed.
- **Traded off:** none yet. The spikes cost CI time and a few PRs in M10, and nothing in the base changes before their results.
