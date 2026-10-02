#!/bin/sh
# Tests A/B updates end to end, across several restarts of one VM: updates
# install, roll back by command, and broken, hanging and frozen updates fall
# back on their own. The steps run inside the VM (ci/ab-test/files); this
# script serves the signed update over HTTP (a release.toml beside the
# gzipped image, made by ci/build.sh) and reads the verdict from the serial
# console. Snapshot mode keeps the disk exactly as built.
set -eu
. ci/vm.sh

dir=out/ab-test
log=out/ab-test.log
# Serve the signed update over HTTP; the VM reaches this host as 10.0.2.2.
python3 -m http.server 8000 --bind 127.0.0.1 --directory "$dir/update" >out/http.log 2>&1 &
server=$!
trap 'kill "$server" 2>/dev/null || true' EXIT

run_vm "$log" 'AB-TEST: (PASS|FAIL)' "${AB_TEST_TIMEOUT:-1500}" -snapshot \
	-drive if=none,id=disk0,format=raw,file="$dir/edel-vm-x86_64.img" \
	-device virtio-blk-pci,drive=disk0,bootindex=0
cat "$log"
if [ "$found" = 1 ] && grep -q 'AB-TEST: PASS' "$log"; then
	echo "PASS: update, restart and automatic fallback work (${waited}s)"
else
	echo "FAIL: see the serial log above"
	exit 1
fi
