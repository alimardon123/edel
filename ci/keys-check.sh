#!/bin/sh
# Fails when a change deletes a line of crates/edel/tests/keys.txt without
# bumping FORMAT in crates/edel/src/system.rs: a settings file key is never
# removed or renamed within a format, because old slots still read the
# files newer ones write (ADR-008, roadmap M2.1). Compares with main, so
# it runs for pull requests (and by hand) only: a push to main was checked
# as its pull request, and a tag may sit behind main, whose newer keys
# would look removed.
set -eu

if [ "${GITHUB_EVENT_NAME:-pull_request}" != pull_request ]; then
	echo "PASS: keys are checked on pull requests; this is a $GITHUB_EVENT_NAME"
	exit 0
fi
keys=crates/edel/tests/keys.txt
git fetch --quiet --depth=1 origin main
if ! git diff FETCH_HEAD HEAD -- "$keys" | grep -q '^-[^-]'; then
	echo "PASS: no settings file key removed"
elif git diff FETCH_HEAD HEAD -- crates/edel/src/system.rs | grep -q '^+pub const FORMAT'; then
	echo "PASS: keys removed together with a format bump"
else
	git diff FETCH_HEAD HEAD -- "$keys" | grep '^-[^-]'
	echo "FAIL: these lines left $keys without a bump of FORMAT in crates/edel/src/system.rs"
	exit 1
fi
