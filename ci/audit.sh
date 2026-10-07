#!/bin/sh
# The dependency audit (roadmap M3.9): checks Cargo.lock against the RustSec
# advisory database.
#
#   sh ci/audit.sh    as "Rust checks" runs it on every pull request
#
# A known vulnerability that ci/audit-exceptions.toml does not accept fails;
# an unmaintained, unsound or yanked crate it does not accept is listed in
# the job's summary and does not fail. The database is fetched fresh on
# every run, so a new advisory can fail a pull request that changed nothing.
# Each run installs the one cargo-audit version below with cargo when the
# cargo-audit on PATH is another.
set -eu

# The cargo-audit version the audit runs with: written here only.
audit_version=0.22.2

if [ "$(cargo audit --version 2>/dev/null || true)" != "cargo-audit-audit $audit_version" ]; then
	echo "installing cargo-audit $audit_version with cargo"
	cargo install --quiet --locked cargo-audit --version "$audit_version"
fi

report=$(mktemp)
trap 'rm -f "$report"' EXIT
# Its exit status 1 only says that it found something; the policy is below.
cargo audit --json >"$report" 2>/dev/null || true
if [ ! -s "$report" ]; then
	echo "FAIL: cargo audit did not run: is the advisory database reachable? Run 'cargo audit' for the reason." >&2
	exit 1
fi

python3 -I - "$report" ci/audit-exceptions.toml "${GITHUB_STEP_SUMMARY:-/dev/null}" <<'PY'
import datetime, json, re, sys, tomllib

report_path, exceptions_path, summary_path = sys.argv[1:]
with open(report_path, encoding="utf-8") as f:
    report = json.load(f)
with open(exceptions_path, "rb") as f:
    exceptions = tomllib.load(f).get("exception", [])

# Every exception names what it is for, why and when: nothing is accepted
# without a reason.
bad = []
for e in exceptions:
    name = e.get("id", "(no id)")
    for key in ("id", "crate", "date", "reason"):
        if not str(e.get(key, "")).strip():
            bad.append(f"{name}: no {key}")
    try:
        datetime.date.fromisoformat(str(e.get("date", "")))
    except ValueError:
        bad.append(f"{name}: date is not YYYY-MM-DD")
    if not re.fullmatch(r"RUSTSEC-\d{4}-\d{4}", str(e.get("id", ""))):
        bad.append(f"{name}: id is not RUSTSEC-YYYY-NNNN")
if bad:
    for line in bad:
        print(f"FAIL: ci/audit-exceptions.toml: {line}")
    sys.exit(1)
accepted = {e["id"]: e for e in exceptions}


def entry(item):
    advisory, package = item["advisory"], item["package"]
    return {
        "id": advisory["id"],
        "crate": f"{package['name']} {package['version']}",
        "title": advisory["title"],
        "url": f"https://rustsec.org/advisories/{advisory['id']}",
        "solution": " or ".join(f"patched in `{r}`" for r in (item.get("versions") or {}).get("patched", [])),
    }


vulns, warns = [], []
for v in report["vulnerabilities"]["list"]:
    vulns.append(entry(v))
for kind, items in report["warnings"].items():
    for w in items:
        a = w.get("advisory")
        if a is None:  # a yanked crate has no advisory
            name = f"{w['package']['name']} {w['package']['version']}"
            warns.append({"id": f"yanked {name}", "crate": name, "title": "this version was yanked", "url": "", "kind": kind})
        else:
            warns.append(dict(entry(w), kind=kind))

found = {x["id"] for x in vulns + warns}
failing = [v for v in vulns if v["id"] not in accepted]
loose = [w for w in warns if w["id"] not in accepted]
stale = [i for i in accepted if i not in found]


def link(x):
    return f"[{x['id']}]({x['url']})" if x["url"] else x["id"]


db = report["database"]
out = ["### Dependency audit", ""]
out.append(f"Cargo.lock against the RustSec database of {db['last-updated'][:10]} ({db['advisory-count']} advisories).")
out.append("")
if failing:
    out += ["**Vulnerabilities not accepted (these fail the pull request):**", ""]
    out += [f"- {link(v)} in `{v['crate']}`: {v['title']}. Fix: {v['solution'] or 'none known'}." for v in failing]
    out.append("")
if loose:
    out += ["**Unmaintained, unsound or yanked crates not accepted (listed, not failing):**", ""]
    out += [f"- {w['kind']}: {link(w)} in `{w['crate']}`: {w['title']}." for w in loose]
    out.append("")
if stale:
    out += ["**Accepted but no longer in Cargo.lock (delete these from `ci/audit-exceptions.toml`):** " + ", ".join(stale), ""]
if accepted:
    out += ["Accepted in `ci/audit-exceptions.toml`:", "", "| Advisory | Crate | Since | Reason |", "|---|---|---|---|"]
    out += [f"| {i} | `{e['crate']}` | {e['date']} | {e['reason']} |" for i, e in accepted.items()]
    out.append("")
if not failing and not loose and not accepted:
    out.append("No advisories.")
with open(summary_path, "a", encoding="utf-8") as f:
    f.write("\n".join(out) + "\n")

for v in failing:
    print(f"FAIL: {v['id']} in {v['crate']}: {v['title']} ({v['url']}); fix: {v['solution'] or 'none known'}, or accept it with a reason in ci/audit-exceptions.toml")
for w in loose:
    print(f"WARN: {w['kind']}: {w['id']} in {w['crate']}: {w['title']}")
for i in stale:
    print(f"WARN: {i} is accepted in ci/audit-exceptions.toml but matches nothing in Cargo.lock: delete it")
if failing:
    sys.exit(1)
print(f"PASS: no vulnerability outside ci/audit-exceptions.toml ({len(accepted) - len(stale)} accepted, {len(loose)} other warnings)")
PY
