#!/bin/sh
# CI's checks of the bills of materials (M8.12), in four modes:
#
#   validate    on the Ubuntu runner: every out/NAME.packages has its
#               out/NAME.cdx.json beside it, every CycloneDX document and
#               security status (out/NAME.vex.json) is valid against its
#               1.6 schema, and every licence expression names SPDX ids.
#               The validator package carries the schemas, so nothing is
#               downloaded.
#   advisories [DIR]  in the Alpine build container, as root: checks
#               every image's and every add-on's packages (M8.42), in out/
#               or in DIR (the security watch's, M8.43), against Alpine's
#               security database (secdb) for the branch in
#               features/base.toml. An image that lacks a fix Alpine
#               already ships fails with status 3, as a rebuild takes the
#               newest packages; a fix Alpine has not published yet is
#               reported, not failed; any other failure is status 1.
#   vex [DIR]   in the Alpine build container, as root: writes
#               DIR/NAME.vex.json (default out/), the security status
#               (CycloneDX VEX) of every image's and add-on's packages
#               against Alpine's secdb: each known CVE as resolved,
#               in_triage (a fix exists that the image lacks, or Alpine
#               has not published one) or not_affected (Alpine lists it
#               as never affecting its package). A missing fix is status
#               0 here; advisories fails it.
#   --self-test in the Alpine container: runs the same check on fixtures,
#               so the logic is tested where it runs.
#
# Stdout is Markdown for the run summary; errors go to stderr.
set -eu

# Pinned, so validation does not change with a newer validator's schemas.
validator='cyclonedx-python-lib[json-validation]==11.9.0'
tab=$(printf '\t')
# Where vex writes its files; empty for every other mode.
vex_dir=""

die() {
	echo "bom: FAIL: $*" >&2
	exit 1
}

need_jq() {
	command -v jq >/dev/null || apk add --quiet --update-cache jq
}

