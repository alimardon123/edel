# Working in docs/

Read [README.md](README.md) (the index) and [DESIGN-PRINCIPLES.md](DESIGN-PRINCIPLES.md) first. Copy the shape of [ADR-008](ADR-008-features-defaults-and-install.md) for a new ADR: it is the newest and most complete. ADR-001 to ADR-003 use an older first-person advisory voice ("What I would cut or defer"); do not copy it. The root CLAUDE.md style rules apply here, including its dash check before committing.

## Every write-up

- Ends with a **Principles check** (defined in the root CLAUDE.md). In an ADR it is a `## Principles check` section, either one paragraph naming the principles (ADR-007) or one bullet per principle in rank order, name in bold, then a "**Traded off:**" bullet (ADR-008). Roadmap milestones use an inline "**Principles check:**" paragraph. ADR-001 to ADR-006 predate the rule; ticking an item or fixing a line in them does not need one.
- When Alimardon's wish drives a doc, say in Context when they asked and quote their words ("On 2026-10-02 they added that ...").
- Cite decisions as "(ADR-006)" at the end of the clause and steps by id ("M5.12", "M7.2a"); link docs with relative links (`[roadmap](ROADMAP.md)`).
- Name a fact by linking to its owner, or generate the text from it (ADR-010's one owner decision): prose says "the settings file", the tokens, the budgets, and never copies their values, so a change at the owner leaves no doc wrong.
- Write for the docs site as well as for GitHub (M8.10a): link pages by relative path, never with a link that leaves `docs/` (the site holds only `docs/`; link to GitHub's copy for code), and keep `docs/index.md`, the front page, and `docs/guide/how-it-works.md` true when a step changes what they describe. `docs/guide/commands.md` and `docs/theme/tokens.css` are written by cargo tests; never edit them by hand.
- Hedge what you have not checked ("as far as I know"). A `## Sources` section, when there is one, goes just before `## Principles check` and ends with a line saying which claims come from general knowledge.

## ADRs

- File name `ADR-NNN-kebab-slug.md`; the next free number is 012. Title line `# ADR-NNN: Sentence-case title`.
- Header, one bold label per line under the title:

  ```
  **Status:** Proposed
  **Date:** 2026-10-02
  **Deciders:** Alimardon
  **Related:** ADR-006 (settings file), [design principles](DESIGN-PRINCIPLES.md), [roadmap](ROADMAP.md)
  ```

  Use `**Supersedes:**` or `**Extends:**`, with a parenthetical saying what, instead of Related when that is the relation.
- Status: give a new ADR `Proposed`. ADR-002 to ADR-008 all say Proposed, even where Alimardon decided parts; ADR-001 says "Superseded in part by ADR-002 (shell) and ADR-003 (base and releases)". None says Accepted. Change a Status only on Alimardon's own words.
- Sections, in order: `## Context`, `## Decision` (numbered `### 1. ...` subsections), `## Options Considered`, `## Consequences`, `## Action Items`, `## Principles check`.
  - Options Considered: a `| Option | Verdict |` table with lettered options ("A. ..."); the chosen verdict starts "**Recommended**", the others say why they were rejected. Name what is deliberately not built.
  - Consequences: bullets led by "**Easier:**", "**Harder:**" and "**Revisit:**", plus "**Cost:**" (lines, PRs) when useful.
  - Action Items: `1. [ ] ...`, citing the roadmap step in parentheses, "(M2.1)". Tick `[x]` with a dated reason: "2. [x] Choose the updater: our own `edel update` (decided 2026-10-01; RAUC is not packaged)." A roadmap step that completes an item ticks it in the step's PR.
- An ADR that creates work adds or edits the roadmap steps its Action Items cite, with a log entry, in the same PR.
- A decision taken after the ADR was written goes into Decision under a dated bold label, "**Updater decision (2026-10-01):** ...", and its action item is ticked.
- Amending an ADR: edit the text in place and extend the Date line, `**Date:** 2026-10-01, amended 2026-10-02 (what changed)`; Context says what Alimardon added and when.
- A newer ADR that changes a line of an older one: edit the older line in place ending "(ADR-NNN)" and mark the older Date line amended; in the newer ADR, name the older one in Related ("ADR-002 (presets, no extension API; one line amended here)") and add "**Amends ADR-NNN:** ... becomes ..." to Consequences.
- A review of another project is `REVIEW-<name>.md` with `**Date:**` and `**Reviewed:**` (the sources, as links), and a bold verdict line ("**Verdict: stay with A/B for version 1.**").

## Roadmap mechanics (ROADMAP.md)

- A step is a task item, then an indented line with the check and the notes:

  ```
  - [ ] **N. Title.** What the PR does, naming the files.
    Done when: the check CI runs. Notes: ...
  ```

  Elsewhere it is cited as M<milestone>.<step>: M1.1, M6.6a.
- **Done when** always names a check CI runs: cargo tests, a QEMU boot with a line on the serial console, a screenshot pixel check, a container run. Never a manual check.
- Tick a step (`- [ ]` to `- [x]`) in the step's own PR, so the tick reaches `main` only if the PR merges green. The roadmap says "merged when CI is green, then the box is ticked"; ticking in the PR is how that is done without a direct push to `main`.
- A step that is wrong or too big is changed in place: split, reorder, or rewrite its "Done when". Never renumber, because other docs cite the numbers. A new step before step 1 takes 0 (M4.0, "Numbered 0 so later references keep their numbers"); a split or an insert elsewhere takes letters (M6.6a and M6.6b; M7.2 renamed M7.2a beside a new M7.2b). Change every reference to a renamed step in the same PR.
- Every plan change gets an entry at the top of "Changes to the plan" (newest first): `- YYYY-MM-DD: what changed, why. Asked by Alimardon. PR #N.` Leave out "Asked by Alimardon." when it was Claude's call. Every split, reorder, rewritten "Done when" and overruled default gets a line. A finished step gets no line (since 2026-10-07): its tick and its "Done with:" note, added to its "Done when" line, record what it built, and a PR that only finishes a step leaves the log alone, so PRs side by side do not conflict there.
- A step that takes an idea from another system (HarmonyOS, Apple, Android, Windows, KDE, GNOME, COSMIC, Hyprland, niri and the rest) names it and says in a "Beyond ...:" sentence how ours goes further for the person (Alimardon, 2026-10-07): fewer steps, one settings file for every machine, privacy, speed or looks, within the principles, never by adding options or parts.
- Open question: do not wait. Take the default in "Decisions taken by default", or pick one by the principles and add a row to its `| Question | Default | Why |` table, naming the principle ("Reliable over Efficient"). When Alimardon overrules, update the row, the affected steps and the log.
- Names marked "check in CI" are from memory: correct the step's notes after its first CI run.
- Milestone headings are `## Mn (phase 1, now): Title`, `## Mn (next): Title` or `## Mn (later): Title`. A step that adds or drops a part updates its milestone's "**Parts after Mn:** ... **Deliberately not added:** ..." line.
- "What is deliberately not in the plan" grows when something is ruled out; building anything on it needs Alimardon's words.

## Index (README.md)

Every new doc gets a line in `SUMMARY.md`, the docs site's table of contents (M8.10a; a page not listed there is not on the site, and `sh ci/site.sh build` checks every link and anchor between pages), and a row in the `| File | Topic | Status |` table in the same PR: DESIGN-PRINCIPLES.md first, ADRs in number order with the short id as link text (`[ADR-009](ADR-009-slug.md)`), then reviews by file name, ROADMAP.md last. Topic is a short phrase list; Status mirrors the doc's status line in short form, and the row changes when the status does.

`FORMATS.md` exists since M1.6, `SPIKE-flatpak.md` since M1.9, `settings.md` since M2.1, and `TRY-IT.md` (also every release's notes) and `RELEASE.md` since M3.4, and `FEATURES.md` since M4.0, `MESSAGES.md` since 2026-10-06 and `layout-guide.md` since M5.6c; M2.1 added the system-file rules and M3.3b the slot size. Docs the roadmap will add, each in its step: `settings.md` generated from M6.11 on, `SHORTCUTS.md` (M5.13), `INSTALL.md` (M6.6b), the generated `defaults.md` (M6.11), `FLEET.md` (M7.3), `hardware/*.toml` (M8.1), `SECURITY.md` (M8.3), `platform-levels.md` (M8.8) and `backports.md` (M8.10b).

## Known inconsistencies: do not copy them

- ADR-006 says applied settings make a new deployment and that the last few deployments stay in the boot menu; since ADR-008 settings apply at once on `/data`, and there are two slots plus `edel rollback` (M1.1).
- ADR-006's "from a USB stick or a URL" is now a path or an `https://` URL only.
- DESIGN-PRINCIPLES.md (edited by #6, #7 and #9) and ADR-007 (action item 1 edited by #7) do not show those edits on their Date lines.
- Done but unticked: ADR-008 action item 9 (PR #7) and ADR-001 items 4 and 5. ADR-003 item 2 is done by PR #1 (both images built from Alpine v3.24, the VM booted in CI); item 1 is partly done (the image builder exists, our own overlay package repository does not).
- Older ADRs say "behavior" and "colors"; new text uses British spelling.
