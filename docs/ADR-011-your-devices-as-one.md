# ADR-011: Your devices as one

**Status:** Proposed
**Date:** 2026-10-08
**Deciders:** Alimardon
**Extends:** M7.10 (the phone and the computer together, planned on 2026-10-07, now M7.10a)
**Related:** ADR-004 (one app adapts to every size), ADR-006 (one settings file), ADR-007 (add-ons), ADR-009 (no telemetry, no account), [review of other systems](REVIEW-apps-and-lessons.md), [design principles](DESIGN-PRINCIPLES.md), [roadmap](ROADMAP.md)

## Context

On 2026-10-07 Alimardon asked for the phone and the computer to work together, and on 2026-10-08 that it be "more seamless and interchangeble and more versatile", learning from Googlebook, Apple, HarmonyOS NEXT and NVIDIA's DGX Spark, and "far greater and better" than them, "as we are running same os everywhere".

What the others do, as far as we found:

| System | What it does | Its limit |
|---|---|---|
| Apple Continuity (from general knowledge) | Handoff of a task, one clipboard, AirDrop, a Mac's pointer moving onto an iPad (Universal Control), an iPad as a second screen (Sidecar), the iPhone in a window on the Mac, the iPhone as a webcam | only Apple devices, one Apple account, and only apps that implement Handoff |
| Googlebook (Android laptops, shipping from 2026-10-04) | Continue On (resume a phone task on the laptop), Cast My Apps (phone apps in a laptop window), the phone's files in the laptop's Files, setup copied from the phone end to end encrypted | needs an Android 17 phone and a Google account; Continue On works only for apps that support it |
| HarmonyOS NEXT | bring two devices close and continue a task on the other, a shared clipboard, gallery and casting, all over its "distributed soft bus" | Huawei devices only, and apps written for its distributed APIs |
| NVIDIA DGX Spark with NVIDIA Sync | the laptop keeps the editor and the windows, the Spark runs the heavy work (notebooks, models) over SSH on the local network | one tool for one kind of work (development), on one product |

Each of them joins devices that run different systems, so each needs every app to opt in, or one vendor's account, or both. Edel OS runs the same system, the same apps and the same settings file on the phone, the tablet, the laptop and the server, so it can join devices at the level of the system, for every app at once.

## Decision

### 1. Your devices are one place, without an account

A person's devices trust each other by pairing, once, with a code one shows and the other scans or types, and nothing else: no account, no cloud, no server of ours. Each paired device and what it may do is a line of the settings file (`[devices.NAME]`), so the whole set travels to a new machine with the file, and a fleet's file can pair a company's machines the same way. Devices find each other on the local network; reaching them from elsewhere is the person's own choice of a tunnel (a VPN they already use), never a relay we run (ADR-009, no telemetry).

### 2. Every app moves, not only the apps that opt in

Because both sides are Edel OS, a window can move to another device as it is: the app keeps running where it started and its window is shown on the other screen over the Wayland protocol itself, through a reused forwarder (waypipe, as far as we know, chosen in the step). So "Open on my laptop", "Move to my tablet" and "Run on my desktop" work for every app, Flatpak or not, with no change to the app. An app that also saves its state through the portals can be handed over fully, closing on one side and opening on the other where it left off.

### 3. The strongest machine lends its power

The same move works the other way round, as NVIDIA Sync does for one kind of work: from a light laptop or a phone, "Run on" starts any app, a terminal or a dev container on a stronger paired machine (a desktop, a home server, a workstation), with its window here and its work there. Nothing new is needed for it: the window moves as in section 2, and dev containers already run anywhere.

### 4. One pointer, one clipboard, one set of files

The compositor treats a paired device's screen as one more screen in `[displays]`: the pointer and the keyboard cross onto it at the edge, as onto a second monitor (Apple's Universal Control, but arranged like any screen in Settings). The clipboard is one across paired devices. Each device's files appear in the others' file manager, and chosen folders stay in step between them, peer to peer, with nothing in a cloud.

### 5. Any device can be another's screen, camera or setup

