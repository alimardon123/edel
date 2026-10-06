#!/bin/sh
# Checks the kernel of every bootable image in images/ against
# ci/kernel-options.toml, and lists the modules in each image's initramfs,
# failing when an lts image's (the desktop's) lacks a driver it needs to find its disk or
# light its screen (roadmap M3.3a), or an image with a screen lacks
# VirtualBox's (M3.6). Reads the built root filesystems in
# out/work/, which are root's, so it uses sudo.
set -eu

fail=0

# options FLAVOR: the option names ci/kernel-options.toml requires of FLAVOR.
options() {
	awk -v flavor="$1" '
		/^\[[A-Z0-9_]+\]$/ { if (name != "" && (all || mine)) print name; name = substr($0, 2, length($0) - 2); all = 1; mine = 0; next }
		/^kernels *=/ { all = 0; if (index($0, "\"" flavor "\"")) mine = 1 }
		END { if (name != "" && (all || mine)) print name }
	' ci/kernel-options.toml
}

# The modules an image's initramfs must hold, by its kernel flavour.
needed_modules() {
	case "$1" in
	lts) echo "nvme usb_storage xhci_pci ahci mmc_block sdhci_pci i915 xe amdgpu virtio_gpu ext4 overlay" ;;
	*) echo "virtio_blk ext4 overlay" ;;
	esac
}

# The firmware directories an image's initramfs must hold files from, by
# its kernel flavour: a graphics driver that loads there without its
# firmware fails and is not retried later (amdgpu and xe need it to start).
needed_firmware() {
	case "$1" in
	lts) echo "amdgpu i915 xe" ;;
	esac
}

for def in images/*.toml; do
	flavor=$(sed -n 's/^kernel = "\(.*\)"$/\1/p' "$def")
	[ -n "$flavor" ] || continue
	name=$(sed -n 's/^name = "\(.*\)"$/\1/p' "$def")
	arch=$(sed -n 's/^arch = "\(.*\)"$/\1/p' "$def")
	boot=out/work/$name-$arch/rootfs/boot
	config=$(sudo sh -c "ls $boot/config-*-$flavor" | head -n 1)
	missing=''
	for option in $(options "$flavor"); do
		sudo grep -qE "^CONFIG_$option=(y|m)\$" "$config" || missing="$missing $option"
	done
	# mkinitfs writes a gzip or zstd compressed cpio archive.
	unpack='gzip -dc'
	[ "$(sudo od -An -tx1 -N4 "$boot/initramfs-$flavor" | tr -d ' ')" = 28b52ffd ] && unpack='zstd -dc'
	files=$(sudo cat "$boot/initramfs-$flavor" | $unpack | cpio -t --quiet)
	modules=$(echo "$files" | sed -n 's|.*/\([^/]*\)\.ko\(\.[a-z]*\)\{0,1\}$|\1|p' | tr '-' '_' | sort -u)
	# Archive names may start with ./; a file's last part has a dot, a
	# directory's does not.
	firmware=$(echo "$files" | grep -cE '(^|/)lib/firmware/(.*/)?[^/]*\.[^/]+$' || true)
	absent=''
	for module in $(needed_modules "$flavor"); do
		echo "$modules" | grep -qx "$module" || absent="$absent $module"
	done
	for dir in $(needed_firmware "$flavor"); do
		echo "$files" | grep -qE "(^|/)lib/firmware/$dir/" || absent="$absent firmware/$dir"
	done
	# An image with a screen lights VirtualBox's and VMware's (VMSVGA)
	# early too, and carries Mesa's driver for it, svga, whose DRI name is
	# vmwgfx (M3.6).
	if grep -q '"graphics"' "$def"; then
		echo "$modules" | grep -qx vmwgfx || absent="$absent vmwgfx"
		root=out/work/$name-$arch/rootfs
		if sudo sh -c "grep -qa vmwgfx $root/usr/lib/libgallium-*.so $root/usr/lib/xorg/modules/dri/* 2>/dev/null"; then
			echo "$name: vmwgfx in the initramfs and Mesa's svga driver (vmwgfx) in the slot, for VirtualBox"
		else
			absent="$absent Mesa's-svga-driver"
		fi
	fi
	count=$(echo "$modules" | grep -c . || true)
	size=$(sudo du -h "$boot/initramfs-$flavor" | cut -f1)
	echo "$name ($flavor): initramfs $size with $count modules and $firmware firmware files, including$(for m in $(needed_modules "$flavor"); do echo "$modules" | grep -qx "$m" && printf ' %s' "$m"; done)"
	if [ -n "$missing" ]; then
		echo "FAIL: $name's kernel lacks$missing (see ci/kernel-options.toml)"
		fail=1
	fi
	if [ -n "$absent" ]; then
		echo "FAIL: $name's initramfs lacks$absent"
		fail=1
	fi
done
[ "$fail" = 0 ] && echo "PASS: every bootable image's kernel has the options our steps rely on"
exit "$fail"
