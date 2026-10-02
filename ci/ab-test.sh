#!/bin/sh
# Tests A/B updates end to end on a freshly installed disk (roadmap M2.5),
# across several restarts of one VM: updates install, roll back by
# command, and broken, hanging and frozen updates fall back on their own.
# The disk is the one ci/install-test.sh installed; its first boot must
# apply the system file's hostname, confirm slot A and make its own ssh
# host key. The steps run inside the VM (ci/ab-test/files); this script
# serves the signed update over HTTP (a release.toml beside the gzipped
# image, made by ci/build.sh) and reads the verdict from the serial
# console. Snapshot mode keeps the installed disk as it was.
set -eu
. ci/vm.sh

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
	grep -q 'edel system: network.hostname: set to ci-installed' "$log" &&
		grep -q 'edel update: slot A confirmed' "$log" &&
		[ -n "$installed_key" ] && [ "$installed_key" != "$(cat out/install-live-key)" ]
}
if ! first_boot; then
	echo "FAIL: the installed disk's first boot lacked its hostname, the confirmation of slot A or its own ssh host key"
	exit 1
fi
if [ "$found" = 1 ] && grep -q 'AB-TEST: PASS' "$log"; then
	echo "PASS: the installed disk booted as ci-installed with its own host key; update, restart and automatic fallback work (${waited}s)"
else
	echo "FAIL: see the serial log above"
	exit 1
fi
