#!/bin/sh
# The release checklist (roadmap M8.7): what a tag's draft release waits for
# that CI cannot prove on its own. A release waits for its preset pictures
# (the desktop test's release-pictures case, in every preset, light and
# dark, at scale 1 and 2), the defaults diff against the last kept release
# (M6.11) and a person's ticks on the checklist issue
# (.github/ISSUE_TEMPLATE/release-checklist.md: the hardware items and the
# Defaults reviewed box), so none of them is missed by a release made in a hurry.
#
#   sh ci/release-checklist.sh gather DIR   collect the pictures from the desktop
#                                           parts' artifacts DIR (out/parts) and
#                                           write defaults.diff and issue.md
#   sh ci/release-checklist.sh open TAG     open the checklist issue for TAG
#                                           (ci/release.sh publish, once the draft exists)
#   sh ci/release-checklist.sh verify TAG   fail while a box of TAG's issue is not
#                                           ticked (release.yml, before the channel moves)
#   sh ci/release-checklist.sh --self-test  prove the gate, the gather and the failure
#
# Output goes to $EDEL_CHECKLIST_OUT (default out/release-checklist): pictures/,
# defaults.diff, issue.md, and ready once gather has all of them. open and
# verify need python3 and gh (GH_TOKEN, GITHUB_REPOSITORY); gather needs git.
set -eu

out=${EDEL_CHECKLIST_OUT:-out/release-checklist}
template=.github/ISSUE_TEMPLATE/release-checklist.md

# The desktop test's release-pictures case saves these names, so the two
# lists here must match it: the colour schemes and the scales.
schemes="light dark"
scales="1 2"

