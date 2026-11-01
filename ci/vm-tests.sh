#!/bin/sh
# Runs the VM tests in lanes. CI runs each lane on a runner of its own, fed
# the images the build job made as one archive per lane (CI speed-ups,
# 2026-10-16), so the lanes run at once and none slows another:
#   installed  install-test.sh, ab-test.sh (it boots the disk the first
#              installed), then apps-test.sh, the default apps across an
#              update, then updates-test.sh, an update with nobody typing
#              (M7.5)
#   system     boot-test.sh, system-test.sh, developer-test.sh (M7.1),
#              flatpak-test.sh, then
#              `desktop-test.sh rollback`, which restarts the desktop VM
#   live       `desktop-test.sh live`, which starts the released desktop
#              image as a USB stick
#   fresh      `desktop-test.sh fresh`, the released image as a computer's
#              own disk with nobody on it; it reads live's archive
#   fleet      fleet-test.sh, the fleet images and the stick that installs
#              itself (M7.3), then server-test.sh, the seeded server and
#              the cloud instance (M7.4), then upgrade-test.sh, an old
#              release's disk updated to this build (M3.5)
# Each lane runs its tests in order and stops at the first failure.
#
#   sh ci/vm-tests.sh             every lane on this machine: installed beside
#                                 system, live, fresh and fleet, which share no file,
#                                 port or disk; exits 1 if any failed
#   sh ci/vm-tests.sh LANE        one lane, as each CI job runs it
#   sh ci/vm-tests.sh lanes       the lanes' names, one per line
#   sh ci/vm-tests.sh archives    the archives the build packs, one per line
#   sh ci/vm-tests.sh archive LANE  the archive LANE reads
#   sh ci/vm-tests.sh pack NAME   what archive NAME holds from out/ into
#                                 out/vm-NAME.tar.zst (sparse, without the
#                                 builds' work/ folders)
#   sh ci/vm-tests.sh take NAME RUN  that archive from run RUN's artifact,
#                                 checked against what was uploaded, unpacked
#                                 back into out/
#
# Budget boots are not here: CI's system lane runs `sizes.sh boots` before
# its tests, alone on its runner, so other VMs do not slow the boots it times.
set -u

# free: the free disk where the lane writes, so a runner that runs out
# can be told from a test that fills it (on 2026-11-01 a live lane's
# runner ran out of disk and its log was lost with it).
free() {
	df -Pk . | awk 'NR == 2 { printf "%.1f GiB", $4 / 1048576 }'
}

# lane SCRIPT...: runs each ci/SCRIPT in order until one fails; a SCRIPT
# may carry its arguments, as in 'desktop-test.sh rollback'.
lane() {
	for script in "$@"; do
		echo "== sh ci/$script (free disk: $(free))"
		sh ci/$script || {
			echo "FAIL: sh ci/$script (free disk: $(free))"
			return 1
		}
	done
	echo "== the lane's tests ended (free disk: $(free))"
}

# run_lane LANE: the lane's tests, in order.
run_lane() {
	case "$1" in
	installed) lane install-test.sh ab-test.sh apps-test.sh updates-test.sh ;;
	system) lane boot-test.sh system-test.sh developer-test.sh flatpak-test.sh 'desktop-test.sh rollback' ;;
	live) lane 'desktop-test.sh live' ;;
	fresh) lane 'desktop-test.sh fresh' ;;
	fleet) lane fleet-test.sh server-test.sh upgrade-test.sh ;;
	*) no_lane "$1" ;;
	esac
}

# archive LANE: the archive holding what the lane reads; fresh needs
# only the desktop image, which live's carries, so it is not packed twice.
archive() {
	case "$1" in
	fresh) echo live ;;
	*)
		files "$1" >/dev/null
		echo "$1"
		;;
	esac
}

