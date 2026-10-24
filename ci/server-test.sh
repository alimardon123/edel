#!/bin/sh
# The server (roadmap M7.4): boots out/server-test/ (ci/server/vm.toml, the
# containers add-on carried) with no screen (-vga none) and the EDEL-SEED
# volume build.sh made, out/server-test/seed.img: the server's settings,
# admin ci with the system test's key, ssh on and the containers add-on.
# Over ssh as ci it runs doas edel status (the admin rule, M6.5), imports our own container image
# (out/edel-container-x86_64.tar.gz, copied in, no network) and runs it,
# and wants the containers' storage on /data. Then it boots a fresh copy
# with no seed as an AWS instance (-smbios), the user network moved to
# 169.254.169.0/24 so the guest's 169.254.169.254:80 reaches a stand-in for
# the metadata address (ci/server/metadata.py), which serves IMDSv2's token
# and out/server-test/user-data.toml; the machine must keep it, restart once
# and start as ci-cloud. Logs in out/server-test.log, out/server-cloud.log,
# out/server-ssh.log and out/server-metadata.log.
set -eu
. ci/vm.sh

dir=out/server-test
disk=$dir/edel-server-x86_64.img
log=out/server-test.log
fail() {
	[ ! -f "$1" ] || cat "$1"
	shift
	echo "FAIL: $*"
	exit 1
}

keep_vm=1
vm_net=",hostfwd=tcp:127.0.0.1:2224-:22"
run_vm "$log" 'ci-server login:' "${BOOT_TIMEOUT:-300}" -no-reboot -snapshot -vga none \
	-drive if=none,id=disk0,format=raw,file="$disk" \
	-device virtio-blk-pci,drive=disk0,bootindex=0 \
	-drive if=none,id=seed,format=raw,file="$dir/seed.img" \
	-device virtio-blk-pci,drive=seed
trap stop_vm EXIT
[ "$found" = 1 ] || fail "$log" "the seeded server did not reach the login prompt as ci-server"

ssh_opts='-i out/keys/ci-ssh -o BatchMode=yes -o ConnectTimeout=10 -o LogLevel=ERROR -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null'
ssh_ci() {
	# shellcheck disable=SC2086
	ssh $ssh_opts -p 2224 ci@127.0.0.1 "$@"
}
tries=0
until ssh_ci doas edel status >out/server-status.txt 2>out/server-ssh.log; do
	tries=$((tries + 1))
	[ "$tries" -lt 10 ] || fail out/server-ssh.log "ci could not log in over ssh with the seeded key and run doas edel status"
	sleep 3
done
cat out/server-status.txt
# shellcheck disable=SC2086
scp $ssh_opts -P 2224 out/edel-container-x86_64.tar.gz ci@127.0.0.1:/tmp/edel-container.tar.gz \
	2>>out/server-ssh.log || fail out/server-ssh.log "could not copy the container image in"
ssh_ci 'podman import /tmp/edel-container.tar.gz edel:ci' >>out/server-ssh.log 2>&1 ||
	fail out/server-ssh.log "podman could not import our own container image"
out=$(ssh_ci 'podman run --rm edel:ci echo ok' 2>>out/server-ssh.log) || true
[ "$out" = ok ] || fail out/server-ssh.log "podman run --rm edel:ci echo ok printed '${out:-nothing}'"
# Containers' storage: ci's (rootless) and the system's /var/lib, both on
# /data's file system, so they survive updates.
store=$(ssh_ci 'podman info --format "{{.Store.GraphRoot}}"' 2>>out/server-ssh.log) ||
	fail out/server-ssh.log "podman info did not say where its storage is"
same=$(ssh_ci "[ \"\$(stat -c %d '$store')\" = \"\$(stat -c %d /data)\" ] && [ \"\$(stat -c %d /var/lib)\" = \"\$(stat -c %d /data)\" ] && echo yes; df -P /var/lib" 2>>out/server-ssh.log) || true
echo "$same"
case $same in
yes*) ;;
*) fail out/server-ssh.log "the containers' storage ($store) or /var/lib is not on /data" ;;
esac
echo "PASS: the seeded server took ci's key over ssh, ran doas edel status and our own container, its storage on /data (${waited}s to the login prompt)"
stop_vm
trap - EXIT

# The cloud: QEMU runs ci/server/metadata.py, a stand-in for AWS's metadata
# address (IMDSv2 only), for each connection to 169.254.169.254:80.
rm -f out/server-metadata.log
log=out/server-cloud.log
keep_vm=0
vm_net=",net=169.254.169.0/24,guestfwd=tcp:169.254.169.254:80-cmd:python3 ci/server/metadata.py $dir/user-data.toml out/server-metadata.log"
run_vm "$log" 'ci-cloud login:' "${BOOT_TIMEOUT:-300}" -snapshot -vga none \
	-smbios type=1,manufacturer="Amazon EC2",product=t3.micro \
	-drive if=none,id=disk0,format=raw,file="$disk" \
	-device virtio-blk-pci,drive=disk0,bootindex=0
cat out/server-metadata.log 2>/dev/null || echo 'the metadata address was never asked'
[ "$found" = 1 ] || fail "$log" "the instance did not start as ci-cloud from its user data"
tr -d '\r' <"$log" | grep -q "edel boot cloud: kept AWS's user data as" ||
	fail "$log" "edel boot cloud did not say it kept the user data"
cat "$log"
echo "PASS: the instance read AWS's user data with an IMDSv2 token, kept it as its settings file and started as ci-cloud (${waited}s)"
