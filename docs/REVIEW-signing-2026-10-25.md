# Review of the updater, the guard and the signing path (2026-10-25)

The one narrow review Alimardon asked for on 2026-10-06, before the first preview is published (roadmap M3.4). Two reviewers worked in parallel, offline and read-only, on the code of `main` at the merge of #456: one on the machine's side (`update.rs`, `guard.rs`, `boot.rs`, `grubenv.rs`, `loader.rs`, the verifying half of `release.rs`, `addons.rs`), one on CI's side (`ci.yml`, `monthly.yml`, `release.yml`, `ci/sign.sh`, `ci/release.sh`, `ci/channel.sh`, the signing half of `release.rs`). The session checked each finding against the code before acting on it.

**Result:** nothing critical. No way around a signature was found, the running slot is never written, and every hash is checked on the bytes that are written. Two findings were high, both about where the release key lives in GitHub, and they need repository settings, not code.

## Checked and sound

- A release list's signature is checked with ed25519's strict check over the exact bytes, before the list is parsed, and the checked bytes are the ones used.
- Older versions, expired lists, another channel and plain http are refused.
- The image is hashed while it streams and again from the disk, with its size and an `e2fsck -fn` check.
- The order off, write, check, on survives a power cut at any point; GRUB reads only the variables it is given.
- `confirm()` and the guard's retry rule are right whenever GRUB's block is readable.
- Add-ons' sizes and hashes are checked before they are renamed into place.
- `ci/sign.sh` writes the key with `umask 077`, removes it on exit, passes it as a path and never prints it; a throwaway signature is never published; pull requests get neither the key nor publishing.
- `monthly.yml` asks for no more permissions than it uses and puts no outside text into a shell line; a fork's pull request named `monthly/...` gets a read-only token, so it tags nothing.

## Fixed in this PR

| Finding | Severity | Fix |
|---|---|---|
| `edel update` could write the other slot while the running one was still on its first, unconfirmed start, destroying the version that last worked | Medium | `update::running_is_settled`: the install waits until the guard has confirmed the running slot, except on a safe start, where an update may be the fix; the message says so and what to do |
| The guard restarted for ever when it could not read GRUB's block (the 2026-10-13 loop, on one remaining path) | Medium | It stands down when `/proc/cmdline` holds `edel.safe`, GRUB's own word that no slot has a try left |
| The sign job signed before the tests ran, and the signed lists stayed downloadable for 90 days | Medium | `sign` waits for Rust checks, the images and the desktop tests; the signed lists are kept one day |
| The sign job used three actions by tag, not by digest | Medium | All three are pinned by digest, as everywhere else |
| The real key signed whatever version and channel the untrusted build wrote (a version 9999 would freeze the preview) | Medium | With the real key, `sign.sh` signs only `stable` and the tag's own version on a tag, else `preview`, and fails naming both otherwise |
| A tag on a commit that never reached `main` got the key | High in part | The sign job refuses a tag whose commit is not on `main`; this catches mistakes, while an attacker who can push could change the check, so the settings below are the real guard |
| `github.ref_name` was written into the publish job's shell line | Low | Passed through `env:` |
| An `https://` download followed a redirect to plain http | Low | `https_only` for https locations; the content stays signed and hashed either way |

## What needs Alimardon (repository settings)

1. **The release key goes into the `release` environment, not the repository.** A repository secret can be read by any workflow run, including one from a branch whose workflow file was changed. `docs/RELEASE.md` now says to add `EDEL_RELEASE_KEY` under Settings, Environments, `release`, Environment secrets.
2. **Only `main` and `v*` tags may use that environment.** In the same page, set Deployment branches and tags to `main` and `v*`. Optionally, add Required reviewers to approve each signing, with administrators not allowed to bypass.
3. **Who may make `v*` tags.** A tag ruleset (Settings, Rules) that lets only you create `v*` tags. The monthly rebuild's tag job makes its tag with the workflow's own token, so the rule must leave that way open (its bypass list; from memory, check the next monthly tag). Without the rule, anyone with write access can tag; today that is only you.

## Left for later steps

| Finding | Severity | Where it belongs |
|---|---|---|
| The sign job runs `cargo build`, so the signer is built beside the key; signing with OpenSSL's ed25519 would need no compiler there | Medium | A step of its own before the release key is added, or accepted: `Cargo.lock` pins every crate by checksum |
| Swapping the boot loader renames files over others on FAT; a power cut there can leave GRUB with no `grub.cfg`, and the `.prev` copies are never used | Low to medium | M1.8's loader swap: write new names first, keep `.prev` as GRUB's fallback |
| If GRUB cannot save its try counter, a broken slot is retried for ever | Low | The guard could treat a try count that never moves as GRUB not counting |
| Taking over a stale update lock can let two writers hold it | Low | `flock` instead of a lock file |
| No timeout while reading a download's body, so a stalled mirror holds the update lock | Low | A read timeout per chunk |
| Add-ons are checked when downloaded, not at each boot's mount, and mounted without `nosuid` | Low | M7.2's add-on mount |
| An old tag can be signed again with a fresh expiry; `channel.sh` moves the stable channel without checking the list's signature; an outsider's issue from the template could pass the checklist gate | Low | The release workflow, before the first public release |
| A signed list with an empty `channel` passes on every channel; one malformed `.pub` blocks every update; the 60-day list with an unauthenticated clock allows a freeze, not a downgrade | Info | Written down; no change |
| `images/edel.toml` names no release keys yet, so released images would refuse every update | Info | By design until the key exists (`images/CLAUDE.md`): the PR that adds the public keys names them in every definition, before the first preview |

## Principles check

- **Reliable:** the two machine-side fixes keep a way back in the two cases where one was lost; every CI fix narrows what the key can sign, and only tested builds are signed.
- **Simple and modular:** each fix is a few lines where the fact already lives; the key's real guard is GitHub's own environment rules, not new code.
- Traded off: signing now starts after the tests, which adds a few minutes to a run's end; an update waits for the guard's confirmation at each start.
