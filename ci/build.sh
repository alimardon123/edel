#!/bin/sh
# Builds the edel tool and every image. Runs inside the Alpine release
# the base feature names, in a container (privileged, because building a root filesystem mounts
# proc, sys and dev inside it):
#
#   docker run --rm --privileged -v "$PWD:/src" -w /src alpine:VERSION sh ci/build.sh
set -eu
what=${1:-all}
case "$what" in
all | desktop-test) ;;
*)
	echo "ci/build.sh: unknown argument $what; give none to build everything, or desktop-test" >&2
	exit 2
	;;
esac

# Inside the checkout, so CI's cache step can keep them between runs.
export CARGO_HOME=/src/.cargo-home CARGO_TARGET_DIR=/src/target

# Flatpak's triggers build the default apps' icon, desktop and MIME caches
# at install when these tools are there (M6.2a); pipewire-tools gives
# apps/make-repo.sh the pw-record it puts in the test runtime (M6.14b).
apk add --no-cache cargo dosfstools e2fsprogs e2fsprogs-extra grub grub-efi merge-usr mtools openssh-keygen pigz qemu-img sfdisk tar flatpak \
	gtk-update-icon-cache desktop-file-utils shared-mime-info pipewire-tools \
	eudev-dev libinput-dev libseat-dev libxkbcommon-dev mesa-dev pkgconf gtk4.0-dev libadwaita-dev

# The tool and our programs the features ship (edel-compositor, M4.2b);
# edel image build copies those from beside itself.
cargo build --release --locked --workspace
# Every image of this run carries the run's version (EDEL_VERSION, from
# ci.yml; 0.1 when built by hand) and installs from one package index in a
# shared apk cache (roadmap M3.1). Test images that no step downloads are
# built with --no-compress. build VERSION ARGS...
version=${EDEL_VERSION:-0.1}
rm -rf out/apk-cache out/apk-cache-tests
build() {
	build_version=$1
	shift
	./target/release/edel image build --version "$build_version" --channel "${EDEL_CHANNEL:-ci}" \
		--apk-cache "${apk_cache:-out/apk-cache}" "$@"
}
. ci/names.sh
# Two throwaway signing keys, new on every run; the real keys arrive with
# the first preview (roadmap M3.4).
rm -rf out/keys
./target/release/edel release keygen out/keys ci-1
./target/release/edel release keygen out/keys ci-2

# The desktop test image (roadmap M4.1): the desktop image plus the test
# feature in ci/desktop/features, which logs user ci in to the compositor
# at once and measures it (ci/desktop-test.sh). It takes CI's first key
# and a 30 s health timeout. Its seed settings file, ci/desktop/seed.toml,
# goes where a slot keeps its own, by the name ci/names.sh gives it.
desktop_test_image() {
	rm -rf out/desktop-seed
	mkdir -p "out/desktop-seed$share_dir"
	cp ci/desktop/seed.toml "out/desktop-seed$share_dir/$settings_name"
	# Its default apps (M6.2) are a test app from a repository made here,
	# with no network.
	sh ci/apps/make-repo.sh out/apps-repo
	build "$version" ci/desktop/vm.toml --files out/desktop-seed --health-timeout 30 --public-key out/keys/ci-1.pub \
		--app org.edel.TestA --apps-from "file://$PWD/out/apps-repo" --no-compress --out out/desktop-test
	# Our own container image (M7.6b), which the profile-developer case
	# imports as edel:ci and runs with podman, served from the runner.
	build "$version" images/container.toml --out out/desktop-container
}

# This container runs as root. Hand the finished images back to whoever
# owns the checkout, so the host can boot, read and delete them without
# root. The work directories stay root's: they hold the built root
# filesystems. hand_back DIR...
hand_back() {
	owner=$(stat -c %u:%g .)
	for dir in "$@"; do
		chown "$owner" "$dir"
		find "$dir" -maxdepth 1 -type f -exec chown "$owner" {} \;
	done
}

if [ "$what" = desktop-test ]; then
	desktop_test_image
	hand_back out out/desktop-test out/desktop-container out/keys
	exit 0
fi

