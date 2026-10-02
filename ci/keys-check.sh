#!/bin/sh
# Fails when a change deletes a line of crates/edel/tests/keys.txt without
# bumping FORMAT in crates/edel/src/system.rs: a system file key is never
# removed or renamed within a format, because old slots still read the
# files newer ones write (ADR-008, roadmap M2.1). Compares with main.
set -eu

keys=crates/edel/tests/keys.txt
git fetch --quiet --depth=1 origin main
if ! git diff FETCH_HEAD HEAD -- "$keys" | grep -q '^-[^-]'; then
	echo "PASS: no system file key removed"
elif git diff FETCH_HEAD HEAD -- crates/edel/src/system.rs | grep -q '^+pub const FORMAT'; then
	echo "PASS: keys removed together with a format bump"
else
	git diff FETCH_HEAD HEAD -- "$keys" | grep '^-[^-]'
	echo "FAIL: these lines left $keys without a bump of FORMAT in crates/edel/src/system.rs"
	exit 1
fi
