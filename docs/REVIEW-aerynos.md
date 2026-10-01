# Review: what we can learn from AerynOS

**Date:** 2026-10-01
**Reviewed:** [aerynos.com](https://aerynos.com/), its docs at [aerynos.dev](https://aerynos.dev/), its [blog](https://aerynos.com/blog/) up to 31 August 2026, and the [os-tools repository](https://github.com/AerynOS/os-tools) (moss and boulder).

## What AerynOS is

An independent Linux distribution (formerly Serpent OS) built entirely with its own tools, written in Rust:

- **moss**, the package and system-state manager, with atomic transactions and rollback.
- **boulder**, the build tool, with simple YAML recipes, containerized builds and caching.
- **blsforme**, which manages boot entries automatically.

It ships GNOME, Plasma and COSMIC plus several tiling window managers (Hyprland, niri, Sway and others). It uses systemd, and it is a rolling release.

**Status: alpha.** The project calls itself alpha-quality "technical preview" software. In 2026 it rebuilt its whole repository (over 1,700 recipes), released ISO 2026.08 with Linux 7.1, and moved to a Discourse forum.

## How its atomic updates work

This is different from the A/B approach in ADR-006, and worth understanding:

1. Every file from every package goes into one **content-addressed store** (`/.moss`), indexed by a hash of its contents. Identical files are stored once.
2. A system **state** is a full `/usr` tree built from that store using hardlinks (and, since August 2026, reflinks). Building a new state costs almost no extra disk space.
3. Moving to the new state is **one atomic operation**: the kernel swaps the old and new `/usr` directories in a single step (`renameat2` with exchange). There is no moment where the system is half-updated.
4. **Rollback** is the same swap in reverse, to any earlier state that is still kept.
5. It works **live**, often without a reboot.

It is also **stateless**: the OS ships its defaults in `/usr`, and `/etc` only holds what the administrator actually changed.

## Lessons for our project

| # | Lesson | What we change |
|---|--------|----------------|
| 1 | **Separate OS defaults from local changes.** With defaults in `/usr` and only real changes in `/etc`, rollback is clean and you can see exactly what a machine changed. | Adopt for our base. It also makes `edel system export` (ADR-006) nearly trivial: the system file plus the small `/etc` difference describes the whole machine. |
| 2 | **Design for format changes from day one.** AerynOS had to build "versioned repositories" in 2026 so moss could upgrade itself across breaking on-disk format changes without stranding users ("install once, update forever"). | Version every on-disk and file format we own (deployments, system file, presets) from version 1, and make the `edel` tool migrate them. This is required for the 10-year promise in ADR-005. |
| 3 | **Test fresh installs, not only updates.** In May 2026 a first-boot configuration mistake broke *new* installs while existing systems updating were fine. They fixed it within 24 hours. | Our Reliable rule now requires CI to test both a fresh install and update-plus-rollback on every image. |
| 4 | **Fewer installer images, current content.** They cut their monthly ISO cadence and moved toward a net installer that pulls current packages. | Our installer stays thin and pulls the current base image, so a new installer is only needed for new hardware support. |
| 5 | **The field is converging on "content store plus read-only metadata image."** AerynOS is developing an EROFS metadata image, the same direction as composefs in Fedora's atomic world. It combines deduplication with an OS that can be verified as untampered. | Keep A/B for version 1, but store our images so we could move to this model later without changing what users see. |
| 6 | **Owning the whole toolchain is very expensive.** Years in, with a capable team, AerynOS is still alpha, and they maintain every recipe themselves. | Confirms the soft fork in ADR-003: we own four parts, not 1,700 recipes. |

## Should we use the moss approach instead of A/B?

| | A/B images (ADR-006) | Content store plus atomic `/usr` swap (moss) |
|---|---|---|
| Mental model | Two slots, very easy to reason about | A store plus many states; more to understand |
| Disk use | Twice the base size (small, since the base is small) | Very efficient through deduplication |
| Applying updates | Reboot into the new slot | Often live, no reboot |
| Rollback points | One or two | Many |
| Verifying the OS is untampered | Straightforward (verified images) | Harder until the metadata-image work lands |
| Fit with Alpine | Works with Alpine's current layout | Needs a fully merged `/usr`; Alpine postponed its merge in 3.23 and I couldn't confirm it landed in 3.24 |

**Verdict: stay with A/B for version 1.** AerynOS needs live `/usr` swaps because *everything*, apps included, lives in `/usr` there. In our design, apps live outside the base (Flatpak and containers, ADR-003) and update live anyway, so the base changes rarely and a reboot for a monthly base update is acceptable, as it is on phones, Windows and macOS. A/B is simpler and easier to verify, which fits Reliable and Simple.

Revisit if base updates turn out to be frequent enough that reboots annoy users, or once Alpine's `/usr` merge is complete.

## Could we just build on AerynOS?

Not as our base: it uses systemd and, as far as I know, glibc; it is rolling rather than stable, alpha, and it ships the desktops you've ruled out. But it is worth following closely, and its tools are Rust like ours, so specific pieces (for example blsforme for boot entries) may be reusable. Check licenses before reusing anything.