# The test images build beside the one image, which spends most of its
# minutes compressing on every core while theirs mostly unpack packages on
# one, so the build ends minutes sooner (CI speed-ups, 2026-10-25). The
# container image goes first, alone: it fetches the run's package index,
# which the test images then take in a copy of the cache of their own, so
# two apk never write one cache. Their output is printed after the one
# image's, not between its lines.
build "$version" images/container.toml --out out
cp -a out/apk-cache out/apk-cache-tests
(
apk_cache=out/apk-cache-tests
# Built again from the same commit (M8.6): ci/repro-check.sh wants the
# container image byte for byte the same, and compares the CI VM image's
# slot, which is not required yet; the CI VM image is built twice here, as
# the released disk image, images/edel.toml, would fetch its apps twice.
rm -rf out/repro
build "$version" images/container.toml --out out/repro
build "$version" ci/vm/vm.toml --no-addons --no-compress --out out/repro/first
build "$version" ci/vm/vm.toml --no-addons --no-compress --out out/repro

# The system test image (roadmap M2.2, M2.3): the CI VM image (ci/vm/vm.toml) with a seed
# settings file in its slot that names the machine and adds user ci, who logs
# in with a fresh ssh key, as root does. The unknown key and the comment
# must survive `edel settings set` and `reset` byte for byte.
seed=out/system-test/seed
rm -rf "$seed"
mkdir -p "$seed$share_dir"
ssh-keygen -q -t ed25519 -N '' -C ci@edel -f out/keys/ci-ssh
cat >"$seed$share_dir/$settings_name" <<EOF
format = 1
future.key = 1

[network]
hostname = "ci-seeded"

# The person who runs CI
[users.ci]
admin = true
ssh_keys = ["$(cat out/keys/ci-ssh.pub)"]

[users.root]
ssh_keys = ["$(cat out/keys/ci-ssh.pub)"]
EOF
build "$version" ci/vm/vm.toml --files "$seed" --no-addons --no-compress --out out/system-test

# The developer mode test image (roadmap M7.1): the CI VM image (ci/vm/vm.toml) with the
# test's steps (ci/developer-test.sh); health_timeout 30 keeps the hang
# to a minute.
build "$version" ci/vm/vm.toml --files ci/developer-test/files --no-addons --health-timeout 30 \
	--no-compress --out out/developer-test

# The fleet (roadmap M7.3): two VM images, each ci/fleet/base.toml under one
# device file (`--settings` merges them, the later winning), and a FAT
# EDEL-SEED volume holding ci/fleet/erase.toml, which fleet-test.sh gives a
# fresh copy of device B's stick.
for device in a b; do
	build "$version" ci/vm/vm.toml --settings ci/fleet/base.toml --settings "ci/fleet/device-$device.toml" \
		--no-compress --out "out/fleet-$device"
done
rm -f out/fleet-seed.img
mkfs.vfat -C -n EDEL-SEED out/fleet-seed.img 4096
mcopy -i out/fleet-seed.img ci/fleet/erase.toml "::$settings_name"

# The server (roadmap M7.4): ci/server/vm.toml, which carries the containers
# add-on, with a FAT EDEL-SEED volume holding the server's settings (admin
# ci with the system test's ssh key, ssh on, the containers add-on) and the
# same settings named ci-cloud as the user data server-test.sh serves from
# a stand-in for a cloud's metadata address.
build "$version" ci/server/vm.toml --no-compress --health-timeout 30 --out out/server-test
server_settings() {
	cat <<EOF
format = 1

[network]
hostname = "$1"

[users.ci]
admin = true
ssh_keys = ["$(cat out/keys/ci-ssh.pub)"]

[services]
ssh = true

[addons]
installed = ["containers"]
EOF
}
server_settings ci-server >out/server-test/seed.toml
server_settings ci-cloud >out/server-test/user-data.toml
rm -f out/server-test/seed.img
mkfs.vfat -C -n EDEL-SEED out/server-test/seed.img 4096
mcopy -i out/server-test/seed.img out/server-test/seed.toml "::$settings_name"

# The install test image (roadmap M2.4, M2.5): the CI VM image (ci/vm/vm.toml) plus a test
# service that installs onto a blank second disk, and the A/B test steps,
# which run on the disk it installs (ci/ab-test.sh). health_timeout 30
# keeps the hang cases to minutes (roadmap M1.5). Its slots are 1024 MiB,
# smaller than an installed slot, as on the desktop stick, so the install
# copies slot A into a 4096 MiB slot, unchanged (M3.3b, M1.12).
build "$version" ci/vm/vm.toml --files ci/ab-test/files --files ci/install-test/files --slot-mib 1024 \
	--health-timeout 30 --public-key out/keys/ci-1.pub --public-key out/keys/ci-2.pub \
	--no-addons --no-compress --out out/install-test
# The updates test image (roadmap M7.5): ci/updates-test/vm.toml, under the
# CI VM image's name, so the A/B test's update below is its update too, with
# ci/updates-test/machine.toml (install and restart by itself) and a crontab
# that runs the hourly job every minute from that update on the host.
build "$version" ci/updates-test/vm.toml --files ci/updates-test/files --settings ci/updates-test/machine.toml \
	--health-timeout 30 --public-key out/keys/ci-1.pub --public-key out/keys/ci-2.pub \
	--no-addons --no-compress --out out/updates-test
# Its update: the same image with a tagged boot loader, so installing it
# must swap the loader once the new slot is confirmed (roadmap M1.8).
# With it, the test add-on ci-hello (M7.2a), built from its root, and
# for adding add-ons without an update (M7.2b) two real ones,
# virtualization (QEMU, whose user joins the image's kvm group) and
# printing (CUPS and SANE, whose scanimage the test runs, M7.6d), and
# ci-hang, whose service hangs the boot that tries it.
build "$version.1" ci/vm/vm.toml --files ci/ab-test/files \
	--health-timeout 30 --public-key out/keys/ci-1.pub --public-key out/keys/ci-2.pub \
	--loader-tag ci --no-addons --addon ci-hello --addon ci-hang --addon virtualization --addon printing \
	--features ci/features --no-compress --out out/ab-test-update

# The update the A/B test downloads over HTTP (ci/ab-test.sh serves this
# directory): a signed release.toml, the shrunk and gzipped image,
# bad.toml, the same manifest with one byte changed (roadmap M1.6, M1.7),
# and expired.toml, signed but past its date (M3.8). Lists are for the
# images' own channel and expire 60 days after signing, as released ones (the default row).
update=out/ab-test/update
rm -rf "$update"
mkdir -p "$update"
ln out/ab-test-update/edel-vm-x86_64.ext4.gz "$update/"
for addon in ci-hello ci-hang virtualization printing; do
	ln out/ab-test-update/edel-vm-x86_64.$addon.ext4.gz out/ab-test-update/edel-vm-x86_64.$addon.toml "$update/"
done
./target/release/edel release make --version "$version.1" --channel "${EDEL_CHANNEL:-ci}" \
	--addon "$update/edel-vm-x86_64.ci-hello.ext4.gz" --addon "$update/edel-vm-x86_64.ci-hang.ext4.gz" \
	--addon "$update/edel-vm-x86_64.virtualization.ext4.gz" --addon "$update/edel-vm-x86_64.printing.ext4.gz" \
	"$update/edel-vm-x86_64.ext4.gz"
./target/release/edel release sign --key out/keys/ci-1.key --expires-in 60 "$update/release.toml"
# badaddon.toml: signed, but its add-on's sha256 is not the file's, so the
# fetch fails after the slot is written and the slot stays off (M7.2a).
awk '/^\[\[addons\]\]/ { a = 1 } a && /^sha256 = / { $0 = "sha256 = \"" sprintf("%064d", 0) "\"" } { print }' \
	"$update/release.toml" >"$update/badaddon.toml"
./target/release/edel release sign --key out/keys/ci-1.key "$update/badaddon.toml"
sed 's/^version = "\(.*\)"$/version = "\1x"/' "$update/release.toml" >"$update/bad.toml"
cp "$update/release.toml.sig" "$update/bad.toml.sig"
sed 's/^expires = .*$/expires = "2020-01-01"/' "$update/release.toml" >"$update/expired.toml"
./target/release/edel release sign --key out/keys/ci-1.key "$update/expired.toml"

# The Flatpak spike image: the CI VM image (ci/vm/vm.toml) plus dbus, flatpak and a test
# service that installs and runs a Flathub runtime (roadmap M1.9).
build "$version" ci/flatpak/vm.toml --no-compress --out out/flatpak

# The desktop test image here too, for desktop-test.sh rollback, which
# runs in the VM test lane: its own update image and desktop add-on,
# signed, are the update that case installs and breaks (roadmap M4.8,
# M8.17c).
desktop_test_image
update=out/desktop-test/update
rm -rf "$update"
mkdir -p "$update"
ln out/desktop-test/edel-desktop-x86_64.ext4.gz "$update/"
ln out/desktop-test/edel-desktop-x86_64.desktop.ext4.gz out/desktop-test/edel-desktop-x86_64.desktop.toml "$update/"
./target/release/edel release make --version "$version.1" --channel "${EDEL_CHANNEL:-ci}" \
	--addon "$update/edel-desktop-x86_64.desktop.ext4.gz" "$update/edel-desktop-x86_64.ext4.gz"
./target/release/edel release sign --key out/keys/ci-1.key --expires-in 60 "$update/release.toml"

# The default apps' test (roadmap M6.2, ci/apps-test.sh): the CI VM image (ci/vm/vm.toml) with
# Flatpak, whose default app is TestA from the repository desktop_test_image
# made, and its update, whose default app is TestB and which drops TestA.
# Both are built from the local repository and point their machines at it
# over HTTP, where ci/apps-test.sh serves it.
apps_test() {
	apps_version=$1
	shift
	build "$apps_version" ci/apps-test/vm.toml --health-timeout 30 --public-key out/keys/ci-1.pub \
		--apps-from "file://$PWD/out/apps-repo" --apps-url http://10.0.2.2:8002/ --no-compress "$@"
}
apps_test "$version" --app org.edel.TestA --out out/apps-test
apps_test "$version.1" --app org.edel.TestB --dropped-app org.edel.TestA --out out/apps-test-update
update=out/apps-test/update
rm -rf "$update"
mkdir -p "$update"
ln out/apps-test-update/edel-apps-test-x86_64.ext4.gz "$update/"
./target/release/edel release make --version "$version.1" --channel "${EDEL_CHANNEL:-ci}" "$update/edel-apps-test-x86_64.ext4.gz"
./target/release/edel release sign --key out/keys/ci-1.key --expires-in 60 "$update/release.toml"
) >out/build-tests.log 2>&1 &
tests=$!
for def in images/*.toml; do
	[ "$def" = images/container.toml ] || build "$version" "$def" --out out
done
tests_ok=0
wait "$tests" || tests_ok=$?
cat out/build-tests.log
[ "$tests_ok" = 0 ] || {
	echo "ci/build.sh: building the test images failed (exit $tests_ok); out/build-tests.log above says where"
	exit 1
}

# The release (roadmap M3.4), made on every run so every PR proves it can be
# made: out/release/ holds what a GitHub release publishes (the update
# images and their release.toml, the disks, the container image, the
# package lists and the bills of materials, M8.12), and
# out/channel/release.toml names the same update images by their URL
# under the tag's release, for the stable channel. They are
# signed by ci/sign.sh in the workflow's sign job, on a runner of its own
# (M3.7), so the release key never reaches cargo, apk or this build.
tag=${EDEL_TAG:-v$version}
rm -rf out/release out/channel
mkdir -p out/release out/channel
for f in out/*.ext4.gz out/*.img.gz out/*.qcow2 out/*.tar.gz out/*.packages out/*.cdx.json out/*.*.toml; do
	ln "$f" out/release/
done
# The update images are IMAGE.ext4.gz; the add-ons IMAGE.NAME.ext4.gz, each
# with its feature file IMAGE.NAME.toml beside it (M7.2a). Every other file
# is a download, whose sum the release's own list carries (M8.10b).
set --
downloads=
for f in out/release/*; do
	case "${f##*/}" in
	*.*.ext4.gz) set -- "$@" --addon "$f" ;;
	*.ext4.gz) set -- "$@" "$f" ;;
	*) downloads="$downloads --download $f" ;;
	esac
