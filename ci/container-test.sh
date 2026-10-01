#!/bin/sh
# Imports the container image into Docker and checks that it is Edel OS
# and that apk works inside it.
set -eu

docker import out/edel-container-x86_64.tar.gz edel:ci
out=$(docker run --rm edel:ci sh -c '. /etc/os-release && echo "$PRETTY_NAME" && apk --version')
echo "$out"
echo "$out" | grep -q '^Edel OS ' || { echo "FAIL: not Edel OS"; exit 1; }
echo "$out" | grep -q '^apk-tools ' || { echo "FAIL: apk does not run"; exit 1; }
echo "PASS: container image"
