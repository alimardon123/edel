# ADR-011: Your devices as one

**Status:** Proposed
**Date:** 2026-10-08, amended 2026-10-08 (sections 7 to 11, what only one system everywhere can do; sections 12 to 18, ideas no other system can match, and the seams kept now)
**Deciders:** Alimardon
**Extends:** M7.10 (the phone and the computer together, planned on 2026-10-07, now M7.10a)
**Related:** ADR-004 (one app adapts to every size), ADR-006 (one settings file), ADR-007 (add-ons), ADR-009 (no telemetry, no account), [review of other systems](REVIEW-apps-and-lessons.md), [design principles](DESIGN-PRINCIPLES.md), [roadmap](ROADMAP.md)

## Context

On 2026-10-07 Alimardon asked for the phone and the computer to work together, and on 2026-10-08 that it be "more seamless and interchangeble and more versatile", learning from Googlebook, Apple, HarmonyOS NEXT and NVIDIA's DGX Spark, and "far greater and better" than them, "as we are running same os everywhere". Later that day they added that we should "make use of that same os everywhere core of us" to give "even more features and seamless user experience at the top quality than others"; sections 7 to 11 are that. Asked to "think and innovate more", they then took sections 12 to 18, adding that "really hard and slow to build features can wait later after we have stable os", but that "we should prepare a way for them ... now", so that "we will not face any difficulties later": section 19 lists the seams kept now.

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

### 7. Your setup follows you

The settings file is the same format on every device, so a person chooses, section by section, what their paired devices share: the look (`[appearance]`, the wallpaper, the accent), the shortcuts, the apps, the Wi-Fi networks, the permissions (M6.14). A change on one shows on the others live, and the layout stays each device's own, as a phone and a laptop want different ones. A whole workspace moves too: "Move this desk to my laptop" carries every window of a workspace across (section 2), so a person stands up from the desk and carries on from the sofa. An app installed on one device can be offered on the others, the same Flatpak for x86_64 and arm64 (ADR-004).

### 8. One update for all your devices

Every device runs the same release, so one paired device downloads it and the others take it from that device over the local network, checking the same signature (M1.6), and a household or an office downloads each release once. The first device to confirm the new slot tells the others it is safe; if it rolls back, the others wait and say why. A person's own devices become each other's early warning, which no vendor gives a household. This shares whole signed images between a person's own devices; it is neither delta updates nor a mirror network, which stay out of the plan.

### 9. Your devices back each other up

Each device's `/data` (the settings file, `/etc`'s changes, the homes) is copied, encrypted, to another paired device the person chooses, a home server or a desktop, on a schedule they set; a new or repaired device is restored from it in the welcome (M6.12), its slots coming from the release and its data from the backup. No cloud, and any device can hold another's backup.

### 10. Every device's hardware is yours

