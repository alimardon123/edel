# ADR-010: Every part can be changed, dropped or stand on its own

**Status:** Proposed
**Date:** 2026-10-06
**Deciders:** Alimardon
**Extends:** ADR-002 (its separable desktop decision becomes the rule for every part; one line amended here)
**Related:** ADR-007 (add-ons), ADR-008 (features, keys and presets as the ways to change things), [ADR-009](ADR-009-enterprise-the-same-system-at-work.md) (enterprise pieces added later), [design principles](DESIGN-PRINCIPLES.md), [roadmap](ROADMAP.md)

## Context

On 2026-10-06 Alimardon asked that "overall our code, our system ... the architecture ... everything is flexible and modular to be changed, easy to be changed later. So that in the future we can change, add, modify, drop and do some extra things without a headache." They added: "if some of the things that we are building is really good and can be used for other projects, then if it's modular, then that means it can those can be a separate project that can be run on its own", recalled that they had asked the same of the desktop, and asked that the "same philosophy, same goal should be applied to any other places that we are building".

Much of this exists. ADR-008 made features, presets and settings things we add, swap or drop as files. ADR-002's separable desktop decision (2026-10-04) keeps the compositor and shell-ui able to ship on their own, and M5.15 checks it. But the rule covers only the desktop, and nothing checks it yet for the other parts we write: the updater and boot guard, the image builder, the system file library, the design tokens and the CI harness. ADR-009 also leaves doors open for later (a long channel, a systemd adapter add-on, enterprise security), and those doors are only cheap if the parts stay apart.

## Decision

### 1. The parts, and what each could become

| Part | Where it lives | Could stand alone as | Its seam |
|---|---|---|---|
| The desktop: compositor, shell-ui, design tokens and icons | `crates/compositor`, `crates/shell-ui`, `design/` | A desktop for other distributions (M5.15) | Wayland and D-Bus protocols; the files `edel::places` names |
| Settings | `crates/settings` (M5.6) | The desktop's settings app | The system file, through `edel::system` |
| The system file library: `edel::system`, `features`, `presets`, `shortcuts`, `tokens` | `crates/edel` (library target) | A library for one-file machine descriptions | TOML files with a format number |
| The updater and the boot guard: `edel update`, `rollback`, `status`, `boot guard`, the GRUB environment | `crates/edel` | A/B updates for other small distributions and devices | GPT partition labels, GRUB's environment block with RAUC's names, `release.toml` |
| The image builder: `edel image build` and feature files | `crates/edel`, `features/`, `images/` | Images from feature files on Alpine | Feature and image TOML files, apk |
| The test harness: `ci/qmp.py`, `desktop-test`, `edel-testclient` | `ci/`, `crates/testclient` | Testing a Wayland desktop in QEMU | QMP, the session's state file |

### 2. The rules every part keeps

1. **Parts meet only through plain files and standard protocols.** Each file has a format number (TOML); the protocols are Wayland, D-Bus and the portals. No part reaches into another part's code or internal state.
2. **Edel OS's places are named in one module.** A part reads or writes Edel OS's paths only through `edel::places`, which falls back to the standard places (the XDG base directories) where Edel OS's own are absent. M5.15a does this for the desktop, and M8.13 for the other parts.
3. **Dependencies go one way.** The library never depends on the binaries; the compositor and shell-ui never depend on each other; nothing depends on the test harness; the desktop uses the `edel` library without its command-line feature (no network or signing code).
4. **Each part is tested on its own**, and its CLAUDE.md says in one line what it needs from outside ("Stands alone: ...").
5. **New things come in through doors that already exist:**
   - a feature or an add-on for the system (ADR-007, ADR-008);
   - a key for a setting, added within its format (ADR-008);
   - a preset for a layout, and a token for a look;
   - a module behind an interface we already have: a window policy, a panel widget, a Settings page, a tiling style;
   - a new format version for a file, with its stepping stone (ADR-008).

   Never a plugin API or runtime-loaded code (ADR-002, ADR-008).
6. **Dropping something is deleting its file, its module and its line.** Readers report a name that is gone and carry on (ADR-008).

### 3. Becoming a separate project waits for Alimardon

Code stays in this one repository with one CI until a part has a user outside Edel OS (Simple). Then it gets its own name, package or repository, on Alimardon's word, since new repositories, packages and names wait for them. A part is ready to leave when CI shows it builds and runs without the rest: M5.15a for the desktop, M8.13 for the others.

### 4. Checked, not remembered

CI checks the seams (M8.13): the dependency rules above with `cargo tree`, no Edel OS path outside `edel::places`, and each library building on its own. A PR that adds a part, a file format or a dependency names the seam it uses in its Principles check.

## Options Considered

| Option | Verdict |
|---|---|
| A. The rules above, checked in CI, in one repository | **Recommended**: modular now, with no new parts and no extra releases to run |
| B. Split each part into its own repository now | Rejected: more CI, versions and releases for one person (Simple); done later, part by part, when a part has a user outside Edel OS |
| C. A plugin or module system loaded at run time | Rejected (ADR-002, ADR-008): a fifth part in all but name, and a security and compatibility surface |
| D. Leave it to good habits | Rejected: the desktop's rule needed a CI check (M5.15a) to stay true, and so will the others |

Not built, and defended: a plugin API, runtime-loaded modules, a component registry, a separate repository per crate before anyone outside uses one.

## Consequences

- **Easier:**
  - lifting a part out for another project;
  - replacing a part, such as another updater or another compositor, by touching one seam;
  - enterprise pieces (ADR-009) arriving later as add-ons and features;
  - a maintainer finding where a change belongs.
- **Harder:** a few paths move behind `edel::places`; each PR keeps a little discipline about its seams.
- **Revisit:** when a part gets a user outside Edel OS (its own repository and package), and when a seam check proves too strict for a real need.
- **Amends ADR-002:** the separable desktop decision becomes the first case of this rule.
- **Cost:** one CI script, one module grown and one line in each crate's CLAUDE.md (M8.13); M5.15a already planned the rest for the desktop.

## Action Items

1. [ ] `edel::places` for the desktop (M5.15a), then for the updater, the image builder and the library (M8.13).
2. [ ] `ci/seams.sh` in the Rust checks job (M8.13).
3. [ ] A "Stands alone" line in each crate's CLAUDE.md (M8.13).
4. [x] The fewest-parts map in DESIGN-PRINCIPLES.md shows what each part could stand alone as (done 2026-10-06 in this ADR's PR).

## Principles check

- **Reliable:** a seam that CI checks keeps a change in one part from breaking another.
- **Simple:** one repository, the doors we already have, no plugin API; a part's whole boundary is files and protocols.
- **Efficient:** places are found once at start, and there is no run-time layer between parts.
- **Scalable:** parts can serve other projects and devices, and enterprise pieces arrive as add-ons.
- **Versatile:** things change by swapping or adding a part through a door that exists, not by adding options.
- **Traded off:** Simple pays a little ceremony in each PR and one more CI check.
