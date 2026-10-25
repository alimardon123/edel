#!/bin/sh
# sh ci/runtime-spike/all.sh (M10.9, M10.10): the app runtime spikes, in Docker. Builds three apps as
# bundles that carry everything but musl and the graphics drivers (a GTK app, GNOME Calculator; a GPU
# app, es2gears; and two command-line tools, jq and ripgrep) from this year's Alpine branch (the one
# features/base.toml names) and from the branch a year older, and runs each on both, standing for an
# older and a newer Edel OS; then builds a Flatpak runtime from each branch's packages with the
# calculator on it, and runs both and Flathub's calculator on this year's. Writes every RESULT line to
# out/runtime-spike/results.txt and a table to out/runtime-spike/summary.md (and the run's summary in
# CI); fails when an app does not draw or run. EDEL_SPIKE_CA: a CA file the containers must trust, and
# then the proxy in $https_proxy is passed on (a cloud session's network); unset in CI.
set -eu
cd "$(dirname "$0")/../.."
new=$(sed -n 's/^branch = "v\(.*\)"/\1/p' features/base.toml)
old=${new%.*}.$((${new#*.} - 2))
out=$PWD/out/runtime-spike
rm -rf "$out"; mkdir -p "$out"
box() {
	img=alpine:$1; shift
	set -- -v "$out:/out" -v "$PWD/ci/runtime-spike:/scripts:ro" "$img" sh -c "apk update -q >/dev/null && $*"
	if [ -n "${EDEL_SPIKE_CA:-}" ]; then
		set -- --network host -e https_proxy="$https_proxy" -e http_proxy="$https_proxy" \
			-v "$EDEL_SPIKE_CA:/etc/ssl/certs/ca-certificates.crt:ro" "$@"
	fi
	docker run --rm --privileged "$@"
}
for b in "$old" "$new"; do
	box $b 'sh /scripts/bundle.sh calc gnome-calculator && sh /scripts/bundle.sh gpu mesa-demos && sh /scripts/bundle.sh cli jq ripgrep && sh /scripts/runtime.sh'
done | tee -a "$out/results.txt"
for b in "$old" "$new"; do box $b 'sh /scripts/run-bundles.sh'; done | tee -a "$out/results.txt"
for from in flathub "local $old" "local $new"; do box $new "sh /scripts/run-flatpak.sh $from"; done | tee -a "$out/results.txt"
sh ci/runtime-spike/summary.sh "$out/results.txt" >"$out/summary.md"
cat "$out/summary.md"
[ -n "${GITHUB_STEP_SUMMARY:-}" ] && cat "$out/summary.md" >>"$GITHUB_STEP_SUMMARY"
# 2 bases x 2 bundle branches x (3 calculator runs, 3 GPU runs, 1 command-line run), and 3 x 3 Flatpak runs
[ "$(grep -c '^RESULT' "$out/results.txt")" = 37 ] || { echo "FAIL: some run printed no RESULT line"; exit 1; }
! grep '^RESULT.* \(drew\|ran\)=no' "$out/results.txt"