# files LANE: what the lane's tests read from out/, as build.sh made it.
files() {
	case "$1" in
	installed) echo out/install-test out/ab-test out/apps-repo out/apps-test out/updates-test ;;
	system) echo out/edel-x86_64.img out/keys out/system-test out/developer-test out/flatpak out/desktop-test ;;
	live) echo out/edel-x86_64.img out/install-file.img ;;
	fleet) echo out/fleet-a out/fleet-b out/fleet-seed.img out/server-test out/upgrade-test out/keys out/edel-container-x86_64.tar.gz ;;
	*) no_lane "$1" ;;
	esac
}

no_lane() {
	echo "vm-tests.sh: there is no lane $1; the lanes are installed, system, live, fresh and fleet" >&2
	exit 2
}

case "${1:-}" in
lanes)
	echo installed
	echo system
	echo live
	echo fresh
	echo fleet
	;;
archives)
	for name in $(sh "$0" lanes); do
		if [ "$(archive "$name")" = "$name" ]; then echo "$name"; fi
	done
	;;
archive)
	archive "$2"
	;;
pack)
	# sudo: the builds' work/ folders are root's, and tar walks past them.
	paths=$(files "$2")
	# shellcheck disable=SC2086
	sudo tar --sparse --exclude='out/*/work' -cf - $paths | zstd -q -T0 -1 -o "out/vm-$2.tar.zst"
	echo "packed lane $2: $(du -h "out/vm-$2.tar.zst" | cut -f1) for $paths"
	;;
take)
	# take NAME RUN: archive NAME from the artifact edel-vm-images-NAME of
	# run RUN, through the REST API with GH_TOKEN. actions/download-artifact
	# ended a few downloads early with no error (2026-10-30 and 31, 3 of
	# about 57 lane jobs, the 2 to 6 GB archives), so this one resumes a
	# download that stops, up to five times, and takes the archive only
	# when its size and sha256 are those the build job uploaded.
	api=https://api.github.com/repos/$GITHUB_REPOSITORY/actions
	meta=$(curl -fsSL --retry 5 -H "Authorization: Bearer $GH_TOKEN" "$api/runs/$3/artifacts?name=edel-vm-images-$2" |
		jq -r '.artifacts[0] | "\(.id) \(.size_in_bytes) \(.digest)"')
	[ -n "$meta" ] && [ "$meta" != "null null null" ] || {
		echo "FAIL: run $3 has no artifact edel-vm-images-$2"
		exit 1
	}
	# shellcheck disable=SC2086
	set -- "$2" $meta
	zip=out/vm-$1.zip
	mkdir -p out
	rm -f "$zip"
	n=0
	while [ "$(stat -c %s "$zip" 2>/dev/null || echo 0)" != "$3" ] && [ "$n" -lt 5 ]; do
		n=$((n + 1))
		curl -fsSL -C - -H "Authorization: Bearer $GH_TOKEN" -o "$zip" "$api/artifacts/$2/zip" ||
			echo "download $n of edel-vm-images-$1 stopped at $(stat -c %s "$zip" 2>/dev/null || echo 0) of $3 bytes; going on from there"
	done
	sum=$(sha256sum "$zip" | cut -d' ' -f1)
	if [ "sha256:$sum" != "$4" ]; then
		echo "FAIL: edel-vm-images-$1 came $(stat -c %s "$zip") bytes long with sha256 $sum after $n tries; the build job uploaded $3 bytes with $4"
		exit 1
	fi
	echo "took edel-vm-images-$1: $3 bytes in $n tries, sha256 as uploaded"
	unzip -p "$zip" "vm-$1.tar.zst" | zstd -q -dc | tar -xf - || {
		echo "FAIL: edel-vm-images-$1 matched its sha256 yet did not unpack"
		exit 1
	}
	rm "$zip"
	;;
'')
	mkdir -p out
	{ run_lane system && run_lane live && run_lane fresh && run_lane fleet; } >out/lane-other.log 2>&1 &
	other=$!
	run_lane installed
	installed=$?
	wait "$other"
	other=$?
	echo "== the other lanes: system, live, fresh, then fleet"
	cat out/lane-other.log
	[ "$installed" = 0 ] && [ "$other" = 0 ]
	;;
*)
	archive "$1" >/dev/null
	mkdir -p out
	run_lane "$1"
	;;
esac
