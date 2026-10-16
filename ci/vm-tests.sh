#!/bin/sh
# Runs the VM tests in lanes. CI runs each lane on a runner of its own, fed
# the images the build job made as one archive per lane (CI speed-ups,
# 2026-10-16), so the lanes run at once and none slows another:
#   installed  install-test.sh, ab-test.sh (it boots the disk the first
#              installed), then apps-test.sh, the default apps across an
#              update
#   system     boot-test.sh, system-test.sh, flatpak-test.sh, then
#              `desktop-test.sh rollback`, which restarts the desktop VM
#   live       `desktop-test.sh live`, which starts the released desktop
#              image as a USB stick
# Each lane runs its tests in order and stops at the first failure.
#
#   sh ci/vm-tests.sh             every lane on this machine: installed beside
#                                 system then live, which share no file, port
#                                 or disk; exits 1 if any failed
#   sh ci/vm-tests.sh LANE        one lane, as each CI job runs it
#   sh ci/vm-tests.sh lanes       the lanes' names, one per line
#   sh ci/vm-tests.sh pack LANE   what LANE needs from out/ into
#                                 out/vm-LANE.tar.zst (sparse, without the
#                                 builds' work/ folders)
#   sh ci/vm-tests.sh unpack LANE that archive back into out/
#
# Budget boots are not here: sizes.sh measures them in the build job, alone,
# so other VMs do not slow the boots it times.
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

# run_lane LANE: the lane's tests, in order.
run_lane() {
	case "$1" in
	installed) lane install-test.sh ab-test.sh apps-test.sh ;;
	system) lane boot-test.sh system-test.sh flatpak-test.sh 'desktop-test.sh rollback' ;;
	live) lane 'desktop-test.sh live' ;;
	*) no_lane "$1" ;;
	esac
}

# files LANE: what the lane's tests read from out/, as build.sh made it.
files() {
	case "$1" in
	installed) echo out/install-test out/ab-test out/apps-repo out/apps-test ;;
	system) echo out/edel-vm-x86_64.img out/keys out/system-test out/flatpak out/desktop-test ;;
	live) echo out/edel-desktop-x86_64.img out/install-file.img ;;
	*) no_lane "$1" ;;
	esac
}

no_lane() {
	echo "vm-tests.sh: there is no lane $1; the lanes are installed, system and live" >&2
	exit 2
}

case "${1:-}" in
lanes)
	echo installed
	echo system
	echo live
	;;
pack)
	# sudo: the builds' work/ folders are root's, and tar walks past them.
	paths=$(files "$2")
	# shellcheck disable=SC2086
	sudo tar --sparse --exclude='out/*/work' -cf - $paths | zstd -q -T0 -1 -o "out/vm-$2.tar.zst"
	echo "packed lane $2: $(du -h "out/vm-$2.tar.zst" | cut -f1) for $paths"
	;;
unpack)
	zstd -q -dc "out/vm-$2.tar.zst" | tar -xf -
	rm "out/vm-$2.tar.zst"
	;;
'')
	mkdir -p out
	{ run_lane system && run_lane live; } >out/lane-other.log 2>&1 &
	other=$!
	run_lane installed
	installed=$?
	wait "$other"
	other=$?
	echo "== the other lanes: system, then live"
	cat out/lane-other.log
	[ "$installed" = 0 ] && [ "$other" = 0 ]
	;;
*)
	files "$1" >/dev/null
	mkdir -p out
	run_lane "$1"
	;;
esac
