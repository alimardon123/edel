# Messages and logs

Every word Edel OS shows or writes down follows this page: errors, warnings, progress lines, log lines and what Settings says when something goes wrong. Alimardon asked for it on 2026-10-06: messages and logs "easy to understand, easy to read", easy to find, "ergonomically beautiful and still in detail" when detail is needed, so a person can find out what happened and what to do without help. ADR-008's easy to use decision ("mistakes explain themselves") is the rule this page spells out; roadmap step M5.28 brings older messages up to it and gives the logs one place.

## An error says three things

1. **What failed**, in the person's words, naming the thing: `could not install the update into slot B`.
2. **Why**, as far as we know: `the download stopped after 30 s without an answer from the server`.
3. **What to do**: `check the network and run edel update again; the running system is unchanged`.

One line when it fits; the cause chain after it, most specific last, only when it helps. Never a bare code, a stack trace or "something went wrong". Say what was kept safe when that matters to the person ("the running system is unchanged").

| Instead of | Write |
|---|---|
| `Error: EACCES` | `could not write /data/edel/settings.toml: permission denied; run it as root (sudo edel settings set ...)` |
| `invalid value` | `layout.preset: "hiv" is not a preset; did you mean hive? The presets are classic, mac-like, windows-like and hive` |
| `failed` | `the desktop did not start: no screen answered; it starts once one is plugged in` |

## How lines look

- **Plain words**, short sentences, no jargon a person cannot look up. A technical name (a path, a key, a command) goes in as it is, so it can be searched and copied.
- **Lower case, no final period** for command-line messages (`crates/edel/CLAUDE.md`); Settings shows the same text as a sentence.
- **Values quoted** when they could hide spaces or be empty: `"ali laptop"`.
- **One prefix per part**, so a mixed log reads at a glance: `edel update: ...`, `edel settings: ...`, `edel-compositor: ...`, `edel-shell-ui: ...`. `edel` prefixes the command the person ran, on its progress lines and on a failure's one line (`edel update: refused: expired: ...`).
- **Warnings** start `warning:` and say what still worked: `warning: the clock could not be read; the panel shows no time`.
- **Progress** says what is happening now and what it led to, never only "done": `edel update: slot B written and checked (412 MiB, 9 s); restart to use it`.
- **Numbers carry units** (MiB, s, ms) and come from measurements.
- **The same mistake gives the same text** everywhere: Settings, `edel`, the boot log. Write it once, in the part that finds the mistake, and show it from there (ADR-010).

## Where to find things

- **The command line:** results on standard output, problems on standard error; the exit status says which (`0` done, `1` failed, `2` wrong use, `3` waiting for a yes, as `edel install` does).
- **The boot:** the console and the system log; each service prints one line saying what it did.
- **The desktop:** the compositor, shell-ui and every app they start write to the person's session log, `~/.local/state/edel/session.log`, which the compositor opens at the start of each session (M5.28a); the session before's is `session.old.log` beside it, so the log stays small. A session that closed as it should ends with `edel-compositor: the session ended`; when one did not, the next one says so and `edel status` names its log.
- **One report:** `edel report` gathers what a person sends with a bug: the release, the hardware, and the last lines of the system log and of each session log (M3.3b, M5.28a).
- **Detail on demand:** normal runs stay short; the full detail (each step, each value read, timings) is there when asked for, and in the logs, never in a person's way.

## Checking

A pull request that adds a message shows it, as ADR-008 asks for a key, a shortcut or a command. Tests compare messages exactly where people meet them, so a change to a message is a change to its test. M8.15 reads every message a newcomer can meet against this page.
