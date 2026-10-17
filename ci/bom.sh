#!/bin/sh
# CI's checks of the bills of materials (M8.12), in three modes:
#
#   validate    on the Ubuntu runner: every out/NAME.packages has its
#               out/NAME.cdx.json beside it, and every CycloneDX document
#               is valid against its 1.6 schema. The validator package
#               carries the schemas, so nothing is downloaded.
#   advisories  in the Alpine build container, as root: checks every
#               image's packages against Alpine's security database
#               (secdb) for the branch in features/base.toml. An image that
#               lacks a fix Alpine already ships fails, as a rebuild takes
#               the newest packages; a fix Alpine has not published yet is
#               reported, not failed.
#   --self-test in the Alpine container: runs the same check on fixtures,
#               so the logic is tested where it runs.
#
# Stdout is Markdown for the run summary; errors go to stderr.
set -eu

# Pinned, so validation does not change with a newer validator's schemas.
validator='cyclonedx-python-lib[json-validation]==11.9.0'
tab=$(printf '\t')

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
	status=0
	out/bom-venv/bin/python - out/*.cdx.json <<'PY' || status=$?
import json
import os
import sys

from cyclonedx.schema import SchemaVersion
from cyclonedx.validation.json import JsonStrictValidator

v = JsonStrictValidator(SchemaVersion.V1_6)
bad = 0
for path in sys.argv[1:]:
    name = os.path.basename(path)[: -len(".cdx.json")]
    text = open(path, encoding="utf-8").read()
    err = v.validate_str(text)
    if err:
        # The first line says what is wrong; the schema dump after it is long.
        print("- %s: not valid: %s" % (name, str(err).strip().splitlines()[0]))
        bad += 1
    else:
        count = len(json.loads(text).get("components", []))
        print("- %s: valid, %d components" % (name, count))
sys.exit(1 if bad else 0)
PY
	[ "$status" -eq 0 ] || die "a bill of materials is not valid CycloneDX 1.6"
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
# section to stdout and returns 1 when a fix Alpine ships is missing from
# one of the FILEs. Its errors are not stopped by set -e here (a function
# run as `check ... || status=$?` ignores it), so every step that can fail
# says so itself.
check() {
	secdb=$1
	index=$2
	shift 2
	count=$#
	gimg=""
	w=$(mktemp -d "$tmp/check.XXXXXX") || die "could not make a work directory"

	# Every fix of every origin in secdb, skipping "0" (never affected).
	: > "$w/fixes.tsv"
	for f in "$secdb"/*.json; do
		jq -r '(.packages // [])[].pkg | .name as $n | (.secfixes // {}) | to_entries[] | select(.key != "0") | [$n, .key, (.value | join(","))] | @tsv' "$f" >> "$w/fixes.tsv" || die "jq could not read $f"
	done

	# Every binary package of every image, with its origin.
	: > "$w/components.tsv"
	for f in "$@"; do
		img=$(basename "$f" .cdx.json)
		jq -r '.components[] | [.name, .version, (first((.properties // [])[] | select(.name == "apk:origin") | .value) // .name)] | @tsv' "$f" > "$w/one.tsv" || die "jq could not read the packages of $f"
		awk -v img="$img" 'BEGIN { FS = OFS = "\t" } { print img, $0 }' "$w/one.tsv" >> "$w/components.tsv" || die "could not read the packages of $f"
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
	# already has a version at or past the fix.
	: > "$w/lack.tsv"
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
		fi
	done < "$w/cand.tsv"

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
	if [ -s "$w/shipped.txt" ]; then
		n=$(sort -u "$w/shipped.txt" | awk 'END { print NR }')
		echo "bom: FAIL: $n images lack a fix Alpine already ships; rebuild them (the build takes the newest packages)" >&2
		return 1
	fi
	return 0
}

advisories() {
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
	set -- out/*.cdx.json
	[ -f "$1" ] || die "no out/*.cdx.json; run edel image build first"
	status=0
	check "$tmp/secdb" "$tmp/available.txt" "$@" || status=$?
	exit "$status"
}

selftest_fail() {
	echo "bom.sh self-test: FAIL: $1" >&2
	exit 1
}

# want FILE TEXT WHAT: FILE holds TEXT, or the self-test fails saying WHAT.
want() {
	grep -F -e "$2" "$1" >/dev/null || selftest_fail "$3"
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
	printf 'libcrypto3 3.0.2-r0\nzlib 1.3-r0\nbusybox 1.0-r0\n' > "$tmp/available.txt"

	status=0
	check "$tmp/secdb" "$tmp/available.txt" "$tmp/one/edel-test-x86_64.cdx.json" > "$tmp/first.md" 2> "$tmp/first.err" || status=$?
	[ "$status" -eq 1 ] || selftest_fail "the first run wanted exit 1, got $status"
	want "$tmp/first.md" "| edel-test-x86_64 | openssl | libcrypto3 | 3.0.0-r0 | 3.0.2-r0 | CVE-2099-0001, CVE-2099-0002 | fix shipped, rebuild |" "openssl's fixes are not reported as shipped"
	want "$tmp/first.md" "| edel-test-x86_64 | zlib | zlib | 1.3-r0 | 1.3.1-r0 | CVE-2099-0003 | waiting for Alpine's package |" "zlib's unpublished fix is not reported as waiting"
	if grep -F busybox "$tmp/first.md" >/dev/null; then
		selftest_fail "busybox is reported, but its only fix is 0 (never affected)"
	fi

	status=0
	check "$tmp/secdb" "$tmp/available.txt" "$tmp/two/edel-test-x86_64.cdx.json" > "$tmp/second.md" || status=$?
	[ "$status" -eq 0 ] || selftest_fail "the second run wanted exit 0, got $status"
	want "$tmp/second.md" "no advisory open" "an image with every fix is reported as open"

	echo "bom.sh self-test: ok" >&2
}

case "${1:-}" in
validate) validate ;;
advisories) advisories ;;
--self-test) selftest ;;
*)
	echo "usage: sh ci/bom.sh validate | advisories | --self-test" >&2
	exit 2
	;;
esac
