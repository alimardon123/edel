# Releases

**Date:** 2026-10-02 (roadmap M3.4)

How Edel OS is published, what is kept, and the two steps only Alimardon takes. The reasons are in ADR-003 (releases), ADR-005 (old releases kept for the compatibility promise) and ADR-006 (signed updates).

## What CI publishes

Every run of "Build and boot images" makes a release in `out/release/`: the update images (`*.ext4.gz`) and their `release.toml`, signed, the disks (`*.img.gz`), the container image and the package lists; and `out/channel/release.toml`, the same list naming each image by its URL under a tagged release. On a pull request it only checks them (`ci/release.sh dry-run`, `ci/channel.sh dry-run`) and says what it would upload.

| Event | What happens once every test passed |
|---|---|
| A merge to `main` | The `preview` pre-release gets this build's files, replacing the last ones, unless the preview there is newer |
| A tag `vVERSION` (numbers joined by dots) | A draft release `vVERSION` with the same files plus `channel.toml`; nothing is public yet |
| Alimardon publishes that draft | `.github/workflows/release.yml` copies `channel.toml` to `channels/stable/release.toml` on the `gh-pages` branch, so the stable channel moves to it |

What is published is what the job built and tested: the publish job takes the files from the tested job, never from a second build. GitHub Releases hold the images (2 GiB per file); GitHub Pages holds only the small channel manifests.

## Kept forever

The files of every tagged release are never deleted, and a published release is never edited except to fix its notes. Old releases are what the compatibility promise is tested against (M3.5 upgrades each one to every new build), and a machine may still be running any of them. `ci/levels.toml` lists the supported releases with their image URLs and platform levels; it is empty until the first tag. Only the preview is replaced, and it promises nothing.

## Keys

Each image trusts the public keys in its `/usr/share/edel/keys/`, from the definition's `[release] public_keys`; an update signed with any other key is refused (M1.6). Until the release key exists, CI signs with a throwaway key made in each run, and `ci/release.sh publish` refuses to upload a release signed that way.

## What waits for Alimardon

Publishing is off until both steps are done. Each is a repository setting or a secret, which Claude never changes.

1. **Make the release key and keep its backup offline.** On your own computer, in a clone of this repository with Rust installed, run `cargo run --release -- release keygen keys edel-release`. It writes `keys/edel-release.key` (the secret, 64 hex digits) and `keys/edel-release.pub`. Copy the `.key` file to two places that are not online (a USB stick in a drawer, a password manager's secure note), then add its contents as the repository secret `EDEL_RELEASE_KEY` (Settings, Secrets and variables, Actions, New repository secret). Send the `.pub` file in a message or commit it as `images/keys/edel-release.pub`; Claude then names it in every image definition's `[release] public_keys`. Never commit the `.key` file.
2. **Turn publishing on.** Add the repository variable `EDEL_PUBLISH` with the value `yes` (Settings, Secrets and variables, Actions, Variables). The next merge to `main` publishes the first preview. For the stable channel, turn on GitHub Pages from the `gh-pages` branch (Settings, Pages) once the first tag is published.

To stop publishing, delete the variable; nothing already published changes.

## Principles check

- **Reliable:** only tested files are published, every manifest is signed and checked before upload, a throwaway signature can never be published, and old releases stay downloadable for every machine still on them.
- **Simple:** GitHub is the only host, with one workflow job and two short scripts; no mirror, no build farm, no second channel.
- **Efficient:** Pages carries only manifests; the images are uploaded once per release.
- **Traded off:** the preview is replaced on every merge, so a preview machine cannot stay on an older preview; it can stay on a tagged release instead.
