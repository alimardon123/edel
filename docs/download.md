# Download

Every Edel OS release is a GitHub release of this repository. [Try it](TRY-IT.md) says what each file is and how to start it on a USB stick, VirtualBox or QEMU.

| Release | Its page | Its signed list |
|---|---|---|
| The newest stable release | [Latest release](https://github.com/alimardon123/edel/releases/latest) | [`release.toml`](https://github.com/alimardon123/edel/releases/latest/download/release.toml) and [its signature](https://github.com/alimardon123/edel/releases/latest/download/release.toml.sig) |
| The preview, replaced by every merge to `main`, for testing only | [Preview](https://github.com/alimardon123/edel/releases/tag/preview) | [`release.toml`](https://github.com/alimardon123/edel/releases/download/preview/release.toml) and [its signature](https://github.com/alimardon123/edel/releases/download/preview/release.toml.sig) |
| Every release ever made, kept for good | [All releases](https://github.com/alimardon123/edel/releases) | On each release's page |

Until the first preview is published (roadmap M3.4), these pages are empty, and the newest images come from CI, as [Try it](TRY-IT.md#get-the-images) says.

## NVIDIA graphics

NVIDIA cards run on the open driver every Edel OS image carries (the kernel's nouveau and Mesa's NVK), not on NVIDIA's own driver, which is built for another C library than ours. Cards from the GeForce GTX 16 and RTX 20 series on run at full speed with it. Older cards (GTX 10 series and before) start and show the desktop, but the open driver cannot raise their clock, so games run slowly on them. Intel and AMD graphics need nothing extra.

## Check what you downloaded

Each release's `release.toml` lists every file of the release with its sha256 sum: the update images under `[[images]]`, and the disks, the container image and the package lists under `[[downloads]]`, each by its bytes as downloaded. `release.toml.sig` is the list's signature, made with the release key in CI ([Releases](RELEASE.md)), so one signature covers every file.

1. **The list.** With `edel` on any Linux machine, `edel release verify --keys KEYS release.toml` checks `release.toml.sig` against the public keys in `KEYS`, a folder holding the `.pub` files of `images/keys/` in this repository, and says which key signed it or why it refuses it.
2. **The file.** `sha256sum edel-x86_64.img.gz` (on Windows, `certutil -hashfile edel-x86_64.img.gz SHA256`) prints the file's sum; it must be the `sha256` of its `[[downloads]]` entry in that list.

A list signed with CI's throwaway key, which pull requests use, is never published, so a release's signature is always the release key's.

## Security fixes

The monthly rebuild makes a new release from Alpine's fixes on the first of each month ([Security fixes](SECURITY.md)), and [Backports](backports.md) lists the base packages whose fixes we carry ourselves.
