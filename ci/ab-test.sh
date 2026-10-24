#!/bin/sh
# Tests A/B updates end to end on a freshly installed disk (roadmap M2.5),
# across several restarts of one VM: updates install, roll back by
# command, and broken, hanging and frozen updates fall back on their own.
# The disk is the one ci/install-test.sh installed; its first boot must
# apply the settings file's hostname, confirm slot A and make its own ssh
# host key. The steps run inside the VM (ci/ab-test/files); this script
# serves the signed update over HTTP (a release.toml beside the gzipped
# image, made by ci/build.sh) and reads the verdict from the serial
# console. Snapshot mode keeps the installed disk as it was.
set -eu
. ci/vm.sh
. ci/budgets.sh

dir=out/ab-test
log=out/ab-test.log
# Serve the signed update over HTTP; the VM reaches this host as 10.0.2.2.
python3 -m http.server 8000 --bind 127.0.0.1 --directory "$dir/update" >out/http.log 2>&1 &
server=$!
trap 'kill "$server" 2>/dev/null || true' EXIT

run_vm "$log" 'AB-TEST: (PASS|FAIL)' "${AB_TEST_TIMEOUT:-1500}" -snapshot \
	-drive if=none,id=disk0,format=raw,file=out/install-target.img \
	-device virtio-blk-pci,drive=disk0,bootindex=0
cat "$log"
installed_key=$(tr -d '\r' <"$log" | sed -n 's/.*INSTALL-TEST: host key //p' | head -n 1)
first_boot() {
	grep -q 'edel settings: network.hostname: set to ci-installed' "$log" &&
		grep -q 'edel update: slot A confirmed' "$log" &&
		[ -n "$installed_key" ] && [ "$installed_key" != "$(cat out/install-live-key)" ]
}
if ! first_boot; then
	echo "FAIL: the installed disk's first boot lacked its hostname, the confirmation of slot A or its own ssh host key"
	exit 1
fi
# M6.17: slot B's root was on no partition; Alpine's initramfs said so
# and, under panic=, exited instead of waiting in its rescue shell, so the
# kernel panicked and restarted.
if ! tr -d '\r' <"$log" | grep -q 'Mounting root: failed' ||
	! tr -d '\r' <"$log" | grep -q 'Attempted to kill init'; then
	echo "FAIL: slot B with no root did not fail to mount it and panic, which restarts the machine"
	exit 1
fi
# M1.10: slot B's hang ran with no watchdog, so the guard restarted it.
if ! grep -q 'edel guard: this version of Edel OS did not finish starting in .*; restarting the computer now' "$log"; then
	echo "FAIL: with no watchdog, the guard did not restart the hung slot B itself"
	exit 1
fi
# M1.11: slot B named its own health check, network, which never passed.
if ! tr -d '\r' <"$log" | grep -q 'edel guard: health not reached after [0-9]* s: network$'; then
	echo "FAIL: slot B's guard did not wait for its own health check, network, alone"
	exit 1
fi
# M6.17: with no slot left with a try, slot B's guard kept the machine
# running instead of restarting it.
if ! tr -d '\r' <"$log" | grep -q 'edel guard: Edel OS did not finish starting in [0-9]* s (network did not start), and no version has a start left, so the computer keeps running'; then
	echo "FAIL: with no slot left with a try, slot B's guard did not say it keeps the machine running"
	exit 1
fi
if [ "$found" = 1 ] && grep -q 'AB-TEST: PASS' "$log"; then
	# The walks a newcomer takes (M8.15a): the guest timed each one, the
	# update and the rollback across restarts, and the broken update's fall
	# back, which the guest's clock alone measures (docs/REVIEW-experience.md).
	walks=ok
	for w in update rollback broken-update; do
		set -- $(tr -d '\r' <"$log" | sed -n "s/.*AB-TEST: walk $w //p" | tail -n 1)
		walk "$w" "seconds=${1:-}" "steps=${2:-}" || walks=over
	done
	if [ "$walks" != ok ]; then
		echo "FAIL: a walk of the A/B test is over its budget or was not timed (docs/REVIEW-experience.md)"
		exit 1
	fi
	echo "PASS: the installed disk booted as ci-installed with its own host key; update, restart and automatic fallback work, with no watchdog too, with no root and on an image's own health check, and with no try left the machine stayed up (${waited}s)"
else
	echo "FAIL: see the serial log above"
	exit 1
fi
