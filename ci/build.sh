#!/bin/sh
# Builds the edel tool and every image. Runs inside the alpine:3.24
# container (privileged, because building a root filesystem mounts
# proc, sys and dev inside it):
#
#   docker run --rm --privileged -v "$PWD:/src" -w /src alpine:3.24 sh ci/build.sh
set -eu

# Inside the checkout, so CI's cache step can keep them between runs.
export CARGO_HOME=/src/.cargo-home CARGO_TARGET_DIR=/src/target

apk add --no-cache cargo dosfstools e2fsprogs e2fsprogs-extra grub grub-efi mtools openssh-keygen sfdisk tar

cargo build --release --locked
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
for def in images/*.toml; do
	build "$version" "$def" --out out
done
# Two throwaway signing keys, new on every run; the real keys arrive with
# the first preview (roadmap M3.4).
rm -rf out/keys
./target/release/edel release keygen out/keys ci-1
./target/release/edel release keygen out/keys ci-2

# The system test image (roadmap M2.2, M2.3): the VM image with a seed
# system file in its slot that names the machine and adds user ci, who logs
# in with a fresh ssh key, as root does. The unknown key and the comment
# must survive `edel system set` and `unset` byte for byte.
seed=out/system-test/seed
rm -rf "$seed"
mkdir -p "$seed/usr/share/edel"
ssh-keygen -q -t ed25519 -N '' -C ci@edel -f out/keys/ci-ssh
cat >"$seed/usr/share/edel/system.toml" <<EOF
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
# keeps the hang cases to minutes (roadmap M1.5).
build "$version" images/vm.toml --files ci/ab-test/files --files ci/install-test/files \
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

# The release (roadmap M3.4), made on every run so every PR proves it can be
# made: out/release/ holds what a GitHub release publishes (the update
# images and their release.toml, the disks, the container image and the
# package lists), and out/channel/release.toml names the same update images
# by their URL under the tag's release, for the stable channel. Both are
# signed with the release key when CI has it (EDEL_RELEASE_KEY, the secret
# half in hex) and checked against the committed public half in
# images/keys/; otherwise with a throwaway key, and out/channel/signer says
# which, so ci/release.sh never publishes a throwaway signature.
tag=${EDEL_TAG:-v$version}
rm -rf out/release out/channel
mkdir -p out/release out/channel
for f in out/*.ext4.gz out/*.img.gz out/*.tar.gz out/*.packages; do
	ln "$f" out/release/
done
if [ -n "${EDEL_RELEASE_KEY:-}" ]; then
	ls images/keys/*.pub >/dev/null 2>&1 || {
		echo "FAIL: EDEL_RELEASE_KEY is set but images/keys/ has no .pub to check it with (docs/RELEASE.md, step 1)"
		exit 1
	}
	# The secret never outlives this script, even when signing fails.
	trap 'rm -f out/keys/release.key' EXIT
	(umask 077 && printf '%s\n' "$EDEL_RELEASE_KEY" >out/keys/release.key)
	key=out/keys/release.key keys=images/keys signer=release
else
	# Its own directory: verify reads every *.pub in it, and out/keys also
	# holds the test keys and an ssh key.
	./target/release/edel release keygen out/keys/throwaway ci-release
	key=out/keys/throwaway/ci-release.key keys=out/keys/throwaway signer=throwaway
fi
./target/release/edel release make --version "$version" --channel "${EDEL_CHANNEL:-ci}" out/release/*.ext4.gz
./target/release/edel release make --version "$version" --channel "${EDEL_CHANNEL:-ci}" \
	--base-url "https://github.com/alimardon123/edel/releases/download/$tag" --out out/channel out/release/*.ext4.gz
for manifest in out/release/release.toml out/channel/release.toml; do
	./target/release/edel release sign --key "$key" "$manifest"
	./target/release/edel release verify --keys "$keys" "$manifest"
done
rm -f out/keys/release.key
echo "$signer" >out/channel/signer

# This container runs as root. Hand the finished images back to whoever owns
# the checkout, so the host can boot, read and delete them without root.
# The work directories stay root's: they hold the built root filesystems.
owner=$(stat -c %u:%g .)
for dir in out out/ab-test out/ab-test/update out/ab-test-update out/channel out/flatpak out/install-test out/keys out/release out/system-test; do
	chown "$owner" "$dir"
	find "$dir" -maxdepth 1 -type f -exec chown "$owner" {} \;
done
ls -ls out out/install-test
