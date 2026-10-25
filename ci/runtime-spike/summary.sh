#!/bin/sh
# summary.sh RESULTS (M10.9, M10.10): the spike's RESULT lines as Markdown tables, one row per app,
# way and base: the first run (nothing cached yet) and the median of the later runs; then what three
# apps open at once take, and what each installed Flatpak item takes.
set -eu
echo "| App | Way | Built from | Runs on | Download | On disk | Drew | First frame, first run | First frame, later (median) | Memory (PSS) |"
echo "|---|---|---|---|---|---|---|---|---|---|"
awk '
/^RESULT/ {
	delete f
	for (i = 2; i <= NF; i++) { split($i, kv, "="); f[kv[1]] = kv[2] }
	app = (f["app"] == "") ? "calc" : f["app"]
	if (f["bundle"] != "") { way = "bundle"; built = f["bundle"]; dl = f["gz_kib"]; disk = f["kib"] }
	else if (f["runtime"] != "") { way = "old runtime, machine driver, " f["musl"] " musl"; built = f["runtime"]; dl = ""; disk = "" }
	else { way = (f["from"] == "flathub") ? "Flathub" : "Alpine runtime"; built = (f["from"] == "flathub") ? "Flathub" : substr(f["from"], 8); dl = f["download_kib"]; disk = f["disk_kib"] }
	k = app SUBSEP way SUBSEP built SUBSEP f["base"]
	if (!(k in seen)) { seen[k] = 1; keys[++n] = k }
	D[k] = dl; S[k] = disk; ok = (f["drew"] != "") ? f["drew"] : f["ran"]
	if (OK[k] != "no") OK[k] = ok
	ms = (f["first_frame_ms"] != "") ? f["first_frame_ms"] : f["ms"]
	if (f["run"] == 1) { F[k] = ms; P1[k] = f["pss_kib"] }
	else { L[k] = L[k] " " ms; if (f["pss_kib"] > 0) M[k] = M[k] " " f["pss_kib"] }
}
/^TOGETHER/ { T[++t] = $0 }
/^ITEM/ { I[++m] = $0 }
function median(list,   a, c, i, j, x) {
	c = split(list, a, " "); if (c == 0) return ""
	for (i = 2; i <= c; i++) { x = a[i] + 0; for (j = i - 1; j >= 1 && a[j] + 0 > x; j--) a[j + 1] = a[j]; a[j + 1] = x }
	return a[int((c + 1) / 2)]
}
function mb(k) { return (k == "" || k == 0) ? "" : sprintf("%.0f MB", k / 1024) }
END {
	for (i = 1; i <= n; i++) {
		k = keys[i]; split(k, p, SUBSEP)
		later = median(L[k]); mem = median(M[k]); if (mem == "") mem = P1[k]
		printf "| %s | %s | %s | %s | %s | %s | %s | %s | %s | %s |\n", p[1], p[2], p[3], p[4], mb(D[k]), mb(S[k]), OK[k], (F[k] == "" ? "" : F[k] " ms"), (later == "" ? "" : later " ms"), mb(mem)
	}
	print ""; print "| Three apps open at once | Memory (PSS, summed) |"; print "|---|---|"
	for (i = 1; i <= t; i++) { split(T[i], w, " "); delete f; for (j = 2; j <= length(w); j++) { split(w[j], kv, "="); f[kv[1]] = kv[2] }; printf "| %s | %s |\n", f["from"], mb(f["pss_kib"]) }
	print ""; print "| Installed item | Size |"; print "|---|---|"
	for (i = 1; i <= m; i++) { s = I[i]; sub(/^ITEM from=/, "", s); c = split(s, w, "\t"); printf "| %s %s | %s |\n", w[1], w[2], w[3] }
}' "$1"
