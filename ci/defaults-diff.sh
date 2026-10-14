#!/bin/sh
# Writes "Visible defaults changed" to the step summary (M6.11): what a
# person would see differently out of the box, as the diff of the design
# tokens and icons (design/), the presets (presets/) and every key's
# default (docs/defaults.md, generated from edel::key_help) against the
# last kept release, the newest tag vVERSION (docs/RELEASE.md: kept
# forever). Before the first one, a pull request is compared with main,
# and the section says so. A record for the reviewer, as M3.1's package
# diff is, never a gate: it always exits 0 once it has written.
set -eu

summary=${GITHUB_STEP_SUMMARY:-/dev/stdout}
# The most lines one file's diff shows; the rest is cut, with a note.
most=300

git fetch --quiet --force --tags origin 2>/dev/null || true
tag=$(git tag --list 'v[0-9]*' --sort=-v:refname | head -n 1)
if [ -n "$tag" ]; then
	base=$tag
	against="the kept release $tag"
elif [ "${GITHUB_EVENT_NAME:-pull_request}" = pull_request ]; then
	git fetch --quiet --depth=1 origin main
	base=FETCH_HEAD
	against="main, as no release is kept yet"
else
	{
		echo "### Visible defaults changed (M6.11)"
		echo
		echo "No release is kept yet, so there is nothing to compare with on a $GITHUB_EVENT_NAME."
	} >>"$summary"
	exit 0
fi

changed=$(git diff --name-only "$base" HEAD -- design presets docs/defaults.md)
{
	echo "### Visible defaults changed (M6.11)"
	echo
	if [ -z "$changed" ]; then
		echo "Against $against: no design token, icon, preset or default changed."
		exit 0
	fi
	echo "Against $against:"
	echo
	echo '```'
	git diff --stat=100 "$base" HEAD -- design presets docs/defaults.md
	echo '```'
	for file in $changed; do
		case $file in
		*.svg | *.png | *.jpg) continue ;; # drawn, so the list above is enough
		esac
		lines=$(git diff "$base" HEAD -- "$file" | wc -l)
		echo
		echo "<details><summary>$file</summary>"
		echo
		echo '```diff'
		git diff "$base" HEAD -- "$file" | head -n "$most"
		echo '```'
		[ "$lines" -le "$most" ] || echo "The diff goes on for $((lines - most)) more lines: git diff $base HEAD -- $file"
		echo
		echo "</details>"
	done
} >>"$summary"
