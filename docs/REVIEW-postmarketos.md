# Review: what we can learn from postmarketOS and Duranium

**Date:** 2026-10-02
**Reviewed:** [pmaports](https://gitlab.postmarketos.org/postmarketOS/pmaports), [postmarketos-mkinitfs](https://gitlab.postmarketos.org/postmarketOS/postmarketos-mkinitfs), [boot-deploy](https://gitlab.postmarketos.org/postmarketOS/boot-deploy) and [duranium-build](https://gitlab.postmarketos.org/postmarketOS/duranium-build) as of 2026-10-02, the [docs](https://docs.postmarketos.org/), the wiki, and the blog from [introducing Duranium](https://postmarketos.org/blog/2026/03/17/introducing-duranium/) to the [rename to Nura](https://postmarketos.org/blog/2026/09/27/nura-rename/).

## What postmarketOS is

A distribution for phones and other devices, built as an overlay of its own packages on Alpine. It was renamed **Nura** on 2026-09-27; postmarketos.org redirects to nura.eco, while the repositories and package names still say postmarketOS. It matters to us twice: it has the soft-fork shape of ADR-003, and its device packages are where our phone image comes from (M9.7).

- **Overlay on Alpine:** about 670 active packages; musl, apk, busybox and OpenRC come from Alpine unchanged. A forked Alpine package gets a `9999` version prefix and a versioned `provides`, so apk always prefers it.
- **Releases:** every six months, each on one Alpine stable branch, supported for about seven months (v26.06 on Alpine 3.24 ends 2027-01-31).
- **Devices:** the top tier needs five maintainers and hardware CI in two places and holds no device; 39 devices are in community, 151 in testing, 458 archived.
- **Boot:** an initramfs generator, a separate shell init, and boot-deploy, which writes boot files for about a dozen boot methods into one `/boot`, in place. Mainline has no A/B, boot counting or rollback.
- **Two init systems** since v25.06: a second repository of 36 packages, about 20 of them Alpine forks, and switching between OpenRC and systemd means reinstalling.

## What Duranium is

postmarketOS's image-based variant, for testers since 2026-03-17: weekly edge images for x86_64, Arm laptops, Fairphone 5 and 6, Pixel 3a and OnePlus 6 and 6T, no stable release yet.

- **All systemd:** mkosi builds from Alpine packages, systemd-repart partitions at first boot, systemd-sysupdate updates, systemd-boot counts tries in the kernel image's file name, the initramfs runs systemd.
- **Layout:** two read-only 5 GiB erofs `/usr` slots with dm-verity, one LUKS2 root for `/etc` changes, `/var` and `/home`. A GNOME update is about 2.4 GB.
- **`/etc` is an overlay:** the image's defaults below, the machine's changes above, the same as our M1.4.
- **Add-ons** are built from the same package snapshot as the base and refused on any other base version. Alpine's triggers write combined files (icon caches, compiled GSettings schemas) that cannot be stacked across layers.
- **Signing:** one GPG key over a SHA256SUMS file ("this key is not secure", says the build); the kernel image is unsigned, so verity catches corruption, not tampering.
- **Encryption** is always on with an empty passphrase until the first-boot wizard sets one; unl0kr, an on-screen keyboard, asks for it before the system starts.
- **Phones** boot through U-Boot acting as UEFI firmware: the whole GPT disk is flashed inside Android's `userdata` and U-Boot reads it with `blkmap`. A GPT name longer than 24 characters broke fastboot on the OnePlus 6.
- **Factory reset** is requested from a logged-in session, never from the boot menu, where anyone or an automatic choice could wipe the device.

## Lessons for our project

| # | Lesson | Where it goes |
|---|--------|---------------|
| 1 | An `/etc` overlay hides system users a later slot adds; Duranium repairs the user list every boot | M1.4 |
| 2 | Machine identity (hostname, machine-id, password backups) never lives in the slot | M1.4, M2.4 |
| 3 | One package snapshot per CI run, so images of one release cannot differ | M3.1 |
| 4 | Two add-ons must not ship the same file; combined trigger files cannot be stacked | M7.2a |
| 5 | U-Boot as UEFI boots a GPT inside `userdata` on the OnePlus 6 (SDM845), so GRUB for arm64 may keep our try counter | M9.7 |
| 6 | A touch device needs a touch passphrase prompt (unl0kr is in postmarketOS's repository, not in Alpine 3.24) | M8.2, M9 |
| 7 | Required kernel options live in a small TOML file with a reason each, checked in CI | M3.3a |
| 8 | Forks get a `9999` version, versioned `provides`, a "Forked from Alpine to ..." header and a CI test; missing packages go to Alpine first | the first step that creates `packages/` |
| 9 | Fixes reach a release branch with `git cherry-pick -x`, never squashed | M8.5 |

## What to avoid

- systemd's image tools (ruled out anyway) and two init systems.
- Writing boot files in place with no rollback (boot-deploy).
- An initramfs that waits forever on failure. Alpine 3.24's own initramfs (mkinitfs 3.14.1-r0) panics instead of opening its recovery shell when `panic=` is on the command line, which our `grub.cfg` always passes, so the machine restarts and GRUB counts the try. Keep `panic=`.
- Update state in partition names, encryption that cannot be turned off, a long device list, seven-month releases, one signing key that cannot be replaced, and a factory reset in the boot menu.

**Verdict: adopt nothing wholesale; Duranium confirms our design.** It reached the same overlay `/etc`, version-locked add-ons and full-image updates on its own, while our boot guard and two-key signing go further. Its scars and its phone boot path are what we take.

## Sources

The repositories above, cloned on 2026-10-02, and the Alpine v3.24 package index and packages fetched the same day (`merge-usr`, `buffyboard` and `cloud-utils-growpart` are in it; `unl0kr` is not). Research by three agents, each claim checked by a second agent against the source. How systemd-boot counts tries comes from general knowledge.

## Principles check

- **Reliable** drove lessons 1, 2, 5 and the initramfs check: a machine must come back on its own.
- **Simple** rejected the systemd tools, a second init system and boot-deploy.
- **Efficient** rejected encryption by default on old laptops and 5 GiB erofs slots without a measurement.
- **Traded off:** touch unlock and phone boot stay unproven until M9 hardware.
