# How Edel OS works

This page explains Edel OS from the bottom up: the disk, how updates go in and come back out, the one file that describes a machine, how images are put together, and the desktop on top. Each part links to the page that holds its details.

## One base, several images

Every Edel OS machine runs the same base: Linux, Alpine Linux's small C library (musl), its tools (busybox) and its service manager (OpenRC), plus `edel`, our one tool. What differs between a container and a laptop is only which **features** an image adds on top.

| Image | What it is for |
|---|---|
| Container | The base as a container image, to build servers and tools on |
| VM | A server or a virtual machine, with a text console and ssh |
| Desktop | Our desktop, with the kernel and firmware real laptops and desktops need; also the stick to try and install from |
| Phone | Later (roadmap M9) |

An image is a list of features, and a feature is one small file naming the packages, services and programs it brings and why ([Features and images](../FEATURES.md)). `edel image build` turns the list into an image. So adding Wi-Fi tools to every laptop, or dropping something from every server, is one line in one file.

## The disk: two systems and your data

An installed machine's disk has four parts ([the disk layout](../FORMATS.md#the-disk-layout-m12-slot-size-m33b)):

```text
┌──────────────┬────────────────┬────────────────┬──────────────────────────┐
│ boot (ESP)   │ slot A         │ slot B         │ data                     │
│ GRUB and its │ a whole system │ the other one: │ your files (/home), /var │
│ try counters │ read-only      │ next or last   │ and the machine's        │
│              │                │                │ settings                 │
└──────────────┴────────────────┴────────────────┴──────────────────────────┘
```

- **The slots** each hold one complete system. The running one is mounted read-only, so nothing, not even a program run by mistake as root, can change the system's own files. The other slot holds the next update or the version before.
- **The data partition** holds everything that is yours or this machine's: people's homes, logs and caches under `/var`, the settings file, and the few files in `/etc` this machine changed, which lie over the slot's own. It is shared by both slots, so it survives every update and every rollback.
- **The boot partition** holds GRUB, which picks the slot to start, and a tiny file of counters that says which slot is good and how many times a new one has been tried.

## Updates that go back by themselves

An update never changes the running system. It fills the other slot, and the machine only switches when the new slot proves it works.

1. **Check.** `edel update URL` reads the release's list, `release.toml`, and refuses it unless one of the keys the system carries signed it, unless it is newer than the running version, and unless it is still in date and for the machine's channel ([`release.toml`](../FORMATS.md#releasetoml-format-1-m16)).
2. **Write.** It streams the new system into the other slot, checks it byte for byte against the signed list, and tells GRUB to try that slot next.
3. **Try.** At the next start GRUB counts one try and starts the new slot.
4. **Prove.** While the system starts, a guard (`edel boot guard`) keeps the machine's watchdog timer fed and waits for the image's health checks: every service started and, on a desktop, the compositor has shown its first frame. When they pass in time, the slot is **confirmed**, and only then does a new boot loader, if the update brought one, take its place.
5. **Fall back.** If the new slot freezes, the watchdog restarts the machine; if it hangs, the guard stops feeding the watchdog and the same happens. Once the tries run out, GRUB starts the old slot again, which notes what happened and switches the broken slot off.

```text
edel update URL
   │  the new system goes into the other slot
   ▼
restart: GRUB counts a try and starts the new slot
   │
   ▼
healthy in time? ── yes ──► confirmed: the new slot is the system now
   │
   no: the watchdog restarts the machine
   ▼
tries left? ── yes ──► GRUB tries the new slot again
   │
   no
   ▼
the old slot starts again and switches the new one off
```

`edel rollback` goes back by hand, and `edel status` shows both slots and their versions. The same path carries everything, the boot loader included and add-ons later (roadmap M7.2), so there is one way to update and one way back.

## One file describes the machine

The **settings file** is one TOML file that describes a whole machine: its name, its people and their ssh keys, its layout and look, its screens, its shortcuts ([The settings file](../settings.md)). It never holds personal files or passwords.

- At the first start a machine takes its file from a USB stick labelled `EDEL-SEED`, from the boot partition or from the image, and keeps it on the data partition.
- At every start the `edel-settings` service applies it: the hostname, the people, their keys.
- `edel settings set KEY=VALUE` changes one line and keeps everything else in the file as it was, comments included; the desktop follows window, screen and shortcut settings at once.
- `edel settings export` writes this machine as a file, and `edel settings diff` and `apply` make another machine match one. One file can set up a fleet.

A setting is never a second mechanism: each one is a line in the file, a command, and a row in Settings, page by page as roadmap M5 adds them (ADR-008). A missing line means the release's default, so a machine's file holds only what was chosen.

## The desktop

The desktop is two programs we write, both in Rust, and a few we reuse.

| Part | What it does |
|---|---|
| **edel-compositor** | Draws every window and routes all input: floating and tiling windows per workspace, title bars with close, minimize and maximize, workspaces, keyboard shortcuts, every screen at its own scale, effects and animations that drop to a lighter tier when frames run late |
| **edel-shell-ui** | Draws the panels and docks, the launcher, the window switcher and the workspace buttons, and tells apps the colour scheme and accent; the compositor starts it and starts it again if it stops |
| **edel-settings** | Settings, the app for every setting: a sidebar of pages, with Layout (the presets and tiling), Displays (each screen's scale, resolution, place and whether it is on) and About so far, each change one line of the person's settings file, which the desktop follows at once (M5.6) |
| greetd | Logs people in; its greeter runs on our compositor and comes back if a session ends |
| Mesa and PipeWire, and from roadmap M6 NetworkManager and Flatpak | Graphics, sound, networks and apps, reused as they are |

**Layouts are presets.** One setting, `layout.preset`, switches the whole desktop between Classic (Cinnamon-like, the default), Mac-like, Windows-like and Hive (tiling), live, with no restart ([Keyboard shortcuts](../SHORTCUTS.md) work the same in all of them).

**One look, in one place.** Every colour, size, corner and the font come from one file of design tokens, `design/tokens.toml`, and the shell's icons are SVG files in `design/icons/`. The compositor, the panel, GTK apps and these pages all read them, so changing one token changes everything that shows it ([The look](../mockups/README.md)).

## Three levels of change

How much a person can change follows three levels (ADR-007):

1. **Settings:** any value in the settings file, for everyone, from Settings or `edel settings set`.
2. **Add-ons and profiles:** signed extras that ride the same update path and roll back with the slot (roadmap M7.2).
3. **Developer mode:** for the system's own files, off by default, where `apk` and the like are allowed (roadmap M7.1).

## Apps

Apps never link against the base, so the base can update without breaking them ([ADR-005](../ADR-005-compatibility-promise.md)): desktop apps are Flatpaks (roadmap M6.1), servers are containers, and development happens in dev containers that bring whatever distribution a project expects.

## How it is tested

Every change is built into every image and started in virtual machines before it can merge: a fresh install, an update and a rollback, a broken update that must fall back, the desktop's windows, panels and shortcuts checked pixel by pixel, and the size, memory, boot time and frame-time budgets ([`ci/`](https://github.com/alimardon123/edel/tree/main/ci)). A change that breaks any of them does not merge.
