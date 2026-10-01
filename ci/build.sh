#!/bin/sh
# Builds the edel tool and every image. Runs inside the alpine:3.24
# container (privileged, because building a root filesystem mounts
# proc, sys and dev inside it):
#
#   docker run --rm --privileged -v "$PWD:/src" -w /src alpine:3.24 sh ci/build.sh
set -eu

apk add --no-cache cargo e2fsprogs tar

cargo build --release --locked
for def in images/*.toml; do
	./target/release/edel image build "$def" --out out
done

# This container runs as root. Hand the finished images back to whoever owns
# the checkout, so the host can boot, read and delete them without root.
# out/work stays root's: it holds the built root filesystems.
owner=$(stat -c %u:%g .)
chown "$owner" out
find out -maxdepth 1 -type f -exec chown "$owner" {} \;
ls -l out
