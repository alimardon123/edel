#!/bin/sh
# Pulls CI's build container, $ALPINE (alpine:BRANCH@DIGEST, read from
# features/base.toml), and writes the reference that worked to
# $GITHUB_ENV for the steps after it. Docker Hub limits anonymous pulls
# per runner address and refused or timed out three runs in a row on
# 2026-10-09, so Google's copy of Docker Hub's official images is tried
# first; the digest pins the same image on both.
set -eu
for ref in "mirror.gcr.io/library/$ALPINE" "$ALPINE"; do
	for try in 1 2 3; do
		if docker pull -q "$ref"; then
			echo "ALPINE=$ref" >>"${GITHUB_ENV:-/dev/null}"
			echo "pull-alpine.sh: using $ref"
			exit 0
		fi
		sleep $((try * 10))
	done
done
echo "pull-alpine.sh: could not pull $ALPINE from mirror.gcr.io or Docker Hub, three tries each; re-run the job later" >&2
exit 1