A tablet or a phone becomes an extra screen for a laptop (Sidecar), a phone's camera a webcam for the laptop, and a new device sets itself up from an old one: the welcome (M6.12) offers "Set up from my other device", which copies the settings file, the Wi-Fi networks and the apps' list, encrypted end to end, the secrets only by reference (ADR-006).

### 6. Phones of other systems join through an open protocol

Android phones join through the open KDE Connect protocol (M7.10a): notifications, replies, the clipboard, files, battery, media and remote input. An Edel OS phone speaks it too, and everything above on top.

## Options Considered

| Option | Verdict |
|---|---|
| A. Join devices at the level of the system, with reused protocols (Wayland forwarding, KDE Connect, peer-to-peer folder sync, SSH), paired without an account | **Recommended.** Works for every app at once, because every device runs the same system; nothing new to maintain but the glue; private by design |
| B. A handoff API apps must implement (as Apple and HarmonyOS do) | Rejected as the base: most apps would never adopt it; kept only as the optional "saves its state through the portals" path of section 2 |
| C. A cloud account that syncs devices (as Google and Apple do) | Rejected: no account and no telemetry (ADR-009); a server of ours would be a part to run forever and a privacy risk |
| D. Only the KDE Connect protocol | Rejected as the whole answer: it cannot move windows or share a pointer between screens; kept for phones of other systems (section 6) |

## Consequences

- **Easier:** every app gains continuity on the day it is installed; a person's devices, permissions and setup move with one file; a weak device borrows a strong one's power.
- **Harder:** windows over the network must stay smooth (Instant), so each step measures latency on the local network in CI before it merges, and falls back to the window staying where it was.
- **Revisit:** whether a full handoff (section 2's second path) is worth asking apps for, once the window move is in people's hands.
- **Cost:** the steps below, each one PR or two; no new long-running part, as each feature runs only while a device is paired and in use.

## Action Items

1. [ ] Phones of other systems through the KDE Connect protocol (M7.10a).
2. [ ] Pairing without an account, `[devices.NAME]` in the settings file, and the clipboard and files across devices (M7.10b).
3. [ ] Windows that move to another device, and "Run on" a stronger one (M7.10c).
4. [ ] One pointer and keyboard across devices' screens, arranged in `[displays]` (M7.10d).
5. [ ] A device as another's screen or camera, and setup from another device (M7.10e).

## Sources

- Googlebook: [launch (9to5Google)](https://9to5google.com/2026/09/21/googlebook-launch/), [Continue On (Chrome Unboxed)](https://chromeunboxed.com/googlebook-leak-reveals-continue-on-googles-built-in-answer-to-apple-handoff/), [the five laptops (Thurrott)](https://www.thurrott.com/?p=341859).
- HarmonyOS NEXT: [native HarmonyOS (Huawei Central)](https://www.huaweicentral.com/native-harmonyos-is-full-of-innovative-power-huawei/amp/), [its ecosystem in 2025](https://livinginharmony.substack.com/p/huawei-harmonyos-next-ecosystem-in), [distributed capabilities](https://kitemetric.com/blogs/harmonyos-next-mastering-distributed-capabilities).
- NVIDIA DGX Spark: [NVIDIA Sync](https://docs.nvidia.com/dgx/dgx-spark/nvidia-sync.html), [connect to your Spark](https://build.nvidia.com/spark/connect-to-your-spark).
- From general knowledge, not checked here: Apple's Continuity features, waypipe as the Wayland forwarder, and the KDE Connect protocol's feature list.

## Principles check

- **Reliable:** nothing depends on another device or the network: a lost connection leaves each window where it runs, and an app always runs on one machine whole.
- **Instant:** the window move and the shared pointer each have a latency budget measured in CI on the local network.
- **Simple:** reused protocols and tools, no account, no cloud, no new long-running part.
- **Efficient:** nothing runs until a device is paired, and only while it is used.
- **Scalable:** the same pairing serves one person's phone and a company's fleet, through the settings file.
- **Versatile:** every app, every device, any brand of phone through the open protocol.
- **Traded off:** apps are not rewritten for handoff, so a moved window keeps running on its first device unless the app saves its state; that is the price of working for every app at once.
