#!/bin/sh
# Makes what the dev box tests (M7.17) serve the VM from this host, in
# out/dev-box/: the default box image that features/containers' own
# distrobox.conf names, saved with docker so podman loads it in the VM with
# no network; a small .deb app and a small AppImage, each a script that says
# hello and a desktop file, which the box installs and distrobox-export puts
# in the launcher. Run on the runner, before desktop-test.sh starts the VM.
# Needs docker, dpkg-deb, mksquashfs (squashfs-tools) and the network for
# the image and AppImage's runtime.
set -eu
out=out/dev-box
image=$(sed -n 's/^container_image_default="\(.*\)"$/\1/p' features/containers/usr/share/distrobox/distrobox.conf)
[ -n "$image" ] || {
	echo "dev-box-files.sh: features/containers/usr/share/distrobox/distrobox.conf names no container_image_default"
	exit 1
}
rm -rf "$out"
mkdir -p "$out/deb/DEBIAN" "$out/deb/usr/bin" "$out/deb/usr/share/applications" "$out/app/usr/bin"

# The image, by the name distrobox.conf gives it, three tries as the
# registry is on the network.
tries=0
until docker pull -q "$image"; do
	tries=$((tries + 1))
	[ "$tries" -lt 3 ] || exit 1
	echo "dev-box-files.sh: pulling $image failed, try $tries of 3; trying again in 10 s"
	sleep 10
done
docker save -o "$out/dev-box.tar" "$image"
chmod 644 "$out/dev-box.tar"

# edel-hello-deb: a .deb as a vendor ships one.
cat >"$out/deb/DEBIAN/control" <<EOF
Package: edel-hello-deb
Version: 1.0
Architecture: all
Maintainer: Edel OS CI <ci@edel.invalid>
Description: Says hello from the dev box (M7.17's test app)
EOF
printf '#!/bin/sh\necho "hello from the dev box"\n' >"$out/deb/usr/bin/edel-hello-deb"
chmod 755 "$out/deb/usr/bin/edel-hello-deb"
printf '[Desktop Entry]\nType=Application\nName=Edel Hello Deb\nExec=edel-hello-deb\nTerminal=false\n' \
	>"$out/deb/usr/share/applications/edel-hello-deb.desktop"
dpkg-deb --root-owner-group --build "$out/deb" "$out/edel-hello.deb" >/dev/null

# edel-hello-appimage: an AppImage, its AppDir squashed after the type 2
# runtime, as appimagetool makes one.
printf '#!/bin/sh\necho "hello from an AppImage"\n' >"$out/app/AppRun"
chmod 755 "$out/app/AppRun"
printf '[Desktop Entry]\nType=Application\nName=Edel Hello AppImage\nExec=AppRun\nIcon=edel-hello-appimage\nCategories=Utility;\nTerminal=false\n' \
	>"$out/app/edel-hello-appimage.desktop"
# An AppImage carries an icon at its root; one transparent pixel will do.
printf '<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"/>\n' >"$out/app/edel-hello-appimage.svg"
curl -fsSL --retry 3 -o "$out/runtime" https://github.com/AppImage/type2-runtime/releases/download/continuous/runtime-x86_64
[ "$(head -c 4 "$out/runtime" | od -An -tx1 | tr -d ' ')" = 7f454c46 ] || {
	echo "dev-box-files.sh: the AppImage runtime downloaded is not an ELF file"
	exit 1
}
mksquashfs "$out/app" "$out/app.squashfs" -root-owned -noappend -quiet -no-progress
cat "$out/runtime" "$out/app.squashfs" >"$out/edel-hello.AppImage"
chmod 755 "$out/edel-hello.AppImage"
rm -rf "$out/deb" "$out/app" "$out/runtime" "$out/app.squashfs"
echo "dev-box-files.sh: $image, $(du -h "$out/dev-box.tar" | cut -f1); edel-hello.deb and edel-hello.AppImage"
