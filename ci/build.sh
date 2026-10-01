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
ls -l out
