#!/bin/sh
# Checks the kernel of every bootable image in images/ against
# ci/kernel-options.toml, and lists the modules in each image's initramfs,
# failing when an lts image's lacks a driver it needs to find its disk
# (roadmap M3.3a), carries a graphics driver (M8.17e: the root loads them)
# or is over its budget, or an image with a screen lacks VirtualBox's
# (M3.6), and checks that NVIDIA's driver travels in its pack (M8.39a).
# Reads the built root filesystems in out/work/, which are root's, so it
# uses sudo, and the packs' images in out/.
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
	lts) echo "nvme usb_storage xhci_pci ahci mmc_block sdhci_pci ext4 overlay" ;;
	*) echo "virtio_blk ext4 overlay" ;;
	esac
}

# The graphics drivers no initramfs may hold (M8.17e): the root loads them
# at the start, with their firmware, and the firmware's picture lights the
# screen until then. A driver here would start without its firmware. The
# virtual machines' (virtio_gpu, vmwgfx) need none and may stay, as the
# kernel feature's modules list them.
graphics="amdgpu i915 xe radeon nouveau"

. ci/budgets.sh

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
	extra=''
	for module in $graphics; do
		echo "$modules" | grep -qx "$module" && extra="$extra $module"
	done
	# An image with a screen carries VirtualBox's and VMware's (VMSVGA)
	# driver and Mesa's for it, svga, whose DRI name is vmwgfx (M3.6).
	if grep -q '"graphics"' "$def"; then
		root=out/work/$name-$arch/rootfs
		sudo find "$root/lib/modules/" -name 'vmwgfx.ko*' | grep -q . || absent="$absent vmwgfx"
		if sudo sh -c "grep -qa vmwgfx $root/usr/lib/libgallium-*.so $root/usr/lib/xorg/modules/dri/* 2>/dev/null"; then
			echo "$name: vmwgfx in the initramfs and Mesa's svga driver (vmwgfx) in the slot, for VirtualBox"
		else
			absent="$absent Mesa's-svga-driver"
		fi
	fi
	count=$(echo "$modules" | grep -c . || true)
	size=$(sudo du -h "$boot/initramfs-$flavor" | cut -f1)
	echo "$name ($flavor): initramfs $size with $count modules and $firmware firmware files, including$(for m in $(needed_modules "$flavor"); do echo "$modules" | grep -qx "$m" && printf ' %s' "$m"; done)"
	if [ "$flavor" = lts ]; then
		mib=$(sudo du -m "$boot/initramfs-$flavor" | cut -f1)
		limit=$(budget initramfs_mib)
		echo "budget initramfs_mib: $mib MiB, budget $limit MiB"
		if [ "$mib" -gt "$limit" ]; then
			echo "FAIL: $name's initramfs is $mib MiB, over its budget of $limit MiB (ci/budgets.toml)"
			fail=1
		fi
		if [ -n "$extra" ]; then
			echo "FAIL: $name's initramfs holds graphics drivers,$extra, which the root loads with their firmware (M8.17e)"
			fail=1
		fi
	fi
	if [ -n "$missing" ]; then
		echo "FAIL: $name's kernel lacks$missing (see ci/kernel-options.toml)"
		fail=1
	fi
	if [ -n "$absent" ]; then
		echo "FAIL: $name's initramfs lacks$absent"
		fail=1
	fi
done
# NVIDIA's pack (M8.39a): nouveau is in the pack and not in the slot,
# with its GSP option, and of the GSP firmware only the release nouveau
# loads first, so a machine without the card carries none of it.
for pack in out/*.graphics-nvidia.ext4.gz; do
	[ -e "$pack" ] || continue
	image=$(basename "$pack" .graphics-nvidia.ext4.gz)
	if sudo find "out/work/$image/rootfs/lib/modules/" -name 'nouveau.ko*' | grep -q .; then
		echo "FAIL: $image's slot holds nouveau, which its pack graphics-nvidia carries (M8.39a)"
		fail=1
	fi
	files=$(mktemp -d)
	gunzip -c "$pack" >"$files/pack.ext4"
	mkdir "$files/root"
	debugfs -R "rdump / $files/root" "$files/pack.ext4" 2>/dev/null
	found=$(find "$files/root" -name 'nouveau.ko*' | head -n 1)
	gsp=$(find "$files/root" -path '*/nvidia/*/gsp/*' -name 'gsp-*' | sed 's|.*/gsp-\([0-9]*\)\..*|\1|' | sort -u | tr '\n' ' ')
	if [ -z "$found" ]; then
		echo "FAIL: $image's pack graphics-nvidia lacks nouveau (M8.39a)"
		fail=1
	elif ! grep -qs 'options nouveau config=NvGspRm=1' "$files"/root/usr/lib/modprobe.d/nouveau.conf; then
		echo "FAIL: $image's pack graphics-nvidia lacks usr/lib/modprobe.d/nouveau.conf, which turns GSP on for Turing and Ampere (M8.39a)"
		fail=1
	elif [ "$(echo "$gsp" | wc -w)" != 1 ]; then
		echo "FAIL: $image's pack graphics-nvidia keeps GSP releases $gsp, one too many (M8.39a)"
		fail=1
	else
		echo "$image: nouveau in its pack graphics-nvidia ($(du -h "$found" | cut -f1)), GSP on, GSP release $gsp only; pack $(du -m "$files/pack.ext4" | cut -f1) MiB"
	fi
	rm -rf "$files"
done
[ "$fail" = 0 ] && echo "PASS: every bootable image's kernel has the options our steps rely on"
exit "$fail"
