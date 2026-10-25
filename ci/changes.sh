#!/bin/sh
# Decides whether a change can affect the heavy CI jobs (CI speed-ups of
# 2026-10-16): the image build and boot tests ("Build and boot images") and
# the desktop tests ("Desktop tests"). A change that cannot touch them skips
# those jobs; the joined checks still report success. A draft runs the Rust
# checks only, unless it carries the label full-ci, or labels lane-NAME
# (lane-fresh, say), which run those VM test lanes alone: on images an
# earlier run of the pull request built, when everything changed since is
# test-side (`test_side`), so a test fix is proven in minutes; else
# on a new build. A draft never merges, and marking it ready runs
# everything, so one full run still comes before each merge.
#
#   sh ci/changes.sh               decide for this checkout (CI's first job)
#   sh ci/changes.sh --self-test   check the classification on fixed lists
#
# Reads EVENT, DRAFT, FULL_CI and LABELS (the pull request's, by spaces)
# from the environment, and for a pull request BASE_REF, ACTION, BEFORE,
# AFTER, HEAD_REF, HEAD_SHA and REPO (GH_TOKEN for gh). Prints images=,
# desktop=, lanes=, reuse= and number= (the run whose images a lane run
# takes, and its run number, which numbered them; both empty when the
# images are built) and a reason line. Appends all but the reason to
# $GITHUB_OUTPUT, and a short Markdown section to $GITHUB_STEP_SUMMARY,
# when those variables are set and not empty.
set -eu

