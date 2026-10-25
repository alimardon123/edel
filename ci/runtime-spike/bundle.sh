#!/bin/sh
# bundle.sh NAME PACKAGE... (M10.9), in an alpine container: one app as a folder that carries every
# library it needs, /out/bundles/BRANCH/NAME, from Alpine's packages, but the frozen part the machine
# gives: musl and the graphics drivers (mesa, libdrm and LLVM). Writes its size, the size to download
# (gzip -9) and its package lists beside it.
set -eu
name=$1; shift
v=$(cut -d. -f1,2 /etc/alpine-release)
r=/tmp/root-$name; rm -rf $r; mkdir -p $r/etc/apk
cp -r /etc/apk/keys /etc/apk/repositories $r/etc/apk/
apk add -q --root $r --initdb busybox "$@" >/dev/null
out=/out/bundles/$v/$name; rm -rf $out; mkdir -p $out/etc
frozen=$(apk --root $r info -q | grep -E '^(musl|mesa(-egl|-gbm|-gl|-gles|-glapi|-dri-gallium|-vulkan-.*|-va-gallium|-vdpau-gallium)?|libdrm|llvm[0-9]*-libs|busybox.*|alpine-baselayout.*|apk-tools)$' || true)
for p in $frozen; do apk --root $r info -qL $p | sed 's|^|/|'; done | while read -r f; do
	[ -n "${f#/}" ] && [ ! -d "$r$f" ] && rm -f "$r$f"; true
done
cp -a $r/usr $out/usr
[ -d $r/lib ] && cp -a $r/lib $out/lib
[ -d $r/etc/fonts ] && cp -a $r/etc/fonts $out/etc/
[ -d $r/var/cache/fontconfig ] && mkdir -p $out/var/cache && cp -a $r/var/cache/fontconfig $out/var/cache/
apk --root $r info -q | grep -vxF "$frozen" >$out/PACKAGES
echo "$frozen" >$out/FROZEN
du -sk $out | cut -f1 >$out/SIZE_KIB
tar -C $out -cf - . | gzip -9 | wc -c >$out/GZ_BYTES
echo "BUNDLE branch=$v app=$name kib=$(cat $out/SIZE_KIB) gz_kib=$(($(cat $out/GZ_BYTES) / 1024)) packages=$(wc -l <$out/PACKAGES)"
