# Edel OS

**A lightweight, fast and beautiful Linux operating system for everyone.**

Edel OS is one small system that never changes under you. It runs as a container, a server, a laptop's or a desktop's system, and later a phone's. It updates as a whole, in one step, and when an update fails it goes back to the version before by itself. One plain file describes a whole machine, so the same file can set up a second one.

> **Edel OS is early and in development.** Nothing is published yet, and its desktop is only now meeting real hardware. You can already [try it](TRY-IT.md) from a USB stick or in a virtual machine, and see where it is going in the [roadmap](ROADMAP.md).

## Where to start

| You want to | Read |
|---|---|
| Try it on a laptop, in VirtualBox or in QEMU | [Try it](TRY-IT.md) |
| Understand how it works, from the disk to the desktop | [How Edel OS works](guide/how-it-works.md) |
| Describe a machine in one file, or copy one | [The settings file](settings.md) |
| Learn the keys of the desktop | [Keyboard shortcuts](SHORTCUTS.md) |
| Use the `edel` command | [Commands](guide/commands.md) |
| Know why it is built the way it is | [Design principles](DESIGN-PRINCIPLES.md) and [the decisions](README.md) |
| See what comes next | [Roadmap](ROADMAP.md) |
| Write or fix these pages | [Writing these docs](guide/writing-docs.md) |

## What makes it different

- **It never leaves you with a broken machine.** Each update goes into a second copy of the system. If the new copy does not start, or starts and hangs, the machine starts the old copy again without anyone doing anything.
- **It is small and quick.** One base, built on Alpine Linux, with the fewest parts that do the job, so it reacts on the next frame even on old hardware.
- **One file describes the machine.** Its name, its people, its layout and its look are lines in one text file, the same on a laptop and on a fleet of servers.
- **Its desktop is its own.** A compositor and a panel written in Rust, with floating and tiling windows, title bars with close buttons in both, and one-button layouts that feel like Cinnamon, macOS, Windows or a tiling desktop.
- **Apps stay apart from the system.** Desktop apps come as Flatpaks and servers as containers, so the system can update without breaking them, and old apps keep working for years.

Edel OS is free software under the [GNU GPL, version 3 or later](https://github.com/alimardon123/edel/blob/main/LICENSE). Its source, these pages included, is at [github.com/alimardon123/edel](https://github.com/alimardon123/edel).
