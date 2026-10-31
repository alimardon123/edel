# Proposal: the Undo bar from the keyboard, and fewer places per setting

**Status:** Accepted (Alimardon, 2026-10-25: option B for the Undo bar, A and E for the keys)
**Date:** 2026-10-25
**Deciders:** Alimardon
**Related:** the whole-system review of 2026-10-25 (its shell surfaces finding on the Undo bar and its architecture finding on keys), ADR-008 (keys), ADR-010 (one owner), [shortcuts](SHORTCUTS.md)

Two review findings need a design choice before code, so this page gives the options and a recommendation for each. Alimardon took both recommendations on 2026-10-25; A, B and E are built (B is `undo_desktop_edit` in [shortcuts](SHORTCUTS.md), #508).

## 1. Undo after Done, from the keyboard

After Done in the panel editor or the lock screen's edit mode, the Undo bar shows for 10 seconds. A pointer can press Undo, and since #499 a screen reader can too. A person who uses only the keyboard cannot, because the bar never takes the keyboard. That is on purpose: a person who has just pressed Done goes back to typing in their app, and the bar must not take those keys.

| Option | Verdict |
|---|---|
| A. The bar takes the keyboard when Done was pressed from the keyboard. Return undoes, Escape closes, and any other key closes the bar. | Rejected. The first key a person types after Done is lost, which surprises people. |
| B. A shortcut, Super+Z, "undo the desktop's last change", which works while the bar shows. The bar's button reads "Undo (Super+Z)". | **Recommended.** No key is ever taken from an app. It follows the Super+key pattern of the other shortcuts, and it works for pointer and keyboard users alike. |
| C. Done keeps the editor open, with its own Undo, until the 10 seconds pass. | Rejected. The editor would hold the whole keyboard for 10 more seconds. |
| D. Let shell surfaces take the keyboard when asked (on demand), plus a key that moves the keyboard to the panel. | Later. This is the general answer for every shell surface, and it belongs with the accessibility work, not with this bar. |

**Cost of B:** one shortcut in the compositor, one request on the shell's own protocol to shell-ui, a line in [SHORTCUTS.md](SHORTCUTS.md), and the shortcut's Settings row and settings file line, the same as every other shortcut. It is one PR in the UI lane.

## 2. Fewer places to write for each new setting

Adding a setting key today means writing it by hand in five places:

1. `KEYS` in `crates/edel/src/settings.rs` (path, kind, supported).
2. The settings file struct's field, in the same file.
3. `HELP` in `crates/edel/src/key_help.rs` (help, default, decided by, belongs, retired), keyed by the same string as `KEYS`.
4. `ROWS` in `crates/settings/src/rows.rs` (the row's title), or `NO_ROW` with a reason.
5. `crates/edel/tests/keys.txt`, the append-only record.

Three generated files follow from those (`docs/settings.md`, `docs/defaults.md` and `po/*.pot`), all written by one command, `EDEL_WRITE_DOCS=1 cargo test -p edel`. Tests hold the three tables equal, but they report only after the fact, one gap at a time.

To be fair to the review's numbers (20 and 33 files in two key commits), most of those files were the feature itself: the code that acts on the key, its tests and its page. The tables account for only a few of them. Moving `edel settings apply` to per-feature planners is the larger lever, and it is the review's next finding.

| Option | Verdict |
|---|---|
| A. Fold `HELP`'s fields into `Key`, so there is one entry per key in `KEYS`. `key_help` then reads them from there, and the generated docs stay as they are. | **Recommended.** Places 1 and 3 become one, and the test that holds them equal goes away. It is a mechanical move of about 1,400 lines, with no change to the file format, the base or the updater. |
| B. Fold Settings' row titles into `Key` too. | Rejected. Rows are not keys: there are 154 rows over 122 keys, plus keys with no row. The titles are Settings' own words, so ADR-010 says Settings owns them. |
| C. Write every key in one TOML file, and generate `KEYS`, `HELP` and the struct with a build script. | Rejected. It adds a new format and a build step, and it is harder to read and to debug. |
| D. A macro that writes the struct's field from the table. | Not now. It would save one line per key, at the cost of macro code few people can read. Revisit if the two ever drift. |
| E. One test, "a new key is complete". It takes the last line of `keys.txt` and lists in a single message every place that key still lacks (entry, field, row or `NO_ROW`, generated docs). | **Recommended, with A.** It is cheap, and it tells a person or a coding agent everything at once instead of one failure per run. |

**After A and E:** a new key is written in four places (its `KEYS` entry, its field, its Settings row and its `keys.txt` line), and one test, `every_key_is_complete` in `crates/edel/src/settings.rs`, names in one message every place each key still lacks; Settings' own test names a missing row. The generated files still come from one command.

**Cost of A and E:** two PRs in the UI lane, A then E, each checked by the existing key tests, `ci/keys-check.sh` and `ci/one-place.sh`.

## Principles check

- **Reliable:** the Undo shortcut never takes a key from an app, so nothing a person types is lost.
- **Simple and modular:** each key's facts sit in one entry (ADR-010), and Settings keeps its own words. There is no new format, macro or build step.
- **Functional:** Undo after Done works with a pointer, a screen reader and the keyboard.
- **Traded off:** one more shortcut to learn (B), and the struct field still repeats the key's name once (D is not taken).
