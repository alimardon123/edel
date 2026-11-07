#!/bin/sh
# Keeps a GitHub runner's disk from filling up. On 2026-11-03, two runs
# ran out of disk in one hour: one while a Rust job restored its 2.5 GB
# cache, the other while a build compressed an add-on. Each job had
# passed on main shortly before, with the same caches, so the runners
# differ in how much they have free. The runner image carries toolchains
# no job of ours uses (.NET, Android, Haskell, CodeQL, Boost). This
# script removes them, but only when a job would otherwise be short of
# space, so a roomy runner loses no time.
# Usage: sh ci/disk.sh free GIB   prints the free space, and removes the
#                                 unused toolchains when / has less
#                                 than GIB GiB free
#        sh ci/disk.sh report     prints the free space and adds it to
#                                 the job's summary
set -eu

unused="/usr/local/lib/android /usr/share/dotnet /opt/ghc /usr/local/.ghcup /opt/hostedtoolcache/CodeQL /usr/local/share/boost"

# Free GiB on the root file system, rounded down.
free_gib() {
	df -Pk / | awk 'NR == 2 { print int($4 / 1048576) }'
}

case "${1:-}" in
free)
	want=${2:?usage: sh ci/disk.sh free GIB}
	have=$(free_gib)
	echo "disk: $have GiB free on /, this job wants $want"
	if [ "$have" -lt "$want" ]; then
		start=$(date +%s)
		# shellcheck disable=SC2086 # the list is split on purpose
		sudo rm -rf $unused
		echo "disk: removed the toolchains no job uses in $(($(date +%s) - start)) s, now $(free_gib) GiB free"
	fi
	;;
report)
	line="Disk at the end of ${GITHUB_JOB:-this job}: $(free_gib) GiB free on /"
	echo "$line"
	if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
		echo "$line" >>"$GITHUB_STEP_SUMMARY"
	fi
	;;
*)
	echo "usage: sh ci/disk.sh free GIB | report" >&2
	exit 2
	;;
esac
