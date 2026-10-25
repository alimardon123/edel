#!/bin/sh
# runtime.sh [nogl] (M10.10), in an alpine container: a Flatpak runtime, org.edel.Platform//BRANCH, made
# from that branch's packages (musl, GTK 4, libadwaita, mesa and fonts, with their caches made ahead),
# and on it each app of $APPS (default: GNOME Calculator), made from the packages it adds on top, /usr
# becoming /app; all committed to /out/repo-BRANCH, a Flatpak repository. With nogl the runtime carries
# no graphics driver (mesa's files are taken out), for a machine to lend its own; the repository is then
# /out/repo-BRANCH-nogl.
set -eu
v=$(cut -d. -f1,2 /etc/alpine-release)
repo=/out/repo-$v; [ "${1:-}" = nogl ] && repo=$repo-nogl
apk add -q flatpak ostree glib gtk4.0 >/dev/null
RT="musl busybox gtk4.0 libadwaita mesa-dri-gallium mesa-egl mesa-gl mesa-gles mesa-gbm font-dejavu fontconfig shared-mime-info hicolor-icon-theme adwaita-icon-theme"
mk() { r=$1; shift; rm -rf $r; mkdir -p $r/etc/apk; cp -r /etc/apk/keys /etc/apk/repositories $r/etc/apk/; apk add -q --root $r --initdb "$@" >/dev/null; }
# commit DIR REF: DIR (metadata and files/) as REF, with the metadata Flatpak checks, byte for byte
commit() {
	md=$(cat $1/metadata; printf x); md=${md%x}
	ostree --repo=$repo commit --branch=$2 --tree=dir=$1 --owner-uid=0 --owner-gid=0 --add-metadata-string=xa.metadata="$md" >/dev/null
}
mk /tmp/rt $RT
apk --root /tmp/rt info -q | sort >/tmp/rt.pk
w=/tmp/w; rm -rf $w $repo; mkdir -p $w/rt/files
ostree --repo=$repo init --mode=archive-z2
# the runtime: Flatpak mounts files/ at /usr and links /lib to /usr/lib, so /lib's files move into usr/lib
cp -a /tmp/rt/usr/. $w/rt/files/; cp -a /tmp/rt/lib/. $w/rt/files/lib/; cp -a /tmp/rt/bin/. $w/rt/files/bin/ 2>/dev/null || true
if [ "${1:-}" = nogl ]; then
	for p in $(grep -E '^(mesa(-egl|-gl|-gles|-gbm|-dri-gallium)?|libdrm|llvm[0-9]*-libs)$' /tmp/rt.pk); do
		apk --root /tmp/rt info -qL $p | while read -r f; do case $f in usr/*) rm -f $w/rt/files/${f#usr/} ;; lib/*) rm -f $w/rt/files/lib/${f#lib/} ;; esac; done
	done
fi
mkdir -p $w/rt/files/etc $w/rt/files/cache; cp -a /tmp/rt/etc/fonts $w/rt/files/etc/
fc-cache -s -y $w/rt/files >/dev/null 2>&1 || true
cp -a /tmp/rt/var/cache/fontconfig $w/rt/files/cache/ 2>/dev/null || true
for t in $w/rt/files/share/icons/*/; do [ -f $t/index.theme ] && gtk4-update-icon-cache -q -f $t; done
glib-compile-schemas $w/rt/files/share/glib-2.0/schemas
printf '/app/lib\n/usr/lib\n' >$w/rt/files/etc/ld-musl-x86_64.path
printf '[Runtime]\nname=org.edel.Platform\nruntime=org.edel.Platform/x86_64/%s\nsdk=org.edel.Platform/x86_64/%s\n' $v $v >$w/rt/metadata
commit $w/rt runtime/org.edel.Platform/x86_64/$v
echo "RUNTIME branch=$v gl=${1:-yes} kib=$(du -sk $w/rt/files | cut -f1) packages=$(wc -l </tmp/rt.pk)"
for app in ${APPS:-gnome-calculator}; do
	case $app in gnome-calculator) id=org.gnome.Calculator ;; gnome-text-editor) id=org.gnome.TextEditor ;; gnome-clocks) id=org.gnome.clocks ;; *) id=org.edel.$app ;; esac
	mk /tmp/all $RT $app; apk --root /tmp/all info -q | sort >/tmp/all.pk
	a=$w/$id; mkdir -p $a/files
	# the app: the files of every package it adds on top, /usr becoming /app
	for p in $(comm -13 /tmp/rt.pk /tmp/all.pk); do apk --root /tmp/all info -qL $p; done | while read -r f; do
		case $f in usr/*) d=$a/files/${f#usr/} ;; lib/*) d=$a/files/lib/${f#lib/} ;; *) continue ;; esac
		mkdir -p $(dirname $d); cp -a /tmp/all/$f $d
	done
	[ -d $a/files/share/glib-2.0/schemas ] && glib-compile-schemas $a/files/share/glib-2.0/schemas
	printf '[Application]\nname=%s\nruntime=org.edel.Platform/x86_64/%s\nsdk=org.edel.Platform/x86_64/%s\ncommand=%s\n\n[Context]\nshared=ipc;\nsockets=wayland;\ndevices=dri;\n' $id $v $v $app >$a/metadata
	commit $a app/$id/x86_64/stable
	echo "APP branch=$v id=$id kib=$(du -sk $a/files | cut -f1) packages=$(comm -13 /tmp/rt.pk /tmp/all.pk | wc -l)"
done
flatpak build-update-repo $repo >/dev/null
echo "REPO branch=$v gl=${1:-yes} objects_kib=$(du -sk $repo/objects | cut -f1)"
