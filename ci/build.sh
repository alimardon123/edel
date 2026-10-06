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

apk add --no-cache cargo dosfstools e2fsprogs e2fsprogs-extra grub grub-efi mtools openssh-keygen pigz sfdisk tar \
	eudev-dev libinput-dev libseat-dev libxkbcommon-dev mesa-dev pkgconf

# The tool and our programs the features ship (edel-compositor, M4.2b);
# edel image build copies those from beside itself.
cargo build --release --locked --workspace
# Every image of this run carries the run's version (EDEL_VERSION, from
# ci.yml; 0.1 when built by hand) and installs from one package index in a
# shared apk cache (roadmap M3.1). Test images that no step downloads are
# built with --no-compress. build VERSION ARGS...
version=${EDEL_VERSION:-0.1}
rm -rf out/apk-cache
build() {
	build_version=$1
	shift
	./target/release/edel image build --version "$build_version" --channel "${EDEL_CHANNEL:-ci}" \
		--apk-cache out/apk-cache "$@"
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
	build "$version" ci/desktop/vm.toml --files out/desktop-seed --health-timeout 30 --public-key out/keys/ci-1.pub \
		--no-compress --out out/desktop-test
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
	hand_back out out/desktop-test out/keys
	exit 0
fi

for def in images/*.toml; do
	build "$version" "$def" --out out
done

# The system test image (roadmap M2.2, M2.3): the VM image with a seed
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
build "$version" images/vm.toml --files "$seed" --no-compress --out out/system-test

# The install test image (roadmap M2.4, M2.5): the VM image plus a test
# service that installs onto a blank second disk, and the A/B test steps,
# which run on the disk it installs (ci/ab-test.sh). health_timeout 30
# keeps the hang cases to minutes (roadmap M1.5). Its slots are 1024 MiB,
# smaller than an installed slot, as on the desktop stick, so the install
# grows slot A to 4096 MiB (M3.3b).
build "$version" images/vm.toml --files ci/ab-test/files --files ci/install-test/files --slot-mib 1024 \
	--health-timeout 30 --public-key out/keys/ci-1.pub --public-key out/keys/ci-2.pub \
	--no-compress --out out/install-test
# Its update: the same image with a tagged boot loader, so installing it
# must swap the loader once the new slot is confirmed (roadmap M1.8).
build "$version.1" images/vm.toml --files ci/ab-test/files \
	--health-timeout 30 --public-key out/keys/ci-1.pub --public-key out/keys/ci-2.pub \
	--loader-tag ci --no-compress --out out/ab-test-update

# The update the A/B test downloads over HTTP (ci/ab-test.sh serves this
# directory): a signed release.toml, the shrunk and gzipped image, and
# bad.toml, the same manifest with one byte changed (roadmap M1.6, M1.7).
update=out/ab-test/update
rm -rf "$update"
mkdir -p "$update"
ln out/ab-test-update/edel-vm-x86_64.ext4.gz "$update/"
./target/release/edel release make --version "$version.1" --channel ci "$update/edel-vm-x86_64.ext4.gz"
./target/release/edel release sign --key out/keys/ci-1.key "$update/release.toml"
sed 's/^channel = "ci"$/channel = "cj"/' "$update/release.toml" >"$update/bad.toml"
cp "$update/release.toml.sig" "$update/bad.toml.sig"

# The Flatpak spike image: the VM image plus dbus, flatpak and a test
# service that installs and runs a Flathub runtime (roadmap M1.9).
build "$version" ci/flatpak/vm.toml --no-compress --out out/flatpak

# The desktop test image here too, for desktop-test.sh rollback, which
# runs in the VM test lane: its own update image, signed, is the update
# that case installs and breaks (roadmap M4.8).
desktop_test_image
update=out/desktop-test/update
rm -rf "$update"
mkdir -p "$update"
ln out/desktop-test/edel-desktop-x86_64.ext4.gz "$update/"
./target/release/edel release make --version "$version.1" --channel ci "$update/edel-desktop-x86_64.ext4.gz"
./target/release/edel release sign --key out/keys/ci-1.key "$update/release.toml"

# The release (roadmap M3.4), made on every run so every PR proves it can be
# made: out/release/ holds what a GitHub release publishes (the update
# images and their release.toml, the disks, the container image and the
# package lists), and out/channel/release.toml names the same update images
# by their URL under the tag's release, for the stable channel. They are
# signed by ci/sign.sh in a container of its own, so the release key never
# reaches cargo, apk or this build.
tag=${EDEL_TAG:-v$version}
rm -rf out/release out/channel
mkdir -p out/release out/channel
for f in out/*.ext4.gz out/*.img.gz out/*.tar.gz out/*.packages; do
	ln "$f" out/release/
done
./target/release/edel release make --version "$version" --channel "${EDEL_CHANNEL:-ci}" out/release/*.ext4.gz
./target/release/edel release make --version "$version" --channel "${EDEL_CHANNEL:-ci}" \
	--base-url "https://github.com/alimardon123/edel/releases/download/$tag" --out out/channel out/release/*.ext4.gz

hand_back out out/ab-test out/ab-test/update out/ab-test-update out/channel out/desktop-test out/desktop-test/update \
	out/flatpak out/install-test out/keys out/release out/system-test
ls -ls out out/install-test