done
# shellcheck disable=SC2086 # the file names hold no spaces
./target/release/edel release make --version "$version" --channel "${EDEL_CHANNEL:-ci}" $downloads "$@"
./target/release/edel release make --version "$version" --channel "${EDEL_CHANNEL:-ci}" \
	--base-url "https://github.com/alimardon123/edel/releases/download/$tag" --out out/channel "$@"
# The upgrade test's update (M3.5): the release's update images and add-ons
# beside one list for each channel an old release can be on, signed with
# the throwaway key ci/upgrade-test.sh makes the old release trust.
for channel in preview stable; do
	rm -rf "out/upgrade-test/$channel"
	mkdir -p "out/upgrade-test/$channel"
	ln out/release/*.ext4.gz out/release/*.*.toml "out/upgrade-test/$channel/"
	./target/release/edel release make --version "$version" --channel "$channel" --out "out/upgrade-test/$channel" "$@"
	./target/release/edel release sign --key out/keys/ci-1.key --expires-in 60 "out/upgrade-test/$channel/release.toml"
done

# A FAT stick holding a settings file (M6.6b), which desktop-test.sh live
# plugs in for Settings' installer page to find; FAT, as a stick written on
# any computer would be.
rm -f out/install-file.img
mkfs.vfat -C -n EDEL-FILE out/install-file.img 4096
mcopy -i out/install-file.img ci/desktop/install-file.toml "::$settings_name"

hand_back out out/ab-test out/ab-test/update out/ab-test-update out/apps-test out/apps-test/update out/apps-test-update out/channel out/desktop-test out/developer-test out/desktop-test/update \
	out/desktop-container out/flatpak out/fleet-a out/fleet-b out/server-test out/updates-test out/upgrade-test out/install-test out/keys out/release out/repro out/system-test
ls -ls out out/install-test
