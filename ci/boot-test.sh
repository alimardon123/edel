#!/bin/sh
# Boots the VM image in QEMU and waits for the Edel OS login prompt on the
# serial console. The disk is opened in snapshot mode, so the tested image
# stays exactly as built.
set -eu

img=out/edel-vm-x86_64
log=out/boot.log
timeout_s=${BOOT_TIMEOUT:-300}

accel=tcg
[ -w /dev/kvm ] && accel=kvm
echo "booting with $accel, waiting up to ${timeout_s}s"

rm -f "$log"
qemu-system-x86_64 \
	-machine q35,accel="$accel" -m 512 -smp 2 \
	-display none -monitor none -serial file:"$log" -no-reboot \
	-kernel "$img.vmlinuz" -initrd "$img.initramfs" \
	-append "root=/dev/vda rootfstype=ext4 modules=ext4,virtio_pci,virtio_blk rw console=ttyS0" \
	-drive file="$img.ext4",if=virtio,format=raw -snapshot \
	-nic user,model=virtio-net-pci &
qemu=$!

ok=0
i=0
while [ "$i" -lt "$timeout_s" ]; do
	if grep -q 'edel login:' "$log" 2>/dev/null; then
		ok=1
		break
	fi
	kill -0 "$qemu" 2>/dev/null || break
	sleep 1
	i=$((i + 1))
done
kill "$qemu" 2>/dev/null || true
wait "$qemu" 2>/dev/null || true

cat "$log"
if [ "$ok" = 1 ] && grep -q 'Welcome to Edel OS' "$log"; then
	echo "PASS: VM booted to the Edel OS login prompt in ${i}s"
else
	echo "FAIL: no Edel OS login prompt"
	exit 1
fi
