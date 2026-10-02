#!/bin/sh
# Builds the edel tool and every image. Runs inside the alpine:3.24
# container (privileged, because building a root filesystem mounts
# proc, sys and dev inside it):
#
#   docker run --rm --privileged -v "$PWD:/src" -w /src alpine:3.24 sh ci/build.sh
set -eu

# Inside the checkout, so CI's cache step can keep them between runs.
export CARGO_HOME=/src/.cargo-home CARGO_TARGET_DIR=/src/target

apk add --no-cache cargo dosfstools e2fsprogs e2fsprogs-extra grub grub-efi mtools sfdisk tar

cargo build --release --locked
for def in images/*.toml; do
	./target/release/edel image build "$def" --out out
done
# Two throwaway signing keys, new on every run; the real keys arrive with
# the first preview (roadmap M3.4).
rm -rf out/keys
./target/release/edel release keygen out/keys ci-1
./target/release/edel release keygen out/keys ci-2

# The A/B test image: the VM image plus the test steps that ci/ab-test.sh runs.
# health_timeout 30 keeps the hang cases to minutes (roadmap M1.5).
./target/release/edel image build images/vm.toml --files ci/ab-test/files \
	--health-timeout 30 --public-key out/keys/ci-1.pub --public-key out/keys/ci-2.pub \
	--out out/ab-test

# The update the A/B test downloads over HTTP (ci/ab-test.sh serves this
# directory): a signed release.toml, the shrunk and gzipped image, and
# bad.toml, the same manifest with one byte changed (roadmap M1.6, M1.7).
update=out/ab-test/update
rm -rf "$update"
mkdir -p "$update"
ln out/ab-test/edel-vm-x86_64.ext4.gz "$update/"
./target/release/edel release make --version 0.1.1 --channel ci "$update/edel-vm-x86_64.ext4.gz"
./target/release/edel release sign --key out/keys/ci-1.key "$update/release.toml"
sed 's/^channel = "ci"$/channel = "cj"/' "$update/release.toml" >"$update/bad.toml"
cp "$update/release.toml.sig" "$update/bad.toml.sig"

# This container runs as root. Hand the finished images back to whoever owns
# the checkout, so the host can boot, read and delete them without root.
# The work directories stay root's: they hold the built root filesystems.
owner=$(stat -c %u:%g .)
for dir in out out/ab-test out/ab-test/update out/keys; do
	chown "$owner" "$dir"
	find "$dir" -maxdepth 1 -type f -exec chown "$owner" {} \;
done
ls -ls out out/ab-test
