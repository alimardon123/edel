#!/bin/sh
# Makes a tiny Flatpak repository for the default apps' test (roadmap
# M6.2), with no network: a runtime, org.edel.TestPlatform branch 1, that
# holds musl and the libraries edel-testclient needs, taken from this
# Alpine container, and an app, org.edel.TestA branch stable, which is
# edel-testclient showing a window titled test-a in #3399cc. ci/build.sh
# runs it inside the build container, after cargo built the test client,
# and builds the desktop test image's default apps from it. Usage:
# sh ci/apps/make-repo.sh REPO
set -eu

repo=$1
work=out/apps-src
client=target/release/edel-testclient
[ -x "$client" ] || {
	echo "ci/apps/make-repo.sh: $client is missing; run cargo build --release --workspace first" >&2
	exit 1
}
rm -rf "$work" "$repo"
mkdir -p "$work/runtime/files/lib" "$work/app/files/bin" "$work/app/files/share/applications"

# The runtime is mounted at /usr in the sandbox, where Flatpak links /lib
# to /usr/lib, so musl's loader and the client's libraries go in lib.
cp -L /lib/ld-musl-x86_64.so.1 "$work/runtime/files/lib/"
ldd "$client" | awk '/=> \// { print $3 }' | while read -r lib; do
	cp -L "$lib" "$work/runtime/files/lib/"
done
cat >"$work/runtime/metadata" <<'META'
[Runtime]
name=org.edel.TestPlatform
runtime=org.edel.TestPlatform/x86_64/1
sdk=org.edel.TestPlatform/x86_64/1
META
flatpak build-export --runtime --arch=x86_64 "$repo" "$work/runtime" 1

cp "$client" "$work/app/files/bin/"
cat >"$work/app/files/share/applications/org.edel.TestA.desktop" <<'DESKTOP'
[Desktop Entry]
Type=Application
Name=Test A
Comment=The default apps' test app (M6.2)
Exec=edel-testclient --title test-a --colour 3399cc --size 300x200
DESKTOP
cat >"$work/app/metadata" <<'META'
[Application]
name=org.edel.TestA
runtime=org.edel.TestPlatform/x86_64/1
sdk=org.edel.TestPlatform/x86_64/1
META
# build-finish writes the command and the Wayland socket into the
# metadata and exports the desktop file with its Exec line rewritten to
# flatpak run, so the launcher starts the sandboxed copy, never the
# host's test client.
flatpak build-finish --command=edel-testclient --socket=wayland "$work/app"
flatpak build-export --arch=x86_64 "$repo" "$work/app" stable
echo "ci/apps/make-repo.sh: $repo holds org.edel.TestPlatform//1 and org.edel.TestA//stable"
