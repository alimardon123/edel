#!/bin/sh
# summary.sh RESULTS (M10.9, M10.10): the spike's RESULT lines as a Markdown table, one row per app,
# way and base: the first run (nothing cached yet) and the best of the later runs.
set -eu
echo "| App | Way | Built from | Runs on | Download | On disk | Drew | First frame, first run | First frame, later (best) | Memory (PSS) |"
echo "|---|---|---|---|---|---|---|---|---|---|"
awk '
/^RESULT/ {
	delete f
	for (i = 2; i <= NF; i++) { split($i, kv, "="); f[kv[1]] = kv[2] }
	if (f["bundle"] != "") { way = "bundle"; built = f["bundle"]; dl = f["gz_kib"]; disk = f["kib"] }
	else { way = (f["from"] == "flathub") ? "Flathub" : "Alpine runtime"; built = (f["from"] == "flathub") ? "Flathub" : substr(f["from"], 8); dl = f["download_kib"]; disk = f["disk_kib"] }
	k = f["app"] (f["app"] == "" ? "calc" : "") SUBSEP way SUBSEP built SUBSEP f["base"]
	if (!(k in order)) { order[k] = ++n; keys[n] = k }
	D[k] = dl; S[k] = disk; ok = (f["drew"] != "") ? f["drew"] : f["ran"]
	if (OK[k] != "no") OK[k] = ok
	ms = (f["first_frame_ms"] != "") ? f["first_frame_ms"] : f["ms"]
	if (f["run"] == 1) { F[k] = ms; P1[k] = f["pss_kib"] } else { L[k] = L[k] " " ms; M[k] = M[k] " " f["pss_kib"] }
}
function mid(list,   a, c) { c = split(list, a, " "); if (c == 0) return ""; if (c == 1) return a[1]; return (a[1] < a[2]) ? a[1] : a[2] }
function mb(k) { return (k == "" || k == 0) ? "" : sprintf("%.0f MB", k / 1024) }
END {
	for (i = 1; i <= n; i++) {
		k = keys[i]; split(k, p, SUBSEP)
		later = mid(L[k]); mem = mid(M[k]); if (mem == "") mem = P1[k]
		printf "| %s | %s | %s | %s | %s | %s | %s | %s | %s | %s |\n", p[1], p[2], p[3], p[4], mb(D[k]), mb(S[k]), OK[k], (F[k] == "" ? "" : F[k] " ms"), (later == "" ? "" : later " ms"), mb(mem)
	}
}' "$1"
