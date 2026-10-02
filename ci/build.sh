#!/bin/sh
# Builds the edel tool and every image. Runs inside the alpine:3.24
# container (privileged, because building a root filesystem mounts
# proc, sys and dev inside it):
#
#   docker run --rm --privileged -v "$PWD:/src" -w /src alpine:3.24 sh ci/build.sh
set -eu

# Inside the checkout, so CI's cache step can keep them between runs.
export CARGO_HOME=/src/.cargo-home CARGO_TARGET_DIR=/src/target

apk add --no-cache cargo dosfstools e2fsprogs grub grub-efi mtools openssh-keygen sfdisk tar

cargo build --release --locked
for def in images/*.toml; do
	./target/release/edel image build "$def" --out out
done
# Two throwaway signing keys, new on every run; the real keys arrive with
# the first preview (roadmap M3.4).
rm -rf out/keys
./target/release/edel release keygen out/keys ci-1
./target/release/edel release keygen out/keys ci-2

# The system test image (roadmap M2.2): the VM image with a seed system file
# in its slot that names the machine and adds user ci, who logs in with a
# fresh ssh key.
seed=out/system-test/seed
rm -rf "$seed"
mkdir -p "$seed/usr/share/edel"
ssh-keygen -q -t ed25519 -N '' -C ci@edel -f out/keys/ci-ssh
cat >"$seed/usr/share/edel/system.toml" <<EOF
format = 1

[network]
hostname = "ci-seeded"

[users.ci]
admin = true
ssh_keys = ["$(cat out/keys/ci-ssh.pub)"]
EOF
./target/release/edel image build images/vm.toml --files "$seed" --out out/system-test

# The A/B test image: the VM image plus the test steps that ci/ab-test.sh runs.
# health_timeout 30 keeps the hang cases to minutes (roadmap M1.5).
./target/release/edel image build images/vm.toml --files ci/ab-test/files \
	--health-timeout 30 --public-key out/keys/ci-1.pub --public-key out/keys/ci-2.pub \
	--out out/ab-test
# Its update: the same image with a tagged boot loader, so installing it
# must swap the loader once the new slot is confirmed (roadmap M1.8).
./target/release/edel image build images/vm.toml --files ci/ab-test/files \
	--health-timeout 30 --public-key out/keys/ci-1.pub --public-key out/keys/ci-2.pub \
	--loader-tag ci --out out/ab-test-update

# The update the A/B test downloads over HTTP (ci/ab-test.sh serves this
# directory): a signed release.toml, the shrunk and gzipped image, and
# bad.toml, the same manifest with one byte changed (roadmap M1.6, M1.7).
update=out/ab-test/update
rm -rf "$update"
mkdir -p "$update"
ln out/ab-test-update/edel-vm-x86_64.ext4.gz "$update/"
./target/release/edel release make --version 0.1.1 --channel ci "$update/edel-vm-x86_64.ext4.gz"
./target/release/edel release sign --key out/keys/ci-1.key "$update/release.toml"
sed 's/^channel = "ci"$/channel = "cj"/' "$update/release.toml" >"$update/bad.toml"
cp "$update/release.toml.sig" "$update/bad.toml.sig"

# The Flatpak spike image: the VM image plus dbus, flatpak and a test
# service that installs and runs a Flathub runtime (roadmap M1.9).
./target/release/edel image build ci/flatpak/vm.toml --out out/flatpak

# This container runs as root. Hand the finished images back to whoever owns
# the checkout, so the host can boot, read and delete them without root.
# The work directories stay root's: they hold the built root filesystems.
owner=$(stat -c %u:%g .)
for dir in out out/ab-test out/ab-test/update out/ab-test-update out/flatpak out/keys out/system-test; do
	chown "$owner" "$dir"
	find "$dir" -maxdepth 1 -type f -exec chown "$owner" {} \;
done
ls -ls out out/ab-test