validate() {
	echo "### Bills of materials (M8.12)"
	echo
	for list in out/*.packages; do
		[ -f "$list" ] || die "no out/*.packages; run edel image build first"
		stem=${list%.packages}
		[ -f "$stem.cdx.json" ] || die "$list has no $stem.cdx.json beside it"
	done
	if [ ! -x out/bom-venv/bin/python ]; then
		python3 -m venv out/bom-venv >&2 || die "python3 could not make out/bom-venv"
		out/bom-venv/bin/pip install --quiet "$validator" >&2 || die "pip could not install $validator"
	fi
	set -- out/*.cdx.json
	for f in out/*.vex.json; do
		if [ -f "$f" ]; then set -- "$@" "$f"; fi
	done
	status=0
	out/bom-venv/bin/python - "$@" <<'PY' || status=$?
import json
import os
import re
import sys

from cyclonedx import spdx
from cyclonedx.schema import SchemaVersion
from cyclonedx.validation.json import JsonStrictValidator

OPERATORS = ("AND", "OR", "WITH")
v = JsonStrictValidator(SchemaVersion.V1_6)
bad = 0
packages = 0
as_spdx = 0
for path in sys.argv[1:]:
    base = os.path.basename(path)
    vex = base.endswith(".vex.json")
    name = base[: -len(".vex.json" if vex else ".cdx.json")]
    text = open(path, encoding="utf-8").read()
    err = v.validate_str(text)
    if err:
        # The first line says what is wrong; the schema dump after it is long.
        print("- %s: not valid: %s" % (name, str(err).strip().splitlines()[0]))
        bad += 1
        continue
    doc = json.loads(text)
    if vex:
        found = doc.get("vulnerabilities", [])
        resolved = [x for x in found if x.get("analysis", {}).get("state") == "resolved"]
        print("- %s: valid, %d vulnerabilities, %d resolved" % (name, len(found), len(resolved)))
        continue
    comps = doc.get("components", [])
    print("- %s: valid, %d components" % (name, len(comps)))
    for comp in comps:
        packages += 1
        pkg = comp.get("name", "?")
        good = False
        for lic in comp.get("licenses", []):
            if "expression" in lic:
                tokens = [t for t in re.split(r"[\s()]+", lic["expression"]) if t and t not in OPERATORS]
                wrong = [t for t in tokens if not (spdx.is_supported_id(t) or t.startswith("LicenseRef-"))]
                for t in wrong:
                    print("- %s: %s's licence expression names %s, not an SPDX id" % (name, pkg, t))
                    bad += 1
                if tokens and not wrong:
                    good = True
            elif "license" in lic and "id" in lic["license"]:
                lid = lic["license"]["id"]
                if spdx.is_supported_id(lid):
                    good = True
                else:
                    print("- %s: %s's licence names %s, not an SPDX id" % (name, pkg, lid))
                    bad += 1
        if good:
            as_spdx += 1
print("- Licences: %d of %d packages as SPDX ids or expressions, the rest as apk's words" % (as_spdx, packages))
sys.exit(1 if bad else 0)
PY
	[ "$status" -eq 0 ] || die "a bill of materials or security status file is not valid"
}

# Set by check: w is its work directory, gimg..gshipped the group it is
# folding, and the rest of its variables are its own.
flush_group() {
	[ -n "$gimg" ] || return 0
	gpkgs=$(sort -u "$w/g.pkgs" | awk 'NF { if (n++) printf ", "; printf "%s", $0 } END { print "" }') || die "could not list the packages"
	gcves=$(tr ',' '\n' < "$w/g.cves" | sort -u | awk 'NF { if (n++) printf ", "; printf "%s", $0 } END { print "" }') || die "could not list the advisories"
	if [ "$gshipped" = yes ]; then
		rowstatus="fix shipped, rebuild"
		echo "$gimg" >> "$w/shipped.txt"
	else
		rowstatus="waiting for Alpine's package"
	fi
	printf '| %s | %s | %s | %s | %s | %s | %s |\n' "$gimg" "$gorigin" "$gpkgs" "$gver" "$gfixed" "$gcves" "$rowstatus" >> "$w/table.txt"
}

# check SECDB_DIR INDEX FILE...
# INDEX is apk's package list as "name version" lines. Writes the Markdown
# section to stdout and returns 3 when a fix Alpine ships is missing from
# one of the FILEs. Its errors are not stopped by set -e here (a function
# run as `check ... || status=$?` ignores it), so every step that can fail
# says so itself. With vex_dir set, it also writes each image's security
# status there.
check() {
	secdb=$1
	index=$2
	shift 2
	count=$#
	gimg=""
	w=$(mktemp -d "$tmp/check.XXXXXX") || die "could not make a work directory"

	# Every fix of every origin in secdb, skipping "0" (never affected), and
	# the origins secdb lists as never affecting their package (key "0").
	: > "$w/fixes.tsv"
	: > "$w/never.tsv"
	for f in "$secdb"/*.json; do
		jq -r '(.packages // [])[].pkg | .name as $n | (.secfixes // {}) | to_entries[] | select(.key != "0") | [$n, .key, (.value | join(","))] | @tsv' "$f" >> "$w/fixes.tsv" || die "jq could not read $f"
		jq -r '(.packages // [])[].pkg | .name as $n | (.secfixes // {}) | to_entries[] | select(.key == "0") | [$n, (.value | join(","))] | @tsv' "$f" >> "$w/never.tsv" || die "jq could not read $f"
	done

	# Every binary package of every image, with its origin, and every
	# package's reference and package URL (purl) for the security status.
	: > "$w/components.tsv"
	: > "$w/refs.tsv"
	for f in "$@"; do
		img=$(basename "$f" .cdx.json)
		jq -r '.components[] | [.name, .version, (first((.properties // [])[] | select(.name == "apk:origin") | .value) // .name)] | @tsv' "$f" > "$w/one.tsv" || die "jq could not read the packages of $f"
		awk -v img="$img" 'BEGIN { FS = OFS = "\t" } { print img, $0 }' "$w/one.tsv" >> "$w/components.tsv" || die "could not read the packages of $f"
		jq -r '.components[] | [.name, .version, (."bom-ref" // .name), (.purl // "")] | @tsv' "$f" > "$w/one.tsv" || die "jq could not read the references of $f"
		awk -v img="$img" 'BEGIN { FS = OFS = "\t" } { print img, $0 }' "$w/one.tsv" >> "$w/refs.tsv" || die "could not read the references of $f"
	done

	# Candidates: IMAGE NAME VERSION ORIGIN FIXED CVES, for each package
	# whose origin has a fix. The one pass keeps the apk calls few.
	awk 'BEGIN { FS = OFS = "\t" }
		FILENAME == ARGV[1] { n[$1]++; fx[$1, n[$1]] = $2 "\t" $3; next }
		{ for (i = 1; i <= n[$4]; i++) print $1, $2, $3, $4, fx[$4, i] }' \
		"$w/fixes.tsv" "$w/components.tsv" > "$w/cand.tsv" || die "could not join the packages with the fixes"

	# The newest available version of each candidate name.
	cut -f2 "$w/cand.tsv" | sort -u > "$w/names.txt" || die "could not list the candidate names"
	awk 'FILENAME == ARGV[1] { want[$1] = 1; next } ($1 in want) { print $1, $2 }' "$w/names.txt" "$index" | sort > "$w/avail.txt" || die "could not read the package index"
	: > "$w/newest.txt"
	cur=""
	best=""
	while read -r name ver; do
		if [ "$name" != "$cur" ]; then
			if [ -n "$cur" ]; then echo "$cur $best" >> "$w/newest.txt"; fi
			cur=$name
			best=$ver
		else
			c=$(apk version -t "$ver" "$best") || die "apk could not compare $ver and $best"
			if [ "$c" = ">" ]; then best=$ver; fi
		fi
	done < "$w/avail.txt"
	if [ -n "$cur" ]; then echo "$cur $best" >> "$w/newest.txt"; fi

	# The (package, fix) pairs an image lacks. Shipped means the index
	# already has a version at or past the fix. Every candidate also gets
	# its security status line: resolved when the image has the fix.
	: > "$w/lack.tsv"
	: > "$w/vex.tsv"
	while IFS=$tab read -r img name ver origin fixed cves; do
		c=$(apk version -t "$ver" "$fixed") || die "apk could not compare $ver and $fixed"
		if [ "$c" = "<" ]; then
			avail=$(awk -v n="$name" '$1 == n { print $2; exit }' "$w/newest.txt")
			shipped=no
			if [ -n "$avail" ]; then
				c=$(apk version -t "$avail" "$fixed") || die "apk could not compare $avail and $fixed"
				if [ "$c" != "<" ]; then shipped=yes; fi
			fi
			printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$img" "$origin" "$name" "$ver" "$fixed" "$shipped" "$cves" >> "$w/lack.tsv"
			if [ "$shipped" = yes ]; then
				printf '%s\t%s\t%s\tin_triage\tupdate\tFixed in %s, which Alpine ships; this image has %s, and its next rebuild takes the fix.\n' "$img" "$name" "$cves" "$fixed" "$ver" >> "$w/vex.tsv"
			else
				printf '%s\t%s\t%s\tin_triage\t\tFixed in %s, which Alpine has not published yet; this image has %s.\n' "$img" "$name" "$cves" "$fixed" "$ver" >> "$w/vex.tsv"
			fi
		else
			printf '%s\t%s\t%s\tresolved\t\tFixed in %s; this image has %s.\n' "$img" "$name" "$cves" "$fixed" "$ver" >> "$w/vex.tsv"
		fi
	done < "$w/cand.tsv"

	# The packages secdb lists as never affected: not_affected, for each
	# image that has the origin.
	awk 'BEGIN { FS = OFS = "\t" }
		FILENAME == ARGV[1] { cv[$1] = cv[$1] (cv[$1] == "" ? "" : ",") $2; next }
		($4 in cv) { print $1, $2, cv[$4], "not_affected", "", "Alpine lists it as never affecting its package." }' \
		"$w/never.tsv" "$w/components.tsv" >> "$w/vex.tsv" || die "could not join the never-affected packages"

	# One row per image and source package.
	sort -t "$tab" -k1,1 -k2,2 "$w/lack.tsv" > "$w/lack.sorted" || die "could not sort the rows"
	: > "$w/table.txt"
	: > "$w/shipped.txt"
	gimg=""
	gorigin=""
	while IFS=$tab read -r img origin name ver fixed shipped cves; do
		if [ "$img" != "$gimg" ] || [ "$origin" != "$gorigin" ]; then
			flush_group
			gimg=$img
			gorigin=$origin
			gver=$ver
			gfixed=""
			gshipped=no
			: > "$w/g.pkgs"
			: > "$w/g.cves"
		fi
		echo "$name" >> "$w/g.pkgs"
		echo "$cves" >> "$w/g.cves"
		if [ -z "$gfixed" ]; then
			gfixed=$fixed
		else
			c=$(apk version -t "$fixed" "$gfixed") || die "apk could not compare $fixed and $gfixed"
			if [ "$c" = ">" ]; then gfixed=$fixed; fi
		fi
		if [ "$shipped" = yes ]; then gshipped=yes; fi
	done < "$w/lack.sorted"
	flush_group

	m=$(awk 'BEGIN { FS = "\t" } { k = split($NF, c, ","); for (i = 1; i <= k; i++) if (c[i] != "" && !(c[i] in s)) { s[c[i]] = 1; m++ } } END { print m + 0 }' "$w/lack.tsv") || die "could not count the advisories"

	echo "### Security fixes (M8.12)"
	echo
	if [ -s "$w/table.txt" ]; then
		echo "Checked $count images against Alpine's security database ($bom_branch): $m advisories open."
		echo
		echo "| Image | Source package | Packages | Has | Fixed in | Advisories | |"
		echo "|---|---|---|---|---|---|---|"
		cat "$w/table.txt"
	else
		echo "Checked $count images against Alpine's security database ($bom_branch): no advisory open."
	fi

	# The security status of each image (CycloneDX VEX), written when vex
	# asks for it. An image with no advisory gets empty lists.
	if [ -n "$vex_dir" ]; then
		url="https://secdb.alpinelinux.org/$bom_branch/"
		for f in "$@"; do
			img=$(basename "$f" .cdx.json)
			jq -n --arg img "$img" --arg url "$url" --rawfile rows "$w/vex.tsv" --rawfile refs "$w/refs.tsv" '
				def tsv: split("\n") | map(select(length > 0) | split("\t"));
				($refs | tsv | map(select(.[0] == $img)) | reverse
					| map({key: .[1], value: {version: .[2], ref: .[3], purl: (.[4] // "")}})
					| from_entries) as $refmap
				| ($rows | tsv | map(select(.[0] == $img))
					| map(. as $row | ($row[2] | split(",") | map(select(length > 0)))
						| map({id: ., name: $row[1], state: $row[3], response: ($row[4] // ""), detail: $row[5]}))
					| flatten
					| group_by([.id, .name]) | map(.[0]) | sort_by([.id, .name])) as $vulns
				| ($vulns | map(.name) | unique) as $names
				| {
					bomFormat: "CycloneDX",
					specVersion: "1.6",
					version: 1,
					metadata: {component: {type: "operating-system", "bom-ref": $img, name: $img}},
					components: [ $names[] | . as $n | $refmap[$n]
						| {type: "library", "bom-ref": .ref, name: $n, version: .version}
						+ (if .purl != "" then {purl: .purl} else {} end) ],
					vulnerabilities: [ $vulns[] | . as $v | {
						id: .id,
						source: {name: "Alpine secdb", url: $url},
						analysis: ({state: $v.state, detail: $v.detail}
							+ (if $v.response != "" then {response: [$v.response]} else {} end)),
						affects: [{ref: ($refmap[$v.name]).ref}]
					} ]
				}' > "$vex_dir/$img.vex.json" || die "could not write $vex_dir/$img.vex.json"
		done
	fi

	if [ -s "$w/shipped.txt" ]; then
		n=$(sort -u "$w/shipped.txt" | awk 'END { print NR }')
		# vex says nothing of it: advisories, the check that fails it, does.
		if [ -z "$vex_dir" ]; then
			echo "bom: FAIL: $n images lack a fix Alpine already ships; rebuild them (the build takes the newest packages)" >&2
		fi
		return 3
	fi
	return 0
}

# fetch_db: the branch, and the security database and the package index
# in a work directory of its own, for check. Sets bom_branch and tmp.
fetch_db() {
	bom_branch=$(sed -n 's/^branch = "\(.*\)"$/\1/p' features/base.toml)
	[ -n "$bom_branch" ] || die "features/base.toml names no branch"
	tmp=$(mktemp -d)
	trap 'rm -rf "$tmp"' EXIT
	mkdir "$tmp/secdb"
	for repo in main community; do
		url="https://secdb.alpinelinux.org/$bom_branch/$repo.json"
		wget -q -O "$tmp/secdb/$repo.json" "$url" ||
			die "could not fetch $url; secdb is Alpine's security database, needed to check the images"
	done
	apk update --quiet || die "apk update failed; the check needs Alpine's package index"
	need_jq
	apk search -v > "$tmp/search.txt" || die "apk search failed"
	sed 's/ - .*//; s/^\(.*\)-\([^-]*-r[0-9]*\)$/\1 \2/' "$tmp/search.txt" > "$tmp/available.txt" || die "could not read apk's package list"
}

