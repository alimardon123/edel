#!/bin/sh
# Unattended updates (roadmap M7.5): boots out/updates-test/ with no screen
# (-vga none) and its seed, updates.automatic = "install-and-restart" with
# no window, while this host serves the A/B test's signed update
# (out/ab-test/update/, version VERSION.1) on 10.0.2.2:8000. The image's
# crontab runs the hourly job every minute, so with no command typed the
# machine must install the update into slot B, say it restarts, and start
# slot B and confirm it. Logs in out/updates-test.log.
set -eu
. ci/vm.sh

log=out/updates-test.log
python3 -m http.server 8000 --bind 127.0.0.1 --directory out/ab-test/update >out/updates-http.log 2>&1 &
server=$!
trap 'kill "$server" 2>/dev/null || true' EXIT

run_vm "$log" 'edel update: slot B confirmed' "${UPDATES_TEST_TIMEOUT:-600}" -snapshot -vga none \
	-drive if=none,id=disk0,format=raw,file=out/updates-test/edel-vm-x86_64.img \
	-device virtio-blk-pci,drive=disk0,bootindex=0
log_has() {
	tr -d '\r' <"$log" | grep -q "$1"
}
fail() {
	cat "$log"
	echo "FAIL: $*"
	exit 1
}
log_has 'edel update: slot A confirmed' || fail "the first boot did not confirm slot A"
log_has 'edel update: slot B written and checked' || fail "nothing installed the update into slot B by itself"
log_has 'edel update: restarting to start the update' || fail "the machine did not say it restarts for the update"
[ "$found" = 1 ] || fail "slot B did not start and confirm after the restart"
echo "PASS: with updates.automatic = install-and-restart the machine installed the update, restarted by itself and confirmed slot B (${waited}s)"