# kind PATH: prints the heavy jobs a changed PATH can affect: all (both),
# vm (the image build and its tests), desktop (the desktop tests) or none.
kind() {
	case "$1" in
	features/* | images/* | presets/* | design/* | crates/*/src/* | po/*) echo all ;;
	docs/* | *.md | book.toml | SUMMARY.md | theme/* | LICENSE | .github/workflows/release.yml | .github/workflows/monthly.yml | .github/workflows/runtime-spike.yml | ci/runtime-spike/* | .github/workflows/desktop-memory.yml | ci/desktop-memory.sh | ci/monthly.sh | ci/audit.sh | ci/audit-exceptions.toml | ci/site.sh | ci/one-place.sh | ci/seams.sh | ci/keys-check.sh | ci/defaults-diff.sh | ci/changes.sh) echo none ;;
	ci/boot-test.sh | ci/system-test.sh | ci/install-test.sh | ci/install-test/* | ci/ab-test.sh | ci/ab-test/* | ci/apps-test.sh | ci/apps-test/* | ci/flatpak-test.sh | ci/flatpak/* | ci/fleet-test.sh | ci/fleet/* | ci/server-test.sh | ci/server/* | ci/upgrade-test.sh | ci/upgrade-test/* | ci/updates-test.sh | ci/updates-test/* | ci/developer-test.sh | ci/developer-test/* | ci/vm-tests.sh | ci/sizes.sh | ci/https-serve.py | ci/initramfs-check.sh | ci/kernel-options.toml | ci/container-test.sh | ci/repro-check.sh | ci/keys-carried.sh | ci/release.sh | ci/channel.sh | ci/sign.sh | ci/package-report.sh | ci/bom.sh) echo vm ;;
	ci/desktop-part.sh | ci/basics-test.sh | ci/release-checklist.sh) echo desktop ;;
	*) echo all ;;
	esac
}

# decide: reads changed paths on standard input (one per line) and sets
# images, desktop and reason. Reads from a file, never from a pipe, so the
# variables stay set in this shell.
decide() {
	images=no
	desktop=no
	image_by=
	desktop_by=
	while IFS= read -r path; do
		[ -n "$path" ] || continue
		case $(kind "$path") in
		all)
			[ -n "$image_by" ] || image_by=$path
			[ -n "$desktop_by" ] || desktop_by=$path
			;;
		vm) [ -n "$image_by" ] || image_by=$path ;;
		desktop) [ -n "$desktop_by" ] || desktop_by=$path ;;
		esac
	done
	if [ -n "$image_by" ]; then images=yes; fi
	if [ -n "$desktop_by" ]; then desktop=yes; fi
	if [ "$images" = no ] && [ "$desktop" = no ]; then
		reason="only paths that cannot affect them changed"
		return 0
	fi
	img="images: none of the paths"
	if [ "$images" = yes ]; then img="images: $image_by"; fi
	dsk="desktop: none of the paths"
	if [ "$desktop" = yes ]; then dsk="desktop: $desktop_by"; fi
	reason="$img; $dsk"
}

# try WANT PATHS LABEL: runs decide on PATHS (words, one path each) and
# checks that images and desktop give WANT ("yes no" and so on).
try() {
	count=$((count + 1))
	printf '%s\n' $2 >"$tmp"
	decide <"$tmp"
	if [ "$images $desktop" = "$1" ]; then
		echo "PASS: $3 -> images=$images desktop=$desktop"
	else
		echo "FAIL: $3: wanted $1, got images=$images desktop=$desktop ($reason)"
		fails=$((fails + 1))
	fi
}

self_test() {
	tmp=$(mktemp)
	trap 'rm -f "$tmp"' EXIT
	count=0
	fails=0
	try "no no" "docs/ROADMAP.md" "docs/ROADMAP.md alone"
	try "no no" "crates/edel/CLAUDE.md" "crates/edel/CLAUDE.md"
	try "yes yes" "features/base.toml" "features/base.toml"
	try "yes yes" "features/x/README.md" "features/x/README.md"
	try "yes no" "ci/sizes.sh" "ci/sizes.sh"
	try "no yes" "ci/desktop-part.sh" "ci/desktop-part.sh"
	try "yes yes" "ci/sizes.sh ci/desktop-part.sh" "ci/sizes.sh and ci/desktop-part.sh"
	try "yes yes" ".github/workflows/ci.yml" ".github/workflows/ci.yml"
	try "yes yes" "Cargo.lock" "Cargo.lock"
	try "no no" "ci/runtime-spike/all.sh .github/workflows/runtime-spike.yml" "the app runtime spike alone"
	try "no no" "" "empty input"
	# Which changes leave an earlier build's images good for a lane run.
	for case in "yes ci/desktop-test.sh ci/budgets.toml docs/ROADMAP.md" "no ci/desktop-test.sh ci/desktop/seed.toml" "no ci/build.sh" "no crates/edel/src/main.rs" "yes "; do
		count=$((count + 1))
		want=${case%% *}
		printf '%s\n' ${case#* } >"$tmp"
		if test_side <"$tmp"; then got=yes; else got=no; fi
		if [ "$got" = "$want" ]; then
			echo "PASS: test side only: ${case#* } -> $got"
		else
			echo "FAIL: test side only: ${case#* }: wanted $want, got $got"
			fails=$((fails + 1))
		fi
	done
	# Lane labels name lanes vm-tests.sh has, and nothing else.
	count=$((count + 1))
	got=$(lane_labels "full-ci lane-fresh lane-system lane-nowhere desktop")
	if [ "$got" = "system fresh" ]; then
		echo "PASS: lane labels lane-fresh and lane-system -> $got"
	else
		echo "FAIL: lane labels: wanted system fresh, got $got"
		fails=$((fails + 1))
	fi
	# Each archive vm-tests.sh packs has its own artifact in the workflow,
	# which the lanes reading it download by name.
	for name in $(sh ci/vm-tests.sh archives); do
		count=$((count + 1))
		if grep -q "name: edel-vm-images-$name\$" .github/workflows/ci.yml; then
			echo "PASS: the workflow uploads the archive $name"
		else
			echo "FAIL: .github/workflows/ci.yml has no step uploading edel-vm-images-$name, which vm-tests.sh packs"
			fails=$((fails + 1))
		fi
	done
	if [ "$fails" -gt 0 ]; then
		exit 1
	fi
	echo "PASS: changes.sh made $count checks"
}

# proven: true when a push to a pull request (BEFORE to AFTER) changed only
# paths that cannot affect the heavy jobs, and the run on BEFORE passed with
# the image build and every desktop part run, not skipped (faster PRs,
# 2026-10-17): a merge of main that only brought the roadmap's log, say.
# Anything it cannot read makes it false, so everything runs.
proven() {
	[ "${ACTION:-}" = synchronize ] && [ -n "${BEFORE:-}" ] && [ -n "${AFTER:-}" ] && [ -n "${REPO:-}" ] || return 1
	git fetch -q --depth=1 origin "$BEFORE" "$AFTER" 2>/dev/null || return 1
	git diff --name-only "$BEFORE" "$AFTER" >"$tmp" || return 1
	decide <"$tmp"
	[ "$images" = no ] && [ "$desktop" = no ] || return 1
	run=$(gh api "repos/$REPO/actions/runs?head_sha=$BEFORE&event=pull_request" \
		--jq '[.workflow_runs[] | select(.name == "CI" and .conclusion == "success")][0].id // empty' 2>/dev/null) || return 1
	[ -n "$run" ] || return 1
	ran=$(gh api "repos/$REPO/actions/runs/$run/jobs?per_page=100" \
		--jq '[.jobs[] | select(.name == "Build images" or (.name | startswith("Desktop part")))] | (length > 1 and all(.conclusion == "success"))' 2>/dev/null) || return 1
	[ "$ran" = true ]
}

# test_side: true when every path on standard input is one of the VM
# tests' own scripts, run on the runner and held by no image, or cannot
# affect the heavy jobs, so images built before them still serve.
test_side() {
	while IFS= read -r path; do
		[ -n "$path" ] || continue
		case $path in
		ci/boot-test.sh | ci/system-test.sh | ci/install-test.sh | ci/ab-test.sh | ci/apps-test.sh | ci/flatpak-test.sh | ci/fleet-test.sh | ci/server-test.sh | ci/upgrade-test.sh | ci/upgrade-test/* | ci/updates-test.sh | ci/developer-test.sh | ci/desktop-test.sh | ci/vm-tests.sh | ci/vm.sh | ci/budgets.sh | ci/budgets.toml | ci/qmp.py | ci/serial.py | ci/https-serve.py) ;;
		*) [ "$(kind "$path")" = none ] || return 1 ;;
		esac
	done
}

# lane_labels LABELS: the VM test lanes the labels lane-NAME ask for, by
# spaces, in vm-tests.sh's order; labels naming no lane are left out.
lane_labels() {
	for name in $(sh ci/vm-tests.sh lanes); do
		case " $1 " in
		*" lane-$name "*) printf '%s ' "$name" ;;
		esac
	done | sed 's/ $//'
}

# reusable: sets reuse and number to the newest run of this pull request's
# branch, in the last 20 hours (its archives are kept a day), whose image
# build passed and whose commit differs from HEAD_SHA in test-side paths
# only. False when there is none, or anything cannot be read.
reusable() {
	[ -n "${HEAD_REF:-}" ] && [ -n "${HEAD_SHA:-}" ] && [ -n "${REPO:-}" ] || return 1
	since=$(date -u -d '20 hours ago' +%Y-%m-%dT%H:%M:%SZ 2>/dev/null) || return 1
	gh api "repos/$REPO/actions/runs?branch=$HEAD_REF&event=pull_request&per_page=20" \
		--jq ".workflow_runs[] | select(.name == \"CI\" and .created_at >= \"$since\") | \"\\(.id) \\(.run_number) \\(.head_sha)\"" \
		>"$tmp.runs" 2>/dev/null || return 1
	while read -r id n sha; do
		built=$(gh api "repos/$REPO/actions/runs/$id/jobs?per_page=100" \
			--jq '[.jobs[] | select(.name == "Build images" and .conclusion == "success")] | length' 2>/dev/null) || continue
		[ "$built" = 1 ] || continue
		git fetch -q --depth=1 origin "$sha" "$HEAD_SHA" 2>/dev/null || continue
		git diff --name-only "$sha" "$HEAD_SHA" >"$tmp" 2>/dev/null || continue
		if test_side <"$tmp"; then
			reuse=$id number=$n
			rm -f "$tmp.runs"
			return 0
		fi
	done <"$tmp.runs"
	rm -f "$tmp.runs"
	return 1
}

if [ "${1:-}" = --self-test ]; then
	self_test
	exit 0
fi

tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT

# Lanes of the VM tests, read from ci/vm-tests.sh, as a JSON array.
names=$(sh ci/vm-tests.sh lanes)
json() {
	printf '%s\n' $1 | awk 'NF { s = s (n++ ? "," : "") "\"" $1 "\"" } END { print "[" s "]" }'
}
lanes=$(json "$names")
reuse=''
number=''
asked=$(lane_labels "${LABELS:-}")

if [ "${EVENT:-}" != pull_request ]; then
	images=yes
	desktop=yes
	reason="a push or a tag runs everything"
elif [ "${DRAFT:-}" = true ] && [ "${FULL_CI:-}" != true ] && [ -n "$asked" ]; then
	images=yes
	desktop=no
	lanes=$(json "$asked")
	if reusable; then
		reason="a draft labelled for the lanes $asked runs them alone, on the images run $reuse built, as only test-side paths changed since"
	else
		reason="a draft labelled for the lanes $asked runs them alone, on a new build (no run of the last 20 hours built images that still serve)"
	fi
elif [ "${DRAFT:-}" = true ] && [ "${FULL_CI:-}" != true ]; then
	images=no
	desktop=no
	reason="a draft runs Rust checks only; mark it ready, or add the label full-ci and push, for the rest (or lane-NAME for one VM test lane)"
elif [ -n "${BASE_REF:-}" ] && [ "$BASE_REF" != main ]; then
	images=no
	desktop=no
	reason="a step into the lane branch $BASE_REF runs Rust checks only; the lane's pull request to main runs the rest"
elif proven; then
	images=no
	desktop=no
	reason="only paths that cannot affect them changed since $BEFORE, whose full run passed"
elif ! git rev-parse --verify -q HEAD^1 >/dev/null; then
	images=yes
	desktop=yes
	reason="the base commit is not here, so everything runs"
else
	# CI checks out the merge commit (fetch-depth 2), so HEAD^1 is the base.
	git diff --name-only HEAD^1 HEAD >"$tmp"
	decide <"$tmp"
fi

printf 'images=%s\ndesktop=%s\nlanes=%s\nreuse=%s\nnumber=%s\n' "$images" "$desktop" "$lanes" "$reuse" "$number"
echo "reason: $reason"
if [ -n "${GITHUB_OUTPUT:-}" ]; then
	printf 'images=%s\ndesktop=%s\nlanes=%s\nreuse=%s\nnumber=%s\n' "$images" "$desktop" "$lanes" "$reuse" "$number" >>"$GITHUB_OUTPUT"
fi
if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
	{
		echo "### What this change can affect"
		echo
		echo "- Image build and boot tests: $images"
		echo "- Desktop tests: $desktop"
		echo "- Reason: $reason"
	} >>"$GITHUB_STEP_SUMMARY"
fi