advisories() {
	dir=${1:-out}
	fetch_db
	set -- "$dir"/*.cdx.json
	[ -f "$1" ] || die "no $dir/*.cdx.json; run edel image build first"
	status=0
	check "$tmp/secdb" "$tmp/available.txt" "$@" || status=$?
	exit "$status"
}

# vex [DIR]: writes DIR/NAME.vex.json for each image and add-on in DIR.
# A missing fix that Alpine ships is status 0 here (advisories fails it).
vex() {
	dir=${1:-out}
	fetch_db
	set -- "$dir"/*.cdx.json
	[ -f "$1" ] || die "no $dir/*.cdx.json; run edel image build first"
	images=$#
	vex_dir=$dir
	status=0
	check "$tmp/secdb" "$tmp/available.txt" "$@" > /dev/null || status=$?
	[ "$status" -eq 0 ] || [ "$status" -eq 3 ] || exit "$status"
	echo "bom: wrote the security status of $images images and add-ons in $dir (NAME.vex.json)" >&2
	exit 0
}

selftest_fail() {
	echo "bom.sh self-test: FAIL: $1" >&2
	exit 1
}

# want FILE TEXT WHAT: FILE holds TEXT, or the self-test fails saying WHAT.
want() {
	grep -F -e "$2" "$1" >/dev/null || selftest_fail "$3"
}

# want_json FILE PROGRAM WHAT: the jq test PROGRAM holds for FILE, or the
# self-test fails saying WHAT.
want_json() {
	jq -e "$2" "$1" >/dev/null || selftest_fail "$3"
}

# want_vex FILE TEST WHAT: a vulnerability of FILE passes the jq TEST, which
# may use $c, each component's bom-ref mapped to its name; WHAT says what
# fails otherwise.
want_vex() {
	jq -e '(.components | map({key: ."bom-ref", value: .name}) | from_entries) as $c | any(.vulnerabilities[]; '"$2"')' "$1" >/dev/null || selftest_fail "$3"
}

selftest() {
	need_jq
	tmp=$(mktemp -d)
	trap 'rm -rf "$tmp"' EXIT
	bom_branch=vTEST
	mkdir "$tmp/secdb" "$tmp/one" "$tmp/two"
	cat > "$tmp/secdb/main.json" <<'JSON'
{"packages": [
 {"pkg": {"name": "openssl", "secfixes": {"3.0.1-r0": ["CVE-2099-0001"], "3.0.2-r0": ["CVE-2099-0002"]}}},
 {"pkg": {"name": "zlib", "secfixes": {"1.3.1-r0": ["CVE-2099-0003"]}}},
 {"pkg": {"name": "busybox", "secfixes": {"0": ["CVE-2099-0004"]}}}
]}
JSON
	cat > "$tmp/one/edel-test-x86_64.cdx.json" <<'JSON'
{"bomFormat": "CycloneDX", "specVersion": "1.6", "version": 1, "components": [
 {"type": "library", "bom-ref": "a", "name": "libcrypto3", "version": "3.0.0-r0", "properties": [{"name": "apk:origin", "value": "openssl"}]},
 {"type": "library", "bom-ref": "b", "name": "zlib", "version": "1.3-r0", "properties": [{"name": "apk:origin", "value": "zlib"}]},
 {"type": "library", "bom-ref": "c", "name": "busybox", "version": "1.0-r0"}
]}
JSON
	sed 's/3\.0\.0-r0/3.0.2-r0/; s/"1\.3-r0"/"1.3.1-r0"/' "$tmp/one/edel-test-x86_64.cdx.json" > "$tmp/two/edel-test-x86_64.cdx.json"
	# An add-on's own packages (M8.42), IMAGE.ADDON.cdx.json, are checked
	# the same way.
	cat > "$tmp/one/edel-test-x86_64.desktop.cdx.json" <<'JSON'
{"bomFormat": "CycloneDX", "specVersion": "1.6", "version": 1, "components": [
 {"type": "library", "bom-ref": "d", "name": "libssl3", "version": "3.0.1-r0", "properties": [{"name": "apk:origin", "value": "openssl"}]}
]}
JSON
	printf 'libcrypto3 3.0.2-r0\nlibssl3 3.0.2-r0\nzlib 1.3-r0\nbusybox 1.0-r0\n' > "$tmp/available.txt"

	status=0
	check "$tmp/secdb" "$tmp/available.txt" "$tmp/one/edel-test-x86_64.cdx.json" "$tmp/one/edel-test-x86_64.desktop.cdx.json" > "$tmp/first.md" 2> "$tmp/first.err" || status=$?
	[ "$status" -eq 3 ] || selftest_fail "the first run wanted exit 3, got $status"
	want "$tmp/first.md" "| edel-test-x86_64 | openssl | libcrypto3 | 3.0.0-r0 | 3.0.2-r0 | CVE-2099-0001, CVE-2099-0002 | fix shipped, rebuild |" "openssl's fixes are not reported as shipped"
	want "$tmp/first.md" "| edel-test-x86_64.desktop | openssl | libssl3 | 3.0.1-r0 | 3.0.2-r0 | CVE-2099-0002 | fix shipped, rebuild |" "the add-on's missing openssl fix is not reported"
	want "$tmp/first.md" "| edel-test-x86_64 | zlib | zlib | 1.3-r0 | 1.3.1-r0 | CVE-2099-0003 | waiting for Alpine's package |" "zlib's unpublished fix is not reported as waiting"
	if grep -F busybox "$tmp/first.md" >/dev/null; then
		selftest_fail "busybox is reported, but its only fix is 0 (never affected)"
	fi

	# The same run again, writing each image's security status (M8.12).
	mkdir "$tmp/vex"
	vex_dir="$tmp/vex"
	status=0
	check "$tmp/secdb" "$tmp/available.txt" "$tmp/one/edel-test-x86_64.cdx.json" "$tmp/one/edel-test-x86_64.desktop.cdx.json" > /dev/null 2> "$tmp/vex.err" || status=$?
	vex_dir=""
	[ "$status" -eq 0 ] || [ "$status" -eq 3 ] || selftest_fail "the security status run wanted exit 0 or 3, got $status"
	vex1="$tmp/vex/edel-test-x86_64.vex.json"
	vexd="$tmp/vex/edel-test-x86_64.desktop.vex.json"
	want_json "$vex1" '.bomFormat == "CycloneDX" and .specVersion == "1.6"' "the security status is not CycloneDX 1.6"
	want_json "$vexd" '.bomFormat == "CycloneDX" and .specVersion == "1.6"' "the add-on's security status is not CycloneDX 1.6"
	want_vex "$vex1" '.id == "CVE-2099-0001" and $c[.affects[0].ref] == "libcrypto3" and .analysis.state == "in_triage"' "libcrypto3's CVE-2099-0001 is not in triage"
	want_vex "$vex1" '.id == "CVE-2099-0002" and $c[.affects[0].ref] == "libcrypto3" and .analysis.state == "in_triage"' "libcrypto3's CVE-2099-0002 is not in triage"
	want_vex "$vex1" '.id == "CVE-2099-0003" and $c[.affects[0].ref] == "zlib" and .analysis.state == "in_triage"' "zlib's CVE-2099-0003 is not in triage"
	want_vex "$vex1" '.id == "CVE-2099-0004" and $c[.affects[0].ref] == "busybox" and .analysis.state == "not_affected"' "busybox's CVE-2099-0004 is not not_affected"
	want_vex "$vex1" '.id == "CVE-2099-0001" and $c[.affects[0].ref] == "libcrypto3" and .analysis.response == ["update"]' "a shipped fix has no update response"
	want_vex "$vex1" '.id == "CVE-2099-0003" and $c[.affects[0].ref] == "zlib" and (.analysis | has("response") | not) and (.analysis.detail | contains("not published yet"))' "an unpublished fix is marked as shipped, or has no detail saying so"
	want_vex "$vexd" '.id == "CVE-2099-0001" and $c[.affects[0].ref] == "libssl3" and .analysis.state == "resolved"' "libssl3's CVE-2099-0001 is not resolved in the add-on"
	want_vex "$vexd" '.id == "CVE-2099-0002" and $c[.affects[0].ref] == "libssl3" and .analysis.state == "in_triage"' "libssl3's CVE-2099-0002 is not in triage in the add-on"

	status=0
	check "$tmp/secdb" "$tmp/available.txt" "$tmp/two/edel-test-x86_64.cdx.json" > "$tmp/second.md" || status=$?
	[ "$status" -eq 0 ] || selftest_fail "the second run wanted exit 0, got $status"
	want "$tmp/second.md" "no advisory open" "an image with every fix is reported as open"

	# Every fix is in the second image, so its CVEs are resolved (M8.12).
	mkdir "$tmp/vex2"
	vex_dir="$tmp/vex2"
	status=0
	check "$tmp/secdb" "$tmp/available.txt" "$tmp/two/edel-test-x86_64.cdx.json" > /dev/null 2> "$tmp/vex2.err" || status=$?
	vex_dir=""
	[ "$status" -eq 0 ] || selftest_fail "the second security status run wanted exit 0, got $status"
	want_vex "$tmp/vex2/edel-test-x86_64.vex.json" '.id == "CVE-2099-0001" and $c[.affects[0].ref] == "libcrypto3" and .analysis.state == "resolved"' "libcrypto3's CVE-2099-0001 is not resolved in the image with the fix"
	want_vex "$tmp/vex2/edel-test-x86_64.vex.json" '.id == "CVE-2099-0002" and $c[.affects[0].ref] == "libcrypto3" and .analysis.state == "resolved"' "libcrypto3's CVE-2099-0002 is not resolved in the image with the fix"

	echo "bom.sh self-test: ok" >&2
}

case "${1:-}" in
validate) validate ;;
advisories) advisories "${2:-out}" ;;
vex) vex "${2:-out}" ;;
--self-test) selftest ;;
*)
	echo "usage: sh ci/bom.sh validate | advisories [DIR] | vex [DIR] | --self-test" >&2
	exit 2
	;;
esac
