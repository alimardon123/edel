# ADR-012: Hardware that works out of the box

**Status:** Proposed
**Date:** 2026-10-08
**Deciders:** Alimardon
**Related:** ADR-006 (identical slots), ADR-007 (add-ons, level 2), ADR-008 (features), ADR-009 (no telemetry), M5.30 (a lighter stick), M8.17 (one image, online install), [design principles](DESIGN-PRINCIPLES.md), [roadmap](ROADMAP.md)

## Context

On 2026-10-08 Alimardon asked why Linux sound has a bad name and whether we can do better. Much of it is per laptop: Intel laptops since about 2019 need the Sound Open Firmware blobs (`sof-firmware`, 43 MiB in Alpine) and the right ALSA profiles, and makers tune their speakers only for Windows, so on Linux the same speakers sound thin. They then asked how such per-laptop pieces should reach a machine: "included just inside the installer image ... for offline installs, to make it work out of the box", or "an option to download and install online when they install", and asked for the architecture to be right from the start.

The same question covers every per-machine piece, not only sound: Wi-Fi and Bluetooth firmware, graphics firmware, and small quirk files. M5.30 already cuts the stick's firmware to a list and promises the rest as add-ons, and M8.17 already offers an online install beside the offline one. This ADR decides how those meet.

## Decision

### 1. Small text goes in the base, large blobs go in packs

What is small and plain text ships in the base for every machine and is picked at run time by the machine's own IDs: speaker tuning profiles, ALSA use-case profiles (`alsa-ucm-conf`), and quirk files. They take kilobytes, work offline and on the live stick, and need no download. What is large and binary, firmware blobs, ships in the image only when it is on the always list (section 2); everything else is a hardware pack (section 3).

### 2. The always list: start, see, type, get online

M5.30's firmware list, one owner file in `features/`, keeps in the image the firmware a machine needs to start, show its screen, read its keyboard and get online: storage, graphics, keyboards and touchpads, and the Wi-Fi and Ethernet chips of the last ten years, the reference laptop's first. A stick that cannot reach the network cannot fetch anything, so getting online is never left to a pack.

### 3. Hardware packs

A hardware pack is a signed add-on (ADR-007, level 2), one per firmware family (`firmware-sof` for Intel's sound processors, say), built with each release from the same Alpine packages and signed with the release key. Each pack lists the hardware IDs it serves (the kernel's modalias and DMI fields). An installed pack lives on `/data`, where the kernel finds it through `firmware_class.path`, never in a slot, so the slots stay byte-identical on every machine (M1.12). A pack has the version of its release; an update brings the matching packs with it, and a rollback goes back to the packs of that version.

### 4. Offline by default, online when chosen

The stick carries every pack of its release beside its slots, never inside them. The live session loads the ones the machine matches, so sound and Wi-Fi work before installing, and the installer copies only those to the new machine's `/data`. The online install of M8.17 fetches the same matching packs from the release instead of the stick. M8.17's stick budget measures what the packs add; a lighter online-only stick is made only if that measurement says it is worth a second image.

### 5. A device that arrives later

A device plugged in after installing (a dock, a USB Wi-Fi stick, a headset) whose pack is missing is named by `edel status`, by Settings and by one notification, with the pack that serves it and one button or one command (`edel addon add NAME`) to install it. Nothing is installed without the person's word.

### 6. Matching stays on the machine

The machine reads its own IDs and the packs' lists; nothing is sent anywhere, and there is no hardware database service (ADR-009). Matching is a pure function in `edel`, tested with sample IDs.

## Options Considered

| Option | Verdict |
|---|---|
| A. All firmware in every image | Rejected: `linux-firmware` alone is several hundred MiB, every machine carries what it never uses, and the light stick of M5.30 and M8.17 is lost. |
| B. The always list in the image, the rest as packs on the stick and online, small text in the base | **Recommended**: out of the box offline, a lean installed system, identical slots, one mechanism for every later device. |
| C. Online only | Rejected: no network, or a Wi-Fi chip whose firmware is the missing piece, leaves a machine without sound or network (Reliable). |
| D. An image per laptop model | Rejected: many images, against one base (CLAUDE.md's decision) and Simple. |

## Consequences

- **Easier:** a person's laptop works on the live stick and after installing with no network; a new device needs one click; the installed system stays light.
- **Harder:** packs are built, signed and tested with each release; the always list needs care as hardware changes.
- **Revisit:** when the stick's size budget (M8.17) is measured, and when phones (M9) bring their own firmware.
- **Cost:** about 3 PRs: the matching and the packs (M7.11), the sound work that uses them (M6.15), and the stick carrying them (M8.17).

## Action Items

1. [ ] Keep the always list as M5.30's one owner file (M5.30).
2. [ ] Build, sign and match hardware packs, kept on `/data` (M7.11).
3. [ ] Laptop sound that works and sounds right: speaker profiles in the base, `firmware-sof` as the first pack (M6.15).
4. [ ] The stick carries every pack, the installer copies the matching ones, online install fetches them (M8.17).

## Principles check

- **Reliable:** a machine gets online and starts with no network, slots stay identical, and rollback keeps the packs of its version.
- **Simple:** one mechanism for every per-machine blob, the add-ons that exist already; small text needs no mechanism at all.
- **Efficient:** an installed machine holds only its own firmware.
- **Functional:** sound, Wi-Fi and graphics work out of the box on the live stick.
- **Scalable:** the same packs serve a laptop, a fleet and later a phone.
- **Traded off:** the stick is larger than an online-only one would be, until M8.17's measurement says otherwise.
