#!/bin/sh
# basics-test.sh [IMAGE_DEFINITION] (roadmap M6.7): the basics checklist.
#
# Every feature file may list its basics as [[basics]] rows (features/NAME.toml,
# docs/FEATURES.md): a row a person expects to work, such as Audio, is either
# proved by a desktop-test case that the desktop test passed (`test`), or
# marked as needing hardware (`needs = "hardware"`) or the roadmap step that
# brings it (`needs = "M7.8"`). So adding or dropping a feature adds or drops
# its rows (ADR-008).
#
# This reads the image definition (default ci/desktop/vm.toml, the image the
# desktop test boots) and each of its features in order, finds the rows, and
# checks each `test` against out/desktop-test/passed, which desktop-test.sh
# writes one passed case per line. It prints a Markdown table under "Basics
# checklist (M6.7)", appends it to the step summary when GitHub Actions sets
# one, and exits 1 when a row fails or the definition lists no basics at all.
#
# Needs Python 3.11 or later (tomllib), with the standard library only.
set -eu

definition=${1:-ci/desktop/vm.toml}
python3 -I - "$definition" out/desktop-test/passed <<'PYTHON'
import os
import sys
import tomllib

definition, passed_file = sys.argv[1], sys.argv[2]


def fail_now(message):
    print(f"basics-test: {message}", file=sys.stderr)
    sys.exit(1)


def feature_dirs(def_dir):
    # As edel's def::feature_dirs: the features beside the definition first,
    # then the repository's, the nearest directory above that holds both
    # features/ and images/.
    dirs = []
    if os.path.isdir(os.path.join(def_dir, "features")):
        dirs.append(os.path.join(def_dir, "features"))
    here = def_dir
    while not (os.path.isdir(os.path.join(here, "features")) and os.path.isdir(os.path.join(here, "images"))):
        parent = os.path.dirname(here)
        if parent == here:
            fail_now(f"no directory at or above {def_dir} holds both features/ and images/")
        here = parent
    repo = os.path.join(here, "features")
    if repo not in dirs:
        dirs.append(repo)
    return dirs


with open(definition, "rb") as f:
    image = tomllib.load(f)
dirs = feature_dirs(os.path.dirname(os.path.abspath(definition)))
passed = set()
if os.path.exists(passed_file):
    with open(passed_file) as f:
        passed = {line.strip() for line in f if line.strip()}

rows = []
for feature in image.get("features", []):
    path = next((p for p in (os.path.join(d, feature + ".toml") for d in dirs) if os.path.isfile(p)), None)
    if path is None:
        fail_now(f"feature {feature} of {definition} has no features/{feature}.toml")
    with open(path, "rb") as f:
        basics = tomllib.load(f).get("basics", [])
    for basic in basics:
        name, test, needs = basic["name"], basic.get("test"), basic.get("needs")
        if test is not None:
            if test in passed:
                result = "PASS"
            else:
                result = f"FAIL (case {test} did not pass)"
            proved = f"`{test}`"
        elif needs == "hardware":
            result, proved = "needs hardware", ""
        elif needs is not None:
            result, proved = f"comes in {needs}", ""
        else:
            result, proved = "FAIL (the row has neither test nor needs)", ""
        rows.append((name, feature, result, proved))

lines = ["### Basics checklist (M6.7)", "", "| Basic | Feature | Result | Proved by |", "| --- | --- | --- | --- |"]
if not rows:
    lines.append("")
    lines.append(f"FAIL: no feature of {definition} lists a basic")
for name, feature, result, proved in rows:
    lines.append(f"| {name} | {feature} | {result} | {proved} |")
text = "\n".join(lines) + "\n"
print(text, end="")
summary = os.environ.get("GITHUB_STEP_SUMMARY")
if summary:
    with open(summary, "a") as f:
        f.write(text)

failed = not rows or any(result.startswith("FAIL") for _, _, result, _ in rows)
sys.exit(1 if failed else 0)
PYTHON
