# File formats

**Date:** 2026-10-02

Every file format Edel OS owns has a top-level integer `format`, from 1. Writers and checkers are strict; readers on a machine are lenient, because an old slot reads files a newer release wrote and cannot be patched afterwards (ADR-008). This page grows with each format; the disk layout, which no format number can change on a machine already installed, is at the end.

## `release.toml` (format 1, M1.6)

The manifest of one release, published next to its images, with `release.toml.sig` beside it.

```toml
format = 1
version = "2026.10.1"
channel = "stable"
date = "2026-10-02T18:00:00Z"
expires = "2026-12-01"           # the last day it may be used, set when signed (M3.8)

[[images]]
name = "edel-vm-x86_64"        # matches EDEL_IMAGE in the image's os-release
file = "edel-vm-x86_64.ext4.gz"  # beside release.toml; .gz is decompressed on the way
sha256 = "..."                     # of the image as it lands in the slot
size = 187695104
```

- **Signature:** `release.toml.sig` holds an ed25519 signature over the exact bytes of `release.toml`, as hex. Public keys are 32 bytes as hex in `*.pub` files; every image carries two of them in `/usr/share/edel/keys/`, so one key can replace the other without stranding a machine. `edel release keygen`, `make`, `sign` and `verify` handle them.
- **Where from:** `edel update` and `edel update --check` take a path, a `file://` URL or an http(s) URL; the signature makes plain http as safe as https. The image streams into the slot and is hashed on the way (M1.7).
- **Images are shrunk:** `edel image build` cuts the update image to its file system (`resize2fs -M`), so it fits any slot at least that big, and the install grows it to fill the slot. `size` is that shrunk size, so it is also the smallest slot the image needs.
- **What `edel update` refuses:** a list past its `expires` date (`refused: expired`), a list for another channel than the machine's (`refused: channel`; the machine follows `updates.channel`, else its image's own channel, and `--channel NAME` takes another once), a plain `http://` location without `--allow-http`, which only CI's local server needs (`refused: http`), a manifest no carried key signed (`refused: signature`), an image whose sha256 or size differs (`refused: sha256`), a release with no image named like this machine's `EDEL_IMAGE`, and a version that is not newer than the running `VERSION_ID` unless `--allow-downgrade` is given. Versions compare number by number (`2026.10.2` is newer than `2026.9.9`).
- **Expiry (M3.8):** `edel release sign --expires-in DAYS` writes `expires` before it signs, so the date is the signer's; released lists get 60 days (a default row). A list from before M3.8 has no `expires` and is still read, as older updaters ignore the field, so this was no format bump.
- **Reading:** unknown fields are ignored; the signature still covers them. Only an unknown `format` is refused, naming the format.
- **The stepping-stone rule:** a breaking change publishes the new format under a new file name and keeps `release.toml` pointing at a release whose `edel` reads both formats. A machine two formats behind installs that stepping stone, then the newer release at its next check, with no extra command from the person (AerynOS users rerun theirs by hand).
- **Keys today:** until the first preview (M3.4) no image carries a real key. CI makes two throwaway keys on every run and bakes them into its test images with `edel image build --public-key`. The real key goes into the `EDEL_RELEASE_KEY` secret, which waits for Alimardon.

## `settings.toml` (format 1, M2.1)

The file that describes a whole machine, `/data/edel/settings.toml`; [settings.md](settings.md) lists every key. `edel::settings` in `crates/edel` is its one parser, for `edel` and for every later part.

| Reader | Unknown key | Value of the wrong kind or an unknown value | Newer `format` |
|---|---|---|---|
| `edel settings check`, the installer, CI | Refused, naming the key | Refused, listing the allowed values | Refused |
| On a machine: boot apply, compositor, shell-ui, Settings | Left out and reported | Left out, so the default is used, and reported | `settings.toml.v<N>` beside it is read for the reader's own format N; without it nothing is applied, and the reader says so |

- **Absent means default:** every key is optional, and a file holds only what a person chose (ADR-008). A file with no `format` is refused.
- **Keys not acted on yet** are parsed, so adding them later is not a format bump. `edel settings check` refuses them with "not supported yet"; the boot apply skips them and says so.
- **A key is never removed or renamed within a format.** `crates/edel/tests/keys.txt` lists every key, append only; a cargo test fails when the key table lacks one, and `ci/keys-check.sh` fails a change that deletes a line without bumping `FORMAT`. A dropped key stays parsed and is reported as no longer used; an unavoidable rename reads both names.
- **Names this release lacks** (a preset, profile, add-on or feature) are values like any other: the checker refuses them, a reader on a machine uses the default and reports it, and boot never fails over one.
- **Writers** (`edel settings set` and `reset` since M2.3, Settings later) edit the file in place with `toml_edit`, so comments, order and keys this slot does not know survive.

## Feature files (format 1) and image definitions (format 2), M4.0

`features/NAME.toml` and `images/*.toml` are described in [FEATURES.md](FEATURES.md). Both live in this repository, so a format bump converts every file in the same PR, and `edel image build` and `image check` refuse an unknown field or format. Feature files also ship in every image as `/usr/share/edel/features/NAME.toml`, and readers on a machine read those leniently: an unknown field is skipped and noted (`edel report` lists the notes), and a newer `format` is read for what it holds and noted.

## `loader.toml` (format 1, M1.8)

`format = 1` and `version`, a hash of the GRUB binary and `grub.cfg`. A `loader.toml` in another format counts as no version: the slot's loader is then not installed, and the partition's is replaced. Each slot carries one in `/usr/lib/edel/boot/` beside its loader, and the EFI system partition keeps the installed one in `/EFI/edel/`. Once a slot is confirmed, `edel boot mark-good` installs that slot's loader when the versions differ.

## The disk layout (M1.2, slot size M3.3b)

A machine's disk is GPT: partition 1 is the EFI system partition (64 MiB, label `EDEL-ESP`), 2 and 3 are slot A and slot B, and 4 is the data partition (label `edel-data`), which takes the rest. An update can replace everything inside a slot, but never the layout, so it is decided once:

- Every slot of an installed machine is 4096 MiB (`edel::install::SLOT_MIB`), whatever it was installed from, and the VM image's slots are too, because a VM runs from that disk as it is. A machine installed from the first preview must be able to take every later update.
- An update image is the slot's file system shrunk to its contents (`resize2fs -M`); `edel update` writes it into the other slot and grows it to fill the slot. So a release may grow until its files fill 4096 MiB, and no release may need more.
- The desktop image, a stick to try Edel OS and install it from, keeps 1536 MiB slots so it fits a common 8 GB stick (about 7.45 GiB); `edel install` copies its running slot into a 4096 MiB slot and grows it. (The laptop image, a text console with 1024 MiB slots, did this until 2026-10-06.)
- Changing the slot size later means moving every installed machine's data partition, so it waits for a format change of the disk itself, with a migration in `edel update`.
