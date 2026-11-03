#!/bin/sh
# The release rehearsal (M3.4): the path a release takes, end to end, with
# nothing published, in the workflow's "Rehearse the release" job after
# the sign job. The lists ci/sign.sh signed (the artifact edel-signed) are
# laid over the update images ci/build.sh put in out/release/, as the
# publish job lays them; the images come from the fleet lane's archive,
# out/upgrade-test/preview/, the same files linked. That release is served
# as GitHub serves one, and ci/upgrade-test.sh's old disk verifies the
# signed list with its own edel and updates from it: slot B must start and
# be confirmed, twice.
#
# A run signed with the sign job's throwaway key hands its public key over
# in out/keys/throwaway/, and the old disk is made to trust it; a run
# signed with the release key writes no key, as every image carries
# images/keys/ (ci/keys-carried.sh). The publish job waits for this one,
# so only a release an installed machine took is ever published.
#
#   sh ci/rehearse.sh   after vm-tests.sh take fleet RUN and edel-signed in out/
set -eu

fail() {
	echo "FAIL: $*"
	exit 1
}

for file in out/release/release.toml out/release/release.toml.sig out/channel/signer; do
	[ -s "$file" ] || fail "$file is missing; the sign job hands it over in edel-signed"
done
images=out/upgrade-test/preview
ls "$images"/*.ext4.gz >/dev/null 2>&1 || fail "$images has no update images; vm-tests.sh take fleet RUN puts them there"
for file in "$images"/*.ext4.gz "$images"/*.*.toml; do
	if [ -e "$file" ]; then ln -f "$file" out/release/; fi
done

signer=$(cat out/channel/signer)
key=
if [ "$signer" = throwaway ]; then
	for key in out/keys/throwaway/*.pub; do break; done
	[ -s "$key" ] || fail "the lists are signed with a throwaway key, but its public key is not in out/keys/throwaway/"
fi
echo "rehearsing version $(sed -n 's/^version = "\(.*\)"$/\1/p' out/release/release.toml) on channel $(sed -n 's/^channel = "\(.*\)"$/\1/p' out/release/release.toml), signed with the $signer key"
UPGRADE_TEST_LIST=out/release UPGRADE_TEST_KEY=$key sh ci/upgrade-test.sh
echo "PASS: an installed older Edel OS took the release as the sign job signed it; nothing was published"
