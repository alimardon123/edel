#!/bin/sh
# Decides whether a change can affect the heavy CI jobs (CI speed-ups of
# 2026-10-16): the image build and boot tests ("Build and boot images") and
# the desktop tests ("Desktop tests"). A change that cannot touch them skips
# those jobs; the joined checks still report success. A draft runs the Rust
# checks only, unless it carries the label full-ci.
#
#   sh ci/changes.sh               decide for this checkout (CI's first job)
#   sh ci/changes.sh --self-test   check the classification on fixed lists
#
# Reads EVENT, DRAFT and FULL_CI from the environment. Prints images=,
# desktop=, lanes= and a reason line. Appends the first three to
# $GITHUB_OUTPUT, and a short Markdown section to $GITHUB_STEP_SUMMARY,
# when those variables are set and not empty.
set -eu

# kind PATH: prints the heavy jobs a changed PATH can affect: all (both),
# vm (the image build and its tests), desktop (the desktop tests) or none.
kind() {
	case "$1" in
	features/* | images/* | presets/* | design/* | crates/*/src/* | po/*) echo all ;;
	docs/* | *.md | book.toml | SUMMARY.md | theme/* | LICENSE | .github/workflows/release.yml | ci/audit.sh | ci/audit-exceptions.toml | ci/site.sh | ci/one-place.sh | ci/seams.sh | ci/keys-check.sh | ci/defaults-diff.sh | ci/changes.sh) echo none ;;
	ci/boot-test.sh | ci/system-test.sh | ci/install-test.sh | ci/install-test/* | ci/ab-test.sh | ci/ab-test/* | ci/apps-test.sh | ci/apps-test/* | ci/flatpak-test.sh | ci/flatpak/* | ci/vm-tests.sh | ci/sizes.sh | ci/https-serve.py | ci/initramfs-check.sh | ci/kernel-options.toml | ci/container-test.sh | ci/keys-carried.sh | ci/release.sh | ci/channel.sh | ci/sign.sh | ci/package-report.sh) echo vm ;;
	ci/desktop-part.sh | ci/basics-test.sh) echo desktop ;;
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
	try "no no" "" "empty input"
	if [ "$fails" -gt 0 ]; then
		exit 1
	fi
	echo "PASS: changes.sh classifies $count lists"
}

if [ "${1:-}" = --self-test ]; then
	self_test
	exit 0
fi

tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT

# Lanes of the VM tests, read from ci/vm-tests.sh, as a JSON array.
names=$(sh ci/vm-tests.sh lanes)
lanes=$(printf '%s\n' "$names" | awk 'NF { s = s (n++ ? "," : "") "\"" $1 "\"" } END { print "[" s "]" }')

if [ "${EVENT:-}" != pull_request ]; then
	images=yes
	desktop=yes
	reason="a push or a tag runs everything"
elif [ "${DRAFT:-}" = true ] && [ "${FULL_CI:-}" != true ]; then
	images=no
	desktop=no
	reason="a draft runs Rust checks only; mark it ready, or add the label full-ci and push, for the rest"
elif ! git rev-parse --verify -q HEAD^1 >/dev/null; then
	images=yes
	desktop=yes
	reason="the base commit is not here, so everything runs"
else
	# CI checks out the merge commit (fetch-depth 2), so HEAD^1 is the base.
	git diff --name-only HEAD^1 HEAD >"$tmp"
	decide <"$tmp"
fi

printf 'images=%s\ndesktop=%s\nlanes=%s\n' "$images" "$desktop" "$lanes"
echo "reason: $reason"
if [ -n "${GITHUB_OUTPUT:-}" ]; then
	printf 'images=%s\ndesktop=%s\nlanes=%s\n' "$images" "$desktop" "$lanes" >>"$GITHUB_OUTPUT"
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
