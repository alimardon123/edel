# Review: apps, app data, live updates and long support

**Date:** 2026-10-31
**Verdict: keep Flatpak, containers and A/B slots; add a dev box and a keyring before the first public release (M7.17 to M7.19), and app undo, fast restarts and rolling restarts after it (M7.20, M10.11, M11.10, M11.11).**
**Asked by:** Alimardon, on 2026-10-31: check the weak spots of the app model for a better design; whether apps still work on another base; whether apps keep their state across updates and devices; whether the system can update while it runs, for servers; and 10 to 20 years of support with apps 5 years older or newer still running.
**Read:** ADR-003, ADR-005, ADR-006, ADR-007, ADR-011, `docs/SPIKE-app-runtimes.md` (#469, #477), `docs/platform-levels.md`, the roadmap (M7.4, M7.5, M7.6, M7.9, M7.10, M10, M11), `features/flatpak.toml`, `features/containers.toml`, `crates/edel/src/apps.rs` and `machine.rs`, as of `main` at 8de8438. Nothing was built or measured for this review; facts about Flatpak, Flathub and kernels beyond the repo are from general knowledge and are marked so.

## The short answer

| Question | Answer today | Recommendation |
|---|---|---|
| 1. Weak spots | Real, and two are bigger than "developer tools": **no keyring** for apps that store passwords, and **no way at all** for apps that are not on Flathub (`.deb`, `.rpm`, AppImage) | Add a dev box (distrobox on the containers add-on) as the second, last way to get software; a keyring in the desktop; a CI list of hard apps. Before release. |
| 2. Another base | Yes. Flatpak apps and containers do not depend on Alpine. The exceptions are the host services they talk to, which any base can give | Nothing to build; add one cheap CI check later |
| 3. App state | Survives every update and rollback (it lives on `/data`). Moving to another device: the app list moves today, the app data moves with M7.10h (backups), not yet built | Add app undo, put the keyring in the backup, keep the cloud carrier as a seam |
| 4. Update without restart | No, and the kernel makes it impossible to do fully and safely. Apps and containers already update without one | For servers: a fast restart (kexec) and rolling restarts across a group, after release |
| 5. 10 to 20 years | Promise holds for **machines and apps**, not for frozen **releases**. Frozen releases for 10 years is a team's job | Define support as "always updatable to the newest"; build M10.1 to M10.3; a long channel only for appliances, only when someone can carry it |

## 1. Weak spots of the app model

### What we cover today

- **Desktop apps:** Flatpak from Flathub, per person with `flatpak --user` (`machine.rs`), the default apps in one shared installation at `/data/edel/apps` (`apps.rs`). Permissions are Settings rows and keys (`apps.camera`, `apps.files` and the rest, M6.14). Steam's controllers come with the `gaming` add-on.
- **Servers:** OCI containers through the `containers` add-on (podman, crun, fuse-overlayfs).
- **Developers:** the developer profile is the containers add-on plus VSCodium. Command-line AI agents "run in a dev container ... set up by the person" (the M7.9 default row).
- **Not shipped:** distrobox (M7.4's note: "distrobox is not in the containers add-on"), even though ADR-003 names "dev containers (distrobox style)" as the way for developer tools.

### Where it falls short

| Kind of app | Why the sandbox or the base handles it badly | Covered today? |
|---|---|---|
| Apps that keep passwords (browsers, Signal, mail, Git credential helpers, Secrets) | They ask the Secret portal or the Secret Service on the bus; I found no keyring in `features/` (no gnome-keyring, oo7 or KWallet) | **No.** Apps fall back to storing secrets in plain files or refuse. Worth a CI check: this is a basic, not a niche |
| Apps that are not on Flathub (vendor `.deb` and `.rpm`, many commercial and in-house tools) | Built for glibc distros; nothing on our musl base can install them | **No way at all** |
| AppImages | Need FUSE and usually the host's glibc libraries; on musl most fail | **No** (AppImage appears nowhere in the docs) |
| IDEs and terminals in a sandbox (VSCodium) | Every compiler, debugger and language server must live inside the sandbox or be reached through `flatpak-spawn --host`, where our host has none | Partly: VSCodium can open a podman dev container, but only with podman's socket opened to it, which nothing sets up |
| Developer command-line tools (`pip`, `npm -g`, `cargo`, Homebrew, `apt`) | The base is read-only and musl, with no compiler; Homebrew and most prebuilt binaries want glibc | Partly: static binaries (Go, Rust) run as they are; the rest needs a container the person builds by hand in a terminal |
| Apps that need a kernel part (VirtualBox, OBS's virtual camera on v4l2loopback, some VPN clients) | A sandbox cannot load modules, and modules must match our kernel | Through add-ons only, one by one (none of these yet) |
| System tools (GParted, disk and partition tools, firmware updaters) | Need root and raw devices | Mostly by design: our own Settings and `edel` do these jobs |
| Hardware tools (Arduino, serial ports, scanners, some printers' tools) | Need `--device=all` or host udev rules | Works with Flatpak's own permission; scanners need SANE on the host |

### A better design: three doors, each for one kind of software

1. **Flatpak for every desktop app** (unchanged, ADR-003).
2. **One dev box for everything else**, the door we are missing: distrobox (a small shell script) on the containers add-on, with one default glibc image. What the box installs (an `apt install`, a vendor `.deb`, an AppImage, Homebrew, `pip`, `npm`) shows in the launcher and on the host's `PATH` through `distrobox-export`, and the terminal can open in it. Because it is a container on `/data`, it never touches the base, survives every update and rollback, and is the same on the laptop, the server and another base. This closes the `.deb`, `.rpm`, AppImage and developer tool gaps with one reused part and no mechanism of ours. Customization level 2 (it rides the containers add-on), added with one line in `features/containers.toml` and dropped the same way.
3. **Add-ons for what must touch the kernel or the host** (drivers, kernel modules, VPN plugins), as today.

Plus two fixes inside the first door:

- **A keyring in the desktop:** the Secret Service and the Secret portal, started by D-Bus only when an app asks (so no background service while nobody uses it). This needs its one-paragraph reason as a new dependency; my default would be the smallest that serves both interfaces (oo7, in Rust, if Alpine has it, else gnome-keyring), chosen in the step by measurement.
- **The IDE path made whole:** the developer profile opens podman's socket to VSCodium and makes the default dev box, so "open folder in container" works with no terminal.

And one proof: **a CI list of hard apps**, each a small desktop-test case: a password saved by a Flatpak and read back after a restart, an AppImage and a `.deb` app from the dev box shown in the launcher, VSCodium in a dev container, Steam (already a stand-in), a serial device. A new hard app is one case.

**Principles:** Reliable and Functional drive it (the basics must work); Simple holds because distrobox, podman and a keyring are reused, not written, and there is still one way for each kind of software. Traded off: a dev box image is large (a few hundred MB) and only made when the person or the developer profile asks.

Planned as M7.17 to M7.19, before the first public release.

## 2. Do apps keep working on a different base?

**Yes, by design, and the repo already guards it.** A Flatpak runtime carries its own glibc and libraries, and its graphics drivers come as Flathub extensions matched to the runtime, not from the host. An OCI container carries its whole user space. The M10 spike even showed an old runtime without its own driver drawing with a newer host's Mesa. ADR-010 keeps Alpine's details in four places so the base can be swapped.

What an app still needs from whatever base it runs on, all standard and all available on any modern distro:

| From the host | Why | Note |
|---|---|---|
| The kernel with user namespaces and seccomp | bubblewrap's sandbox | Any kernel from the last decade |
| Flatpak and bubblewrap themselves | to run the app | The platform level sets the oldest version |
| The portals and their backends | files, screenshots, screen sharing, settings, secrets | Our Settings portal and the colours (M6.18) speak the standard `org.freedesktop.appearance` keys, so another base's portal serves them too |
| PipeWire, D-Bus, a Wayland compositor (and XWayland) | sound, buses, windows | Standard protocols |
| Fonts | the host's fonts are shown to apps | Missing fonts only change looks |

The real exceptions are the same on every distro: **NVIDIA's own driver** (its Flatpak extension must match the host's kernel module version, and it does not exist for musl at all), **kernel modules and udev rules** an app needs, and **input methods**, whose framework runs on the host. None of these is Alpine's fault, and none would be fixed by another base.

**Recommendation:** nothing to build. Later, a cheap check: the "Desktop on another distribution" job could also install and start one Flatpak app on Ubuntu, so the claim is tested, not only argued.

## 3. App state across updates, rollbacks and devices

**Updates and rollbacks:** apps, their runtimes and their data (`~/.var/app/ID`, `~/.local/share/flatpak`) live in the homes on `/data`, which no slot holds (`features/flatpak.toml`'s reason says so). So a base update or rollback never touches an app or its state. That part is solid.

Two gaps:

- **Rolling back an app.** A system rollback does not roll back an app, and nothing in Settings or `edel` undoes an app update. Flatpak can install an older commit (`flatpak update --commit=`, from general knowledge), but an app that has moved its data to a new format may not read it in the old version. Recommendation: Settings' app page and `edel` get "Undo the last update" for an app, built on Flatpak's own commit history, with a snapshot of that app's `~/.var/app/ID` taken before each app update (small, and kept only for the last one). One step, after release.
- **The person's apps run per person.** Each person installs their own runtimes, so two people on one machine keep two copies (a few GB). Fine for most homes; worth knowing for shared family machines. The default apps are already shared.

**Another device:**

| What | Moves today? | How |
|---|---|---|
| Which apps, their permissions, every setting | Yes | The settings file (`apps.installed`, `apps.*`), `edel settings export` and `apply` |
| The apps' data and the person's files | Not yet | M7.10h (devices back each other up, `/data` encrypted to a paired device, restored by the welcome) and M7.10c (folders in step) |
| A running app, live | Only its window (M7.10c's "Move to"), not its process | ADR-011's trade-off: an app continues elsewhere only if it saves its own state |
| Passwords and keys | No | No keyring yet (section 1); some apps encrypt their data with a key from the keyring, so their data copied without it will not open |

**Recommendation:** M7.10h should copy the keyring with the homes (it is already encrypted end to end) and restore the machine's identity the apps rely on. With that, "copy my devices, continue where I was" works for every app at once, which is ADR-011's goal.

**The optional cloud.** ADR-011 already leaves the door: "trust lives in the keys, never in the way bytes travel ... later a sync server is one more way to carry". So a cloud would be **a carrier, not an account**: a server that holds only encrypted blobs it cannot read, the person's identity being their devices' keys plus one recovery phrase (needed when every device is lost). Find, lock and wipe a lost device ride the same channel. Whether to offer it reopens ADR-011's option C in part, which is Alimardon's call; nothing built now blocks it.

## 4. Updating the system while it runs

**What we do now:** `edel update` writes the new system into the other slot while the machine keeps working, so the work is done live; only the switch needs a restart. Servers can install and restart by themselves inside a window (`updates.automatic = "install-and-restart"`, `updates.restart_window`, M7.5). Flatpak apps update while running (the new version starts next time), and containers update on their own. A VM restart is about 6 to 8 seconds today (the boot budget), plus starting the containers.

**Why a full live update is not possible safely:** the kernel cannot be replaced while running, and swapping user-space files under running programs is exactly what A/B slots avoid; it is how traditional distros end up half updated.

| Option | What it gives | Cost and risk | Verdict |
|---|---|---|---|
| kexec into the new slot | Skips the firmware, which on a real server takes 1 to 3 minutes (from general knowledge), so a restart becomes seconds | `edel` must write GRUB's try counters itself so the guard still falls back; some drivers dislike kexec; Secure Boot (M10.6) allows only signed kernels | **Yes, for machines without a screen, after release**, measured on bare metal, with a normal restart as the fallback |
| Rolling restarts across a group | Real high availability: machines of one group restart one at a time while the others serve, behind the person's load balancer | One key in the settings file (a group name), read by `edel update --automatic`; no daemon if the machines check a shared lock over SSH or the update source | **Yes, the actual answer for servers**, with the fleet work (M7) or after release |
| Restarting only changed services | Seconds | Breaks the read-only slot model | No |
| Live kernel patches (kpatch, livepatch) | No restart for kernel fixes | One patch built and tested per security fix, per kernel: what Canonical and Red Hat sell as a paid team service | No, not for one maintainer |
| Restarting user space only (as systemd's soft-reboot) | Keeps the kernel, faster than kexec | Useless when the kernel changed, which most monthly images do | No |

**Recommendation:** for consumers, today's design is right (update in the background, restart when it suits them). For servers, high availability comes from more than one machine, as every cloud does it; we make that easy with rolling restarts and make each restart short with kexec. Both after release; neither changes the slot design.

## 5. 10 to 20 years of support, and apps 5 years older or newer

**What is promised today:** each yearly base gets two years of fixes (ADR-003); old apps keep working 10 years (guaranteed) and 20 (design target); new apps install on old systems for 10 years if they target the oldest level (ADR-005); a long channel with 10 years as its goal waits for someone to carry it (M11.5).

**The honest limit:** keeping one *release* safe for 10 to 20 years means fixing every security hole in its kernel, musl, OpenSSL and the rest ourselves after Alpine stops (about two years) and after the kernel's LTS ends (now about two years too, from general knowledge). RHEL does it with hundreds of engineers. The one cheaper route for a narrow case is the Civil Infrastructure Platform's super long term kernels (10 years, from general knowledge), for an appliance image only.

**The better promise, which our design already enables:** support the *machine and its apps*, not the frozen release. Because apps never link against the base, a 2026 machine updates to the 2036 base safely (A/B, rollback), and its apps keep running. So "10 to 20 years" becomes: every Edel OS machine can always update to the newest release, tested in CI from every older one (M3.5's upgrade test, from each entry of `ci/levels.toml`), and its apps keep working.

**Apps from 5 years ago on today's system:** they keep their old runtime. Risks and fixes:

| Risk | Fix | In the plan |
|---|---|---|
| Flathub stops serving an end-of-life runtime | Our own mirror of every runtime we shipped | M10.3 |
| An old runtime's graphics driver does not know a new GPU | Build current Mesa as a GL extension for old runtimes; the spike showed host Mesa also works, kept as a seam | M10.2 |
| An old runtime gets no security fixes | The sandbox limits the harm; Settings should say "this app uses an old runtime" | Not yet: a small line for M10 |

**Apps from 5 years later on today's system:** the new app pulls its new runtime, which works on an old kernel (glibc needs only a very old kernel). What breaks is an old **Flatpak or portal** that does not know the new app's needs. M10.1 makes the app platform (Flatpak, portals) an add-on updated on the oldest kept base, which is exactly the fix.

**What it costs:** CI time for one test machine per kept level; storage for the runtime archive (a few GB per runtime branch, tens of GB over the years, from general knowledge); one GL extension build per old runtime a year. All mechanical, none needs a team. The frozen 10-year release does need one.

**Recommendation:** keep M10.1 to M10.3 as planned, right after the release; write the promise as "always updatable, apps keep working" in ADR-005's words when M10 starts; keep the frozen long channel (M11.5) for appliances only, on a CIP kernel, only when someone can carry it. Changing ADR-005's promise wording waits for Alimardon's word.

## What needs Alimardon

On 2026-10-31 Alimardon said to plan it all ("Okay, you can plan it"), so the steps named in the verdict are in the [roadmap](ROADMAP.md). What is left for their word:

1. The cloud carrier (section 3) and the promise wording (section 5) are decisions only Alimardon can reopen; nothing is needed now.

## Principles check

- **Reliable:** every recommendation keeps the read-only slots and the guard; kexec keeps the fallback, rolling restarts keep a group serving.
- **Functional:** passwords, non-Flathub apps and developer tools are basics, so they come before the release.
- **Simple and modular:** each fix reuses a part (distrobox, podman, a keyring, kexec, Flatpak's commits) and adds no mechanism of ours; each is one add-on line, one key or one step.
- **Efficient:** the dev box and the keyring run only when asked; nothing new runs in the background.
- **Scalable:** the same dev box on laptop and server; rolling restarts for fleets.
- **Traded off:** a frozen 10-year release is not recommended, so a fleet that refuses every base update is served only by a later long channel.
