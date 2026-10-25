#!/bin/sh
# runtime.sh (M10.10), in an alpine container: a Flatpak runtime, org.edel.Platform//BRANCH, made from
# that branch's packages (musl, GTK 4, libadwaita, mesa and fonts), and GNOME Calculator on it, made from
# the packages it adds on top, /usr becoming /app; both committed to /out/repo-BRANCH, a Flatpak repository.
set -eu
v=$(cut -d. -f1,2 /etc/alpine-release)
apk add -q flatpak ostree glib >/dev/null
RT="musl busybox gtk4.0 libadwaita mesa-dri-gallium mesa-egl mesa-gl mesa-gles mesa-gbm font-dejavu fontconfig shared-mime-info hicolor-icon-theme adwaita-icon-theme"
mk() { r=$1; shift; rm -rf $r; mkdir -p $r/etc/apk; cp -r /etc/apk/keys /etc/apk/repositories $r/etc/apk/; apk add -q --root $r --initdb "$@" >/dev/null; }
mk /tmp/rt $RT
mk /tmp/all $RT gnome-calculator
apk --root /tmp/rt info -q | sort > /tmp/rt.pk; apk --root /tmp/all info -q | sort > /tmp/all.pk
# the runtime: Flatpak mounts files/ at /usr and links /lib to /usr/lib, so /lib's files move into usr/lib
w=/tmp/w; rm -rf $w; mkdir -p $w/rt/files $w/app/files
cp -a /tmp/rt/usr/. $w/rt/files/; cp -a /tmp/rt/lib/. $w/rt/files/lib/; cp -a /tmp/rt/bin/. $w/rt/files/bin/ 2>/dev/null || true
mkdir -p $w/rt/files/etc; cp -a /tmp/rt/etc/fonts $w/rt/files/etc/
mkdir -p $w/rt/files/cache; cp -a /tmp/rt/var/cache/fontconfig $w/rt/files/cache/ 2>/dev/null || true
printf '/app/lib\n/usr/lib\n' > $w/rt/files/etc/ld-musl-x86_64.path
cat > $w/rt/metadata <<M
[Runtime]
name=org.edel.Platform
runtime=org.edel.Platform/x86_64/$v
sdk=org.edel.Platform/x86_64/$v
M
# the app: the files of every package the calculator adds on top, /usr becoming /app
for p in $(comm -13 /tmp/rt.pk /tmp/all.pk); do apk --root /tmp/all info -qL $p; done | grep -v '^$' | while read f; do
  case $f in usr/*) d=$w/app/files/${f#usr/};; lib/*) d=$w/app/files/lib/${f#lib/};; *) continue;; esac
  mkdir -p $(dirname $d); cp -a /tmp/all/$f $d; done
[ -d $w/app/files/share/glib-2.0/schemas ] && glib-compile-schemas $w/app/files/share/glib-2.0/schemas
comm -13 /tmp/rt.pk /tmp/all.pk > /out/app-packages-$v.txt
rm -rf /out/repo-$v
ostree --repo=/out/repo-$v init --mode=archive-z2
md=$(cat $w/rt/metadata; printf x); md=${md%x}
ostree --repo=/out/repo-$v commit --branch=runtime/org.edel.Platform/x86_64/$v --tree=dir=$w/rt --owner-uid=0 --owner-gid=0 \
  --add-metadata-string=xa.metadata="$md" >/dev/null
cat > $w/app/metadata <<M
[Application]
name=org.gnome.Calculator
runtime=org.edel.Platform/x86_64/$v
sdk=org.edel.Platform/x86_64/$v
command=gnome-calculator

[Context]
shared=ipc;
sockets=wayland;
devices=dri;
M
md=$(cat $w/app/metadata; printf x); md=${md%x}
ostree --repo=/out/repo-$v commit --branch=app/org.gnome.Calculator/x86_64/stable --tree=dir=$w/app --owner-uid=0 --owner-gid=0 \
  --add-metadata-string=xa.metadata="$md" >/dev/null
flatpak build-update-repo --generate-static-deltas /out/repo-$v >/dev/null 2>&1 || true
echo "RUNTIME branch=$v runtime_kib=$(du -sk $w/rt/files|cut -f1) app_kib=$(du -sk $w/app/files|cut -f1) repo_kib=$(du -sk /out/repo-$v/objects|cut -f1) app_packages=$(wc -l < /out/app-packages-$v.txt) runtime_packages=$(wc -l < /tmp/rt.pk)"
