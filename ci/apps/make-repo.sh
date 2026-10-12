#!/bin/sh
# Makes a tiny Flatpak repository for the default apps' tests (roadmap
# M6.2), with no network: a runtime, org.edel.TestPlatform branch 1, that
# holds musl and the libraries edel-testclient needs, taken from this
# Alpine container, and two apps of branch stable, org.edel.TestA and
# org.edel.TestB, each edel-testclient showing a window, titled test-a in
# #3399cc and test-b in #33aa66, so a test can swap one default app for
# the other. ci/build.sh runs it inside the build container, after cargo
# built the test client, and builds the test images' default apps from
# it. Usage: sh ci/apps/make-repo.sh REPO
set -eu

repo=$1
work=out/apps-src
client=target/release/edel-testclient
[ -x "$client" ] || {
	echo "ci/apps/make-repo.sh: $client is missing; run cargo build --release --workspace first" >&2
	exit 1
}
rm -rf "$work" "$repo"
mkdir -p "$work/runtime/files" "$work/runtime/usr/lib"

# A runtime keeps its files in usr/ (an app keeps its own in files/, which
# flatpak build-export also wants to find, empty here, as build-init makes
# both), which
# the sandbox mounts at /usr, where Flatpak links /lib to /usr/lib, so
# musl's loader and the client's libraries go in usr/lib.
cp -L /lib/ld-musl-x86_64.so.1 "$work/runtime/usr/lib/"
ldd "$client" | awk '/=> \// { print $3 }' | while read -r lib; do
	cp -L "$lib" "$work/runtime/usr/lib/"
done
cat >"$work/runtime/metadata" <<'META'
[Runtime]
name=org.edel.TestPlatform
runtime=org.edel.TestPlatform/x86_64/1
sdk=org.edel.TestPlatform/x86_64/1
META
flatpak build-export --runtime --arch=x86_64 "$repo" "$work/runtime" 1

# app ID TITLE NAME COLOUR: exports one app of the test client.
app() {
	dir="$work/$1"
	mkdir -p "$dir/files/bin" "$dir/files/share/applications"
	cp "$client" "$dir/files/bin/"
	cat >"$dir/files/share/applications/$1.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=$3
Comment=A default app of the tests (M6.2)
Exec=edel-testclient --title $2 --colour $4 --size 300x200
DESKTOP
	cat >"$dir/metadata" <<META
[Application]
name=$1
runtime=org.edel.TestPlatform/x86_64/1
sdk=org.edel.TestPlatform/x86_64/1
META
	# build-finish writes the command and the Wayland socket into the
	# metadata and exports the desktop file with its Exec line rewritten
	# to flatpak run, so the launcher starts the sandboxed copy, never the
	# host's test client.
	flatpak build-finish --command=edel-testclient --socket=wayland "$dir"
	flatpak build-export --arch=x86_64 "$repo" "$dir" stable
}
app org.edel.TestA test-a 'Test A' 3399cc
app org.edel.TestB test-b 'Test B' 33aa66
echo "ci/apps/make-repo.sh: $repo holds org.edel.TestPlatform//1, org.edel.TestA//stable and org.edel.TestB//stable"
