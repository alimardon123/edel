#!/bin/sh
# Fails when a change deletes a line of crates/edel/tests/keys.txt without
# bumping FORMAT in crates/edel/src/settings.rs: once Edel OS is released, a
# settings file key is never removed or renamed within a format, because
# old slots still read the files newer ones write (ADR-008, roadmap M2.1).
# Two comparisons: with main, for pull requests (and by hand) only, as a
# push to main was checked as its pull request; and with every kept
# release, each tag vVERSION (docs/RELEASE.md: kept forever), on every run
# (M6.11), as a machine may still run any of them: every key a kept
# release's keys.txt lists must still be listed, unless FORMAT has moved
# on since that release.
set -eu

keys=crates/edel/tests/keys.txt
settings=crates/edel/src/settings.rs

# FORMAT as settings.rs (on standard input) declares it.
format_of() {
	sed -n 's/^pub const FORMAT: i64 = \([0-9]*\);$/\1/p'
}

# Every kept release: its keys must all be here, within one format.
git fetch --quiet --force --tags origin 2>/dev/null || true
format_now=$(format_of <"$settings")
kept=0
for tag in $(git tag --list 'v[0-9]*'); do
	kept=$((kept + 1))
	format_then=$(git show "$tag:$settings" 2>/dev/null | format_of)
	if [ "$format_then" != "$format_now" ]; then
		echo "PASS: $tag is format $format_then and this is format $format_now, so its keys may go"
		continue
	fi
	gone=$(git show "$tag:$keys" 2>/dev/null | grep -vxF -f "$keys" || true)
	if [ -n "$gone" ]; then
		echo "$gone"
		echo "FAIL: these keys of the kept release $tag left $keys within format $format_now; a key is never removed or renamed within a format, so bump FORMAT in $settings or keep them"
		exit 1
	fi
	echo "PASS: every key of the kept release $tag is still in $keys"
done
[ "$kept" -gt 0 ] || echo "PASS: no kept release yet (no tag vVERSION)"

# Until the first public release nobody runs Edel OS, so a key may be
# renamed or dropped with no old name kept (Alimardon, 2026-10-10: "no
# need keep old names/settings compatible for now"). The release that
# publishes Edel OS sets this to yes, and the rule holds from then on.
released=no

if [ "${GITHUB_EVENT_NAME:-pull_request}" != pull_request ]; then
	echo "PASS: keys are checked on pull requests; this is a $GITHUB_EVENT_NAME"
	exit 0
fi
git fetch --quiet --depth=1 origin main
if ! git diff FETCH_HEAD HEAD -- "$keys" | grep -q '^-[^-]'; then
	echo "PASS: no settings file key removed"
elif [ "$released" = no ]; then
	echo "PASS: keys removed or renamed before the first release, which nobody runs yet"
elif git diff FETCH_HEAD HEAD -- crates/edel/src/settings.rs | grep -q '^+pub const FORMAT'; then
	echo "PASS: keys removed together with a format bump"
else
	git diff FETCH_HEAD HEAD -- "$keys" | grep '^-[^-]'
	echo "FAIL: these lines left $keys without a bump of FORMAT in crates/edel/src/settings.rs"
	exit 1
fi
