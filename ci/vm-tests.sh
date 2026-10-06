#!/bin/sh
# Runs the VM tests in two lanes at once, as CI does: the installed-disk
# lane (install-test.sh, then ab-test.sh, which boots the disk the first
# installed) and the other lane (boot-test.sh, system-test.sh,
# flatpak-test.sh, then `desktop-test.sh rollback`, which restarts the
# desktop VM, and `desktop-test.sh live`, which starts the released desktop
# image as a USB stick, so both run apart from the measured desktop cases). The lanes share no file, port or disk, and each runs
# its tests in order and stops at the first failure. The installed-disk
# lane, the longer one, prints as it goes; the other lane's output follows
# when both are done. Exits 1 if either lane failed. Budget boots are not
# here: sizes.sh measures them afterwards, alone, so other VMs do not
# slow the boots it times.
set -u

# lane SCRIPT...: runs each ci/SCRIPT in order until one fails; a SCRIPT
# may carry its arguments, as in 'desktop-test.sh rollback'.
lane() {
	for script in "$@"; do
		echo "== sh ci/$script"
		sh ci/$script || {
			echo "FAIL: sh ci/$script"
			return 1
		}
	done
}

mkdir -p out
lane boot-test.sh system-test.sh flatpak-test.sh 'desktop-test.sh rollback' 'desktop-test.sh live' >out/lane-other.log 2>&1 &
other=$!
lane install-test.sh ab-test.sh
installed=$?
wait "$other"
other=$?
echo "== the other lane: boot-test.sh, system-test.sh, flatpak-test.sh, desktop-test.sh rollback, desktop-test.sh live"
cat out/lane-other.log
[ "$installed" = 0 ] && [ "$other" = 0 ]
