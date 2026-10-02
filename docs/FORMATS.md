# File formats

**Date:** 2026-10-02

Every file format Edel OS owns has a top-level integer `format`, from 1. Writers and checkers are strict; readers on a machine are lenient, because an old slot reads files a newer release wrote and cannot be patched afterwards (ADR-008). This page grows with each format: the system file arrives in M2.1, the slot size rule in M3.3.

## `release.toml` (format 1, M1.6)

The manifest of one release, published next to its images, with `release.toml.sig` beside it.

```toml
format = 1
version = "2026.10.1"
channel = "stable"
date = "2026-10-02T18:00:00Z"

[[images]]
name = "edel-vm-x86_64"        # matches EDEL_IMAGE in the image's os-release
file = "edel-vm-x86_64.ext4.gz"  # beside release.toml; .gz is decompressed on the way
sha256 = "..."                     # of the image as it lands in the slot
size = 187695104
```

- **Signature:** `release.toml.sig` holds an ed25519 signature over the exact bytes of `release.toml`, as hex. Public keys are 32 bytes as hex in `*.pub` files; every image carries two of them in `/usr/share/edel/keys/`, so one key can replace the other without stranding a machine. `edel release keygen`, `make`, `sign` and `verify` handle them.
- **Where from:** `edel update install` and `edel update check` take a path, a `file://` URL or an http(s) URL; the signature makes plain http as safe as https. The image streams into the slot and is hashed on the way (M1.7).
- **Images are shrunk:** `edel image build` cuts the update image to its file system (`resize2fs -M`), so it fits any slot at least that big, and the install grows it to fill the slot. `size` is that shrunk size, so it is also the smallest slot the image needs.
- **What `edel update install` refuses:** a manifest no carried key signed (`refused: signature`), an image whose sha256 or size differs (`refused: sha256`), a release with no image named like this machine's `EDEL_IMAGE`, and a version that is not newer than the running `VERSION_ID` unless `--allow-downgrade` is given. Versions compare number by number (`2026.10.2` is newer than `2026.9.9`).
- **Reading:** unknown fields are ignored; the signature still covers them. Only an unknown `format` is refused, naming the format.
- **The stepping-stone rule:** a breaking change publishes the new format under a new file name and keeps `release.toml` pointing at a release whose `edel` reads both formats. A machine two formats behind installs that stepping stone, then the newer release at its next check, with no extra command from the person (AerynOS users rerun theirs by hand).
- **Keys today:** until the first preview (M3.4) no image carries a real key. CI makes two throwaway keys on every run and bakes them into its test images with `edel image build --public-key`. The real key goes into the `EDEL_RELEASE_KEY` secret, which waits for Alimardon.