Paired devices lend each other their hardware: speakers and microphones over PipeWire's network audio, a printer or a scanner, a phone's mobile data when the laptop has no Wi-Fi (as Apple's Instant Hotspot, for any phone that shares), storage, and the camera and screen of section 5.

### 11. Find, lock, unlock, and one of everything

A lost device can be made to ring, shown on a map when it shares its location, locked or wiped from another paired device, through the person's own tunnel when away (section 1). A phone near the laptop can unlock it, if the person turns that on. A notification dismissed on one device leaves all of them, "do not disturb" is one switch for all, and the launcher's search finds apps, files and settings on every paired device. Quick settings show every device's battery.

### 12. Your desktop in your pocket (after the stable OS)

A person's phone carries their settings file and keys. At any Edel OS computer, a friend's, a library's or a guest machine at work, a tap of the phone logs them in as themselves: their look, apps and files, served from the phone or their home machine (sections 2 and 3), in a guest session that is wiped when they leave. Roaming profiles need a company's servers; this needs only the same system on both sides.

### 13. A second life for every old device

An old phone or laptop takes a role with one choice in Settings: home server, backup keeper (section 9), second screen (section 5), webcam, speaker, photo frame, the household's update source (section 8). A role is a profile (M7.6), a fragment of the settings file, not a new mechanism, so a role is added, swapped or removed as one file.

### 14. Help me, with one button

"Help me" lets a paired helper see the person's screen and, if they allow it, take over, only while the button is on and with the panel showing it all the time. A parent fixes a grandparent's laptop from their phone, with no remote-help product and no account; it is the screen sharing of M6.3 and the window move of section 2, offered to a person instead of an app.

### 15. Undo for your whole setup

Every change to the settings file is kept as a version, on every device: "make my laptop's setup as it was last Tuesday", "undo what I changed on the tablet", each shown as a readable list of changed lines. The file is small text, so the history costs almost nothing.

### 16. Your phone is your key (after the stable OS)

An Edel OS phone is the passkey and security key for the person's devices and the websites they use (FIDO2 and passkeys, open standards), and unlocks the laptop when near (section 11), without an Apple or Google account.

### 17. Apps that truly travel (a spike first)

Beyond moving a window (section 2), a running app could move itself: frozen with its unsaved work on one device and resumed on another, as CRIU does for programs and containers today. Whether it works for everyday desktop apps is unknown, so it is measured in a spike after the stable OS before anything is promised.

### 18. All your devices as one computer (a spike first)

Idle devices that are plugged in can help with big jobs, a video export, a build, a local AI model (M7.9), only with the person's consent and never on battery. A spike after the stable OS measures whether the gain is worth it.

### 19. Seams kept now, so the hard parts come easily later

Sections 12, 16, 17 and 18 wait until the OS is stable (after M8), but what they rest on is decided now, in the steps that build those parts, so none of them needs a rewrite later:

- **Every setting says whether it travels.** The help table of every key (M6.11) records whether it belongs to the person (the look, shortcuts, apps, permissions) or to the device (its layout, its screens, its disks), so sharing (section 7), a pocket desktop (12) and roles (13) read it instead of guessing.
- **Every write of the settings file goes through one function**, `edel::settings::set` and the apply path, which CI checks (M8.13); history (15) and sharing (7) hook there and nowhere else.
- **Each device has its own identity key from its first start**, kept on `/data` beside its ssh host key, so pairing (section 1), a pocket desktop and passkeys have something to trust.
- **The compositor's screens and inputs stay behind its backend trait**, as the DRM and winit backends are, so a paired device's screen or pointer (sections 4 and 5) is one more backend, not a change to the window policies.
- **Screen sharing (M6.3) uses the standard portals and PipeWire**, so help (14) and a device as a screen (5) reuse it.
- **Roles are profiles (M7.6)**, so a second life (13) adds files, not code.
- **Nothing listens on the network until a device is paired**, the rule every section here keeps.

## Options Considered

| Option | Verdict |
|---|---|
| A. Join devices at the level of the system, with reused protocols (Wayland forwarding, KDE Connect, peer-to-peer folder sync, SSH), paired without an account | **Recommended.** Works for every app at once, because every device runs the same system; nothing new to maintain but the glue; private by design |
| B. A handoff API apps must implement (as Apple and HarmonyOS do) | Rejected as the base: most apps would never adopt it; kept only as the optional "saves its state through the portals" path of section 2 |
| C. A cloud account that syncs devices (as Google and Apple do) | Rejected: no account and no telemetry (ADR-009); a server of ours would be a part to run forever and a privacy risk |
| D. Only the KDE Connect protocol | Rejected as the whole answer: it cannot move windows or share a pointer between screens; kept for phones of other systems (section 6) |

## Consequences

- **Easier:** every app gains continuity on the day it is installed; a household downloads each release once and learns from its first device whether it is safe; a lost or broken device comes back whole from another; a person's devices, permissions and setup move with one file; a weak device borrows a strong one's power.
- **Harder:** windows over the network must stay smooth (Instant), so each step measures latency on the local network in CI before it merges, and falls back to the window staying where it was.
- **Revisit:** whether a full handoff (section 2's second path) is worth asking apps for, once the window move is in people's hands.
- **Cost:** the steps below, each one PR or two; no new long-running part, as each feature runs only while a device is paired and in use.

## Action Items

1. [ ] Phones of other systems through the KDE Connect protocol (M7.10a).
2. [ ] Pairing without an account, `[devices.NAME]` in the settings file, and the clipboard and files across devices (M7.10b).
3. [ ] Windows that move to another device, and "Run on" a stronger one (M7.10c).
4. [ ] One pointer and keyboard across devices' screens, arranged in `[displays]` (M7.10d).
5. [ ] A device as another's screen or camera, and setup from another device (M7.10e).
6. [ ] Setup that follows you, whole workspaces moving, apps offered on every device (M7.10f).
7. [ ] One download per release for all your devices, the first to confirm telling the others (M7.10g).
8. [ ] Devices backing each other up, and a new device restored from that (M7.10h).
9. [ ] Hardware lent between devices: sound, printers, mobile data, storage (M7.10i).
10. [ ] Find, lock and unlock devices; one notification, one "do not disturb" and one search for all (M7.10j).
11. [ ] Help me, with one button (M7.10k).
12. [ ] Undo for your whole setup (M7.10l).
13. [ ] A second life for old devices, as roles (M7.10m).
14. [ ] Your desktop in your pocket (M9.10), and your phone as your key (M9.11).
15. [ ] Spikes: apps that travel, and all your devices as one computer (M13.1, M13.2).
16. [ ] The seams of section 19, each in its own step: who a key belongs to (M6.11), one writer of the settings file (M8.13), a device identity key (M6.12), backends for remote screens (compositor), portals for screen sharing (M6.3), roles as profiles (M7.6).

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
