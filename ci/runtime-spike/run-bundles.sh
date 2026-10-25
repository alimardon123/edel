#!/bin/sh
# run-bundles.sh (M10.9), in an alpine container standing for the machine: runs every bundle under
# /out/bundles/*/ in a bubblewrap sandbox on a headless weston. The machine gives only musl's loader
# and its graphics drivers; a library both the bundle and the drivers carry is taken from whichever
# is newer, as Valve's Steam runtime does. Prints one RESULT line per run.
set -u
. /scripts/common.sh
v=$(cut -d. -f1,2 /etc/alpine-release)
apk add -q bubblewrap mesa-dri-gallium mesa-egl mesa-gbm mesa-gl mesa-gles >/dev/null
start_weston
# the frozen graphics part: mesa's libraries and every library they load, but musl
g=/tmp/gl; mkdir -p $g/lib/dri $g/lib/gbm
for p in $(apk info -q | grep -E '^(mesa(-egl|-gbm|-gl|-gles|-dri-gallium)?|libdrm)$'); do apk info -qL $p; done | grep '\.so' | sed 's|^|/|' >/tmp/gl.list
for f in $(cat /tmp/gl.list); do /lib/ld-musl-x86_64.so.1 --list $f 2>/dev/null | awk '/=>/{print $3}'; done >>/tmp/gl.list
for f in $(sort -u /tmp/gl.list); do
	case $f in */ld-musl* | */libc.musl*) continue ;; */dri/*) d=$g/lib/dri ;; */gbm/*) d=$g/lib/gbm ;; *) d=$g/lib ;; esac
	cp -L $f $d/ && echo "$(basename $f) $(basename "$(readlink -f $f)")" >>$g/VERSIONS
done
echo "DRIVERS branch=$v kib=$(du -sk $g | cut -f1)"
# sandbox BUNDLE COMMAND...: the bundle as /usr, its own data kept in /tmp/home/NAME as Flatpak keeps ~/.var/app/ID
sandbox() {
	b=$1; shift
	n=$(basename $(dirname $b))-$(basename $b); newer=/tmp/newer/$n; home=/tmp/home/$(basename $b)
	mkdir -p $home
	if [ ! -d $newer ]; then
		mkdir -p $newer
		while read -r so real; do
			a=$(readlink $b/usr/lib/$so 2>/dev/null || readlink $b/lib/$so 2>/dev/null) || continue
			hv=${real#"$so".}; av=${a#"$so".}
			[ "$hv" != "$real" ] && [ "$(apk version -t "$hv" "$av")" = ">" ] && cp $g/lib/$so $newer/ && echo "NEWER bundle=$n $so $av to $hv"
		done <$g/VERSIONS
	fi
	set -- --ro-bind $newer /run/newer --ro-bind $b/usr /usr --tmpfs /lib \
		--ro-bind /lib/ld-musl-x86_64.so.1 /lib/ld-musl-x86_64.so.1 --symlink ld-musl-x86_64.so.1 /lib/libc.musl-x86_64.so.1 \
		--ro-bind $g /run/drivers --dev /dev --proc /proc --tmpfs /tmp --bind $home /home/app \
		--dir /run/user --bind $XDG_RUNTIME_DIR/wayland-1 /run/user/wayland-1 "$@"
	[ -d $b/lib ] && set -- --ro-bind $b/lib /run/app-lib "$@"
	[ -d $b/etc/fonts ] && set -- --ro-bind $b/etc/fonts /etc/fonts "$@"
	[ -d $b/var/cache/fontconfig ] && set -- --ro-bind $b/var/cache/fontconfig /var/cache/fontconfig "$@"
	bwrap --unshare-all --die-with-parent --clearenv --setenv PATH /usr/bin --setenv HOME /home/app \
		--setenv XDG_RUNTIME_DIR /run/user --setenv WAYLAND_DISPLAY wayland-1 --setenv WAYLAND_DEBUG 1 \
		--setenv LD_LIBRARY_PATH /run/newer:/run/app-lib:/usr/lib:/run/drivers/lib \
		--setenv LIBGL_DRIVERS_PATH /run/drivers/lib/dri --setenv GBM_BACKENDS_PATH /run/drivers/lib/gbm "$@"
}
for bdir in /out/bundles/*; do
	bv=$(basename $bdir)
	for app in "calc /usr/bin/gnome-calculator" "gpu /usr/bin/es2gears_wayland"; do
		set -- $app; b=$bdir/$1
		for run in 1 2 3; do
			first_frame "base=$v bundle=$bv app=$1 run=$run kib=$(cat $b/SIZE_KIB) gz_kib=$(($(cat $b/GZ_BYTES) / 1024))" $2 sandbox $b -- $2
		done
	done
	b=$bdir/cli; t0=$(ms)
	sum=$(echo '{"a":[1,2,3]}' | sandbox $b -- /usr/bin/jq -c '.a|add' 2>/dev/null)
	rg=$(sandbox $b -- /usr/bin/rg --version 2>/dev/null | head -1)
	ok=no; [ "$sum" = 6 ] && [ -n "$rg" ] && ok=yes
	echo "RESULT base=$v bundle=$bv app=cli run=1 kib=$(cat $b/SIZE_KIB) gz_kib=$(($(cat $b/GZ_BYTES) / 1024)) ran=$ok ms=$(($(ms) - t0))"
done
