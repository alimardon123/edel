#!/bin/sh
# desktop-part.sh CASE... (roadmap M6.17): runs one part of the desktop
# tests, as CI's matrix does with each of its parts on a runner of its own.
# It calls desktop-test.sh with the part's cases. When that fails, it names
# the case that failed, keeps the first try's pictures, log and passed list
# under out/desktop-test/first/ and out/desktop-test-first.log, and runs
# the whole part once more from a fresh boot. The whole part runs again,
# not only the failed case, because cases rely on what earlier cases in the
# part left behind (windows, keys), so one case alone from a fresh boot
# would not be the same test. A part that passes the second time has a
# flaky case: a warning names it, and the step summary lists it. A part
# that fails twice exits 1. Run it from the repository root, as
# `sh ci/desktop-part.sh CASE...`.
set -eu

dir=out/desktop-test
log=out/desktop-test.log
first_log=out/desktop-test-first.log

if [ "$#" -eq 0 ]; then
	echo "usage: sh ci/desktop-part.sh CASE..." >&2
	exit 2
fi

if sh ci/desktop-test.sh "$@"; then
	exit 0
fi

# Name the case that failed, before the first try's files move. A boot
# that gave no results leaves no done line, and then no passed file either.
case_name=''
if [ ! -e "$log" ] || ! grep -q 'DESKTOP-TEST: done' "$log"; then
	case_name='the boot'
else
	for c in "$@"; do
		if ! grep -qx -- "$c" "$dir/passed" 2>/dev/null; then
			case_name=$c
			break
		fi
	done
	if [ -z "$case_name" ]; then
		case_name='the part'
	fi
fi

# Keep the first try. The second run deletes the pictures and the passed
# list, and overwrites the log.
rm -rf "$dir/first"
mkdir -p "$dir/first"
for f in "$dir"/*.png; do
	[ -e "$f" ] || continue
	mv "$f" "$dir/first/"
done
if [ -e "$dir/passed" ]; then
	cp "$dir/passed" "$dir/first/passed"
fi
if [ -e "$log" ]; then
	mv "$log" "$first_log"
fi

# desktop-test.sh stops its VM when a case fails, but an error under set -e
# may leave the VM running, so make sure no QEMU of the first try is left.
pkill -f 'qemu-system-x86_64.*edel-desktop-grown' || true
sleep 2

echo "== $case_name failed; running this part once more from a fresh boot"
if sh ci/desktop-test.sh "$@"; then
	echo "::warning title=Flaky desktop case::$case_name failed once and passed when the part ran again (first run: out/desktop-test-first.log, out/desktop-test/first/)"
	if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
		printf '### Flaky desktop cases\n\n- `%s`: failed once and passed when the part ran again\n' "$case_name" >>"$GITHUB_STEP_SUMMARY"
	fi
	exit 0
fi

echo "FAIL: $case_name failed, and the part failed again on its second run (first run: out/desktop-test-first.log)"
exit 1
