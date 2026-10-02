#!/bin/sh
# Roadmap M2.2: boots the system test image, whose slot carries a seed
# system file (made by ci/build.sh) that names the machine ci-seeded and
# adds user ci with a fresh ssh key. Passes when the first boot reaches the
# login prompt under that hostname and ci logs in over ssh and exports the
# machine as a system file.
set -eu
. ci/vm.sh

log=out/system-test.log
keep_vm=1
vm_net=",hostfwd=tcp:127.0.0.1:2222-:22"
run_vm "$log" 'ci-seeded login:' "${BOOT_TIMEOUT:-300}" -no-reboot -snapshot \
	-drive if=none,id=disk0,format=raw,file=out/system-test/edel-vm-x86_64.img \
	-device virtio-blk-pci,drive=disk0,bootindex=0
trap stop_vm EXIT
if [ "$found" != 1 ]; then
	cat "$log"
	echo "FAIL: the first boot did not reach the login prompt as ci-seeded"
	exit 1
fi

ssh_ci() {
	ssh -i out/keys/ci-ssh -p 2222 -o BatchMode=yes -o ConnectTimeout=10 \
		-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null ci@127.0.0.1 "$@"
}
tries=0
until ssh_ci edel system export >out/system-export.toml 2>out/ssh.log; do
	tries=$((tries + 1))
	if [ "$tries" -ge 10 ]; then
		cat "$log" out/ssh.log
		echo "FAIL: ci could not log in over ssh and run edel system export"
		exit 1
	fi
	sleep 3
done
cat "$log" out/system-export.toml
if grep -qx 'hostname = "ci-seeded"' out/system-export.toml &&
	grep -qx '\[users.ci\]' out/system-export.toml &&
	grep -qx 'admin = true' out/system-export.toml; then
	echo "PASS: seeded on first boot as ci-seeded; ci logged in with its key and exported the machine (${waited}s to the login prompt)"
else
	echo "FAIL: the export lacks the seeded hostname or the admin user ci"
	exit 1
fi
