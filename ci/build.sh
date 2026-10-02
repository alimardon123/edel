#!/bin/sh
# Builds the edel tool and every image. Runs inside the alpine:3.24
# container (privileged, because building a root filesystem mounts
# proc, sys and dev inside it):
#
#   docker run --rm --privileged -v "$PWD:/src" -w /src alpine:3.24 sh ci/build.sh
set -eu

# Inside the checkout, so CI's cache step can keep them between runs.
export CARGO_HOME=/src/.cargo-home CARGO_TARGET_DIR=/src/target

apk add --no-cache cargo dosfstools e2fsprogs grub grub-efi mtools sfdisk tar

cargo build --release --locked
for def in images/*.toml; do
	./target/release/edel image build "$def" --out out
done
# The A/B test image: the VM image plus the test steps that ci/ab-test.sh runs.
./target/release/edel image build images/vm.toml --files ci/ab-test/files --out out/ab-test

# This container runs as root. Hand the finished images back to whoever owns
# the checkout, so the host can boot, read and delete them without root.
# The work directories stay root's: they hold the built root filesystems.
owner=$(stat -c %u:%g .)
for dir in out out/ab-test; do
	chown "$owner" "$dir"
	find "$dir" -maxdepth 1 -type f -exec chown "$owner" {} \;
done
ls -ls out out/ab-test