# The presets, classic first and the rest in order, read from presets/ so a
# new preset file is picked up with no edit.
preset_names() {
	echo classic
	for f in presets/*.toml; do
		n=$(basename "$f" .toml)
		[ "$n" = classic ] || echo "$n"
	done
}

# The one place the checklist's title is written.
title() {
	echo "Release $1: checklist"
}

# find_issue TAG: prints the number of the checklist issue with TAG's title, or nothing.
find_issue() {
	gh api "repos/$GITHUB_REPOSITORY/issues?state=all&labels=release&per_page=100" |
		python3 -I -c '
import json, sys
title = sys.argv[1]
for issue in json.load(sys.stdin):
    if issue.get("title") == title:
        print(issue["number"])
        break
' "$(title "$1")"
}

gather() {
	dir=$1
	rm -rf "$out"
	passed=$(find "$dir" -name passed ! -path '*/first/*' -exec cat {} + 2>/dev/null || true)
	if ! printf '%s\n' "$passed" | grep -qx release-pictures; then
		echo "release checklist: the release-pictures case did not pass, so nothing is gathered"
		exit 0
	fi
	mkdir -p "$out/pictures"
	find "$dir" -name 'release-*.png' ! -path '*/first/*' -exec cp {} "$out/pictures/" \;
	missing=
	for preset in $(preset_names); do
		for scheme in $schemes; do
			for scale in $scales; do
				name="release-$preset-$scheme-$scale.png"
				[ -f "$out/pictures/$name" ] || missing="$missing${missing:+, }$name"
			done
		done
	done
	if [ -n "$missing" ]; then
		echo "FAIL: the release's pictures lack $missing"
		exit 1
	fi
	count=$(ls "$out/pictures" | wc -l)
	# defaults-diff.sh appends to its summary file, which is new here.
	GITHUB_STEP_SUMMARY="$out/defaults.diff" GITHUB_EVENT_NAME="${GITHUB_EVENT_NAME:-push}" sh ci/defaults-diff.sh
	# The template's body: what follows its second --- line.
	awk 'c >= 2 { print } /^---$/ { c++ }' "$template" >"$out/issue.md"
	touch "$out/ready"
	msg="release checklist: $count pictures, defaults.diff and the checklist issue ready"
	echo "$msg"
	if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
		echo "$msg" >>"$GITHUB_STEP_SUMMARY"
	fi
}

open_issue() {
	tag=$1
	gh label create release --description "A release's checklist (M8.7)" --color 0e8a16 2>/dev/null || true
	number=$(find_issue "$tag")
	if [ -z "$number" ]; then
		gh issue create --title "$(title "$tag")" --label release --body-file "$out/issue.md"
		echo "release checklist: opened the issue $(title "$tag")"
	else
		echo "release checklist: the issue $(title "$tag") is already there (#$number)"
	fi
}

verify_issue() {
	tag=$1
	number=$(find_issue "$tag")
	if [ -z "$number" ]; then
		echo "FAIL: no issue \"$(title "$tag")\"; the release workflow opens it with the draft"
		exit 1
	fi
	body=$(gh api "repos/$GITHUB_REPOSITORY/issues/$number" --jq .body)
	unticked=$(printf '%s\n' "$body" | grep '^- \[ \] ' || true)
	if [ -n "$unticked" ]; then
		echo "FAIL: the checklist of $tag has boxes not ticked:"
		printf '%s\n' "$unticked"
		echo "Tick them in the issue, then re-run this workflow; the stable channel stays where it is."
		exit 1
	fi
	echo "release checklist: every box of $tag is ticked"
}

self_test() {
	mkdir -p out
	work=$(mktemp -d out/release-checklist-test.XXXXXX)
	trap 'rm -rf "$work"' EXIT
	out="$work/out"
	export EDEL_CHECKLIST_OUT="$out" GITHUB_EVENT_NAME=push
	unset GITHUB_STEP_SUMMARY
	parts="$work/parts"
	part="$parts/edel-desktop-test-D"
	mkdir -p "$part/first"
	printf 'release-pictures\n' >"$part/passed"
	expected=0
	for preset in $(preset_names); do
		for scheme in $schemes; do
			for scale in $scales; do
				: >"$part/release-$preset-$scheme-$scale.png"
				expected=$((expected + 1))
			done
		done
	done
	# A stray copy in first/ (a failed first try's pictures) must not count.
	: >"$part/first/release-zen-dark-2.png"

	# (1) Every picture, the passed case and no stray copy: the checklist is ready.
	if ! got=$(gather "$parts" 2>&1); then
		echo "FAIL: release-checklist self-test: gather refused the complete pictures: $got"
		exit 1
	fi
	[ -e "$out/ready" ] || {
		echo "FAIL: release-checklist self-test: gather wrote no ready file"
		exit 1
	}
	count=$(ls "$out/pictures" | wc -l)
	[ "$count" -eq "$expected" ] || {
		echo "FAIL: release-checklist self-test: gather copied $count pictures, wanted $expected"
		exit 1
	}
	[ -s "$out/defaults.diff" ] || {
		echo "FAIL: release-checklist self-test: gather wrote no defaults.diff"
		exit 1
	}
	[ -s "$out/issue.md" ] || {
		echo "FAIL: release-checklist self-test: gather wrote no issue.md"
		exit 1
	}
	echo "PASS: gather collected $count pictures, defaults.diff and the issue text"

	# (2) One picture missing: gather fails, names it and makes no ready file.
	rm -f "$part/release-zen-dark-2.png"
	rm -rf "$out"
	if got=$(gather "$parts" 2>&1); then
		echo "FAIL: release-checklist self-test: gather passed without release-zen-dark-2.png"
		exit 1
	fi
	case $got in
	*release-zen-dark-2*) ;;
	*)
		echo "FAIL: release-checklist self-test: gather did not name release-zen-dark-2.png: $got"
		exit 1
		;;
	esac
	[ ! -e "$out/ready" ] || {
		echo "FAIL: release-checklist self-test: gather left a ready file after a failure"
		exit 1
	}
	echo "PASS: gather failed on a missing picture and named it"

	# (3) With that failed output, a tag's publish stops before gh.
	if got=$(sh ci/release.sh publish v9.9.9 2>&1); then
		echo "FAIL: release-checklist self-test: publish made a tag's draft without the checklist"
		exit 1
	fi
	want="FAIL: the release checklist is not ready (ci/release-checklist.sh gather): no draft release is made"
	case $got in
	*"$want"*) ;;
	*)
		echo "FAIL: release-checklist self-test: publish did not refuse with the checklist's message: $got"
		exit 1
		;;
	esac
	echo "PASS: publish refused a tag's draft without the checklist"
	echo "release-checklist self-test: ok"
}

case "${1:-}" in
gather)
	gather "${2:?usage: sh ci/release-checklist.sh gather DIR}"
	;;
open)
	open_issue "${2:?usage: sh ci/release-checklist.sh open TAG}"
	;;
verify)
	verify_issue "${2:?usage: sh ci/release-checklist.sh verify TAG}"
	;;
--self-test)
	self_test
	;;
*)
	echo "usage: sh ci/release-checklist.sh gather DIR|open TAG|verify TAG|--self-test" >&2
	exit 2
	;;
esac
