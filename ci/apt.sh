#!/bin/sh
# Installs Ubuntu packages on a GitHub runner from the packages an earlier
# run of the same job kept with actions/cache in ~/.cache/edel-apt, so only
# what changed since is downloaded. Ubuntu's mirror for the runners
# sometimes gave 36 kB/s, and QEMU's 45 MB then took 20 minutes in one job
# of a run: 12 of 93 installs took over 5 minutes (October 2026).
# Usage: sh ci/apt.sh PACKAGE...
set -eu
dir=$HOME/.cache/edel-apt
sudo mkdir -p "$dir/partial"
sudo apt-get update
sudo apt-get install -y -o Dir::Cache::archives="$dir" "$@"
# actions/cache packs the folder as the runner's user.
sudo rm -rf "$dir/partial" "$dir/lock"
sudo chown -R "$(id -u):$(id -g)" "$dir"
