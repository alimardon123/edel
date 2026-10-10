#!/bin/sh
# Fails when a change deletes a line of crates/edel/tests/keys.txt without
# bumping FORMAT in crates/edel/src/settings.rs: once Edel OS is released, a
# settings file key is never removed or renamed within a format, because
# old slots still read the files newer ones write (ADR-008, roadmap M2.1). Compares with main, so
# it runs for pull requests (and by hand) only: a push to main was checked
# as its pull request, and a tag may sit behind main, whose newer keys
# would look removed.
set -eu

# Until the first public release nobody runs Edel OS, so a key may be
# renamed or dropped with no old name kept (Alimardon, 2026-10-10: "no
# need keep old names/settings compatible for now"). The release that
# publishes Edel OS sets this to yes, and the rule holds from then on.
released=no

if [ "${GITHUB_EVENT_NAME:-pull_request}" != pull_request ]; then
	echo "PASS: keys are checked on pull requests; this is a $GITHUB_EVENT_NAME"
	exit 0
fi
keys=crates/edel/tests/keys.txt
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
