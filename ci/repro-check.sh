#!/bin/sh
# Checks that the images build the same twice from one commit (roadmap M8.6).
# ci/build.sh builds the container image once more into out/repro/, and the
# CI VM image twice, into out/repro/first/ and out/repro/,
# with SOURCE_DATE_EPOCH from the commit, and this compares the two by sha256.
# The container image must match byte for byte, or its bytes depend on the
# clock or on the order the file system lists files in. The CI VM image's slot is
# compared and reported, but not required yet (its ext4 still needs a fixed
# hash seed and UUID). Writes a table to the run summary when GitHub sets one.
#
#   sh ci/repro-check.sh     after ci/build.sh
set -eu

first=out
second=out/repro
container=edel-container-x86_64.tar.gz
slot=edel-vm-x86_64.ext4

missing=
for file in "$first/$container" "$second/$container" "$second/first/$slot" "$second/$slot"; do
	if [ ! -f "$file" ]; then
		echo "FAIL: $file is missing; run ci/build.sh first" >&2
		missing=yes
	fi
done
if [ -n "$missing" ]; then
	exit 1
fi

summary=${GITHUB_STEP_SUMMARY:-}
if [ -n "$summary" ]; then
	printf '%s\n\n| Image | First build | Second build | Result |\n| --- | --- | --- | --- |\n' \
		'### Reproducible images (M8.6)' >>"$summary"
fi

# pair NAME FIRST SECOND DIFFERS: prints one line for the pair, adds its row
# to the summary and sets same to yes or no. DIFFERS is the Result text
# when the two differ.
pair() {
	name=$1
	a=$(sha256sum "$2" | cut -d ' ' -f 1)
	b=$(sha256sum "$3" | cut -d ' ' -f 1)
	if [ "$a" = "$b" ]; then
		same=yes
		result=same
		echo "$name: same, $a"
	else
		same=no
		result=$4
		echo "$name: differs, $a and $b"
	fi
	if [ -n "$summary" ]; then
		printf '| %s | %s... | %s... | %s |\n' "$name" "$(printf '%.16s' "$a")" \
			"$(printf '%.16s' "$b")" "$result" >>"$summary"
	fi
}

pair container "$first/$container" "$second/$container" differs
container_same=$same
pair vm "$second/first/$slot" "$second/$slot" "differs (not required yet)"

if [ "$container_same" != yes ]; then
	echo "FAIL: the container image differs between two builds of one commit" >&2
	exit 1
fi
echo "PASS: the container image builds the same twice"
