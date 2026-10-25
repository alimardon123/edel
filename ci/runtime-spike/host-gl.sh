#!/bin/sh
# host-gl.sh BRANCH... (M10.10, ADR-003 section 5), in an alpine container standing for a newer machine:
# installs GNOME Calculator on each older runtime built without a graphics driver (runtime.sh nogl) and
# runs it with the machine's own mesa lent from /srv/gl, a library both carry taken from the newer copy:
# once on the runtime's own musl ("runtime musl") and once started by the machine's musl loader
# ("machine musl"). Prints RESULT lines, with the graphics library the app really loaded.
set -u
. /scripts/common.sh
v=$(cut -d. -f1,2 /etc/alpine-release)
apk add -q flatpak mesa-dri-gallium mesa-egl mesa-gbm mesa-gl mesa-gles >/dev/null
start_weston
export HOME=/tmp/home; mkdir -p $HOME
# the machine's graphics part, as run-bundles.sh lends it, and musl's loader beside it
g=/srv/gl; mkdir -p $g/lib/dri $g/lib/gbm
for p in $(apk info -q | grep -E '^(mesa(-egl|-gbm|-gl|-gles|-dri-gallium)?|libdrm)$'); do apk info -qL $p; done | grep '\.so' | sed 's|^|/|' >/tmp/gl.list
for f in $(cat /tmp/gl.list); do /lib/ld-musl-x86_64.so.1 --list $f 2>/dev/null | awk '/=>/{print $3}'; done >>/tmp/gl.list
for f in $(sort -u /tmp/gl.list); do
	case $f in */ld-musl* | */libc.musl*) continue ;; */dri/*) d=$g/lib/dri ;; */gbm/*) d=$g/lib/gbm ;; *) d=$g/lib ;; esac
	cp -L $f $d/ && echo "$(basename $f) $(basename "$(readlink -f $f)")" >>$g/VERSIONS
done
cp /lib/ld-musl-x86_64.so.1 $g/
for b in "$@"; do
	flatpak --user uninstall -y --noninteractive --all >/dev/null 2>&1
	flatpak --user remote-add --no-gpg-verify old-$b file:///out/repo-$b-nogl
	flatpak --user install -y --noninteractive old-$b org.gnome.Calculator >/dev/null 2>&1 || { echo "RESULT base=$v runtime=$b musl=runtime run=1 drew=no install=failed"; continue; }
	rt=$(flatpak --user info -l org.edel.Platform//$b)/files
	# a library the drivers and the runtime both carry: the newer copy wins
	n=$g/newer-$b; mkdir -p $n
	while read -r so real; do
		a=$(readlink $rt/lib/$so 2>/dev/null) || continue
		hv=${real#"$so".}; av=${a#"$so".}
		[ "$hv" != "$real" ] && [ "$(apk version -t "$hv" "$av")" = ">" ] && cp $g/lib/$so $n/ && echo "NEWER runtime=$b $so $av to $hv"
	done <$g/VERSIONS
	lend="--filesystem=$g:ro --env=LD_LIBRARY_PATH=$n:/app/lib:/usr/lib:$g/lib --env=LIBGL_DRIVERS_PATH=$g/lib/dri --env=GBM_BACKENDS_PATH=$g/lib/gbm --env=WAYLAND_DEBUG=1"
	for musl in runtime machine; do
		set -- flatpak run $lend org.gnome.Calculator
		[ $musl = machine ] && set -- flatpak run $lend --command=$g/ld-musl-x86_64.so.1 org.gnome.Calculator /app/bin/gnome-calculator
		for run in 1 2 3; do
			first_frame "base=$v runtime=$b musl=$musl run=$run" gnome-calculator "$@" &
			sleep 2; gl=; for q in $(pgrep -f gnome-calculator); do gl=$(grep -o '[^ ]*libgallium[^ ]*\|[^ ]*libEGL[^ ]*' /proc/$q/maps 2>/dev/null | sort -u | tr '\n' ',')$gl; done
			wait $!; echo "LOADED base=$v runtime=$b musl=$musl run=$run gl=${gl:-none}"
		done
	done
	grep -v '^\[' /tmp/wl.log | grep -i 'error\|relocat\|symbol' | head -3
done
