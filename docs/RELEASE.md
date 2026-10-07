# Releases

**Date:** 2026-10-02 (roadmap M3.4)

How Edel OS is published, what is kept, and the two steps only Alimardon takes. The reasons are in ADR-003 (releases), ADR-005 (old releases kept for the compatibility promise) and ADR-006 (signed updates).

## What CI publishes

Every run of "Build and boot images" makes a release in `out/release/`: the update images (`*.ext4.gz`) and their `release.toml`, the disks (`*.img.gz`), the container image and the package lists; and `out/channel/release.toml`, the same list naming each image by its URL under a tagged release. `ci/sign.sh` signs both lists in the workflow's `sign` job (M3.7), the only job that is given the release key, and only on a push to `main` or of a tag, in the GitHub environment `release`: it runs on a fresh runner after the build, receives only the two lists, and builds its own `edel` from the locked sources, so nothing the build made (which ran cargo and apk, whose code comes from the network) runs beside the key, and a pull request's build is never signed with the real key. Making the `release` environment wait for Alimardon's approval before each signing is a repository setting (Settings, Environments, release, Required reviewers), which is theirs to switch on. Pull requests and a push to `main` dry-run the preview (`ci/release.sh dry-run`); pull requests and tags also dry-run the tag path and the channel (`ci/channel.sh dry-run`), saying what they would upload.

| Event | What happens once every test passed |
|---|---|
| A merge to `main` | The `preview` pre-release gets this build's files, replacing the last ones, unless the preview there is newer |
| A tag `vVERSION` (numbers joined by dots) | A draft release `vVERSION` with the same files plus `channel.toml`; nothing is public yet |
| Alimardon publishes that draft | `.github/workflows/release.yml` copies `channel.toml` to `channels/stable/release.toml` on the `gh-pages` branch, so the stable channel moves to it |

What is published is what the job built and tested: the publish job takes the files from the tested job, never from a second build. GitHub Releases hold the images (2 GiB per file); GitHub Pages holds only the small channel manifests.

## Kept forever

The files of every tagged release are never deleted, and a published release is never edited except to fix its notes. Old releases are what the compatibility promise is tested against (M3.5 upgrades each one to every new build), and a machine may still be running any of them. `ci/levels.toml` lists the supported releases with their image URLs and platform levels; it is empty until the first tag. Only the preview is replaced, and it promises nothing.

## Keys

Each image trusts the public keys in its `/usr/share/edel/keys/`, from the definition's `[release] public_keys`; an update signed with any other key is refused (M1.6). Every image carries two release keys, so one can replace the other without stranding a machine (FORMATS.md): `edel-release-1` signs, and `edel-release-2` stays offline until it is needed. With the key, `ci/sign.sh` refuses to sign unless `images/keys/` holds both public keys, and the build job's `ci/keys-carried.sh` fails a build whose bootable images do not carry them. Until the release key exists, CI signs with a throwaway key made in each run, and `ci/release.sh publish` refuses to upload a release signed that way.

## What waits for Alimardon

Publishing is off until both steps are done. Each is a repository setting or a secret, which Claude never changes.

1. **Make the two release keys and keep them offline.** On your own computer, in a clone of this repository with Rust installed, run `cargo run --release -- release keygen ~/edel-keys edel-release-1` and the same with `edel-release-2`. The keys go to `~/edel-keys`, outside the clone, so no `git add` can pick them up (`.gitignore` also ignores `*.key`). Each run writes `NAME.key` (the secret, 64 hex digits) and `NAME.pub`. Copy both `.key` files to two places that are not online (a USB stick in a drawer, a password manager's secure note). Send both `.pub` files in a message, or commit them as `images/keys/edel-release-1.pub` and `images/keys/edel-release-2.pub`; Claude then names both in every image definition's `[release] public_keys` in one PR. Once that PR is on `main`, add the contents of `edel-release-1.key` as the repository secret `EDEL_RELEASE_KEY` (Settings, Secrets and variables, Actions, New repository secret); a run that has the secret but not both public keys in every image stops with a message naming this step. `edel-release-2.key` never goes online. Never commit a `.key` file.
2. **Turn publishing on.** Add the repository variable `EDEL_PUBLISH` with the value `yes` (Settings, Secrets and variables, Actions, Variables). The next merge to `main` publishes the first preview. For the stable channel, turn on GitHub Pages from the `gh-pages` branch (Settings, Pages) once the first tag is published.

To stop publishing, delete the variable; nothing already published changes.

**The docs site (M8.10a)** is a third, separate switch: add the repository variable `EDEL_SITE` with the value `yes`, and turn on GitHub Pages from the `gh-pages` branch (Settings, Pages, Deploy from a branch, `gh-pages`, `/ (root)`). From the next merge to `main` on, CI publishes the site to <https://alimardon123.github.io/edel/> next to the update channels, which it never touches. GitHub Pages is free for a public repository. To stop, delete the variable.

## Principles check

- **Reliable:** only tested files are published, every manifest is signed and checked before upload, a throwaway signature can never be published, and old releases stay downloadable for every machine still on them.
- **Simple:** GitHub is the only host, with one workflow job and two short scripts; no mirror, no build farm, no second channel.
- **Efficient:** Pages carries only manifests; the images are uploaded once per release.
- **Traded off:** the preview is replaced on every merge, so a preview machine cannot stay on an older preview; it can stay on a tagged release instead.
