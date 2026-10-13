#!/bin/sh
# Tests the default apps across an update (roadmap M6.2): boots
# out/apps-test/, whose default app is org.edel.TestA, serves the apps'
# repository (out/apps-repo, ci/apps/make-repo.sh) on port 8002 and the
# signed update, whose default app is org.edel.TestB and which drops
# TestA, on port 8003, and reads the verdict of the test service
# (ci/apps-test/features/apps-test) from the serial console. The VM
# restarts once, into slot B. It boots a sparse copy 2 GiB larger, so the
# data partition, which grows to the disk's end at the first boot, has
# room for the apps the update installs; the copy is removed at the end.
set -eu
. ci/vm.sh

dir=out/apps-test
log=out/apps-test.log
python3 -m http.server 8002 --bind 127.0.0.1 --directory out/apps-repo >out/apps-http.log 2>&1 &
apps=$!
python3 -m http.server 8003 --bind 127.0.0.1 --directory "$dir/update" >out/apps-update-http.log 2>&1 &
update=$!
grown="$dir/edel-apps-test-grown.img"
cp --sparse=always "$dir/edel-apps-test-x86_64.img" "$grown"
truncate -s +2G "$grown"
trap 'kill "$apps" "$update" 2>/dev/null || true; rm -f "$grown"' EXIT

run_vm "$log" 'APPS-TEST: (PASS|FAIL)' "${APPS_TEST_TIMEOUT:-600}" \
	-drive if=none,id=disk0,format=raw,file="$grown" \
	-device virtio-blk-pci,drive=disk0,bootindex=0
tr -d '\r' <"$log" | grep -a 'APPS-TEST:' || true
if ! grep -q 'APPS-TEST: PASS' "$log"; then
	cat "$log"
	echo "FAIL: the default apps did not follow the update; the lines above say where"
	exit 1
fi
echo "PASS: an update that swapped default app TestA for TestB installed TestB, kept TestA for the person who used it and gave a new person only TestB"
