#!/bin/sh
# Roadmap M2.4: boots the install test image (ci/install-test/files) with a
# blank second disk. Its test service runs `edel install` without --yes
# (the plan, exit code 3, the disk unchanged) and then with --yes. The
# installed disk, out/install-target.img, is what ci/ab-test.sh boots next
# (M2.5); this script saves the live image's ssh host key for it.
# M6.6b: a third blank disk, out/install-tty.img, is installed by
# `edel install` with no arguments on the VM's second serial port, whose
# questions ci/serial.py answers from out/install-tty.sock; the
# transcript is out/install-tty.log, which must not show the password.
# M6.6b: the main install takes its settings file over https, from
# https-serve.py on the runner with a throwaway authority made for this
# run, which the guest fetches over plain http and adds to its own; the
# same file over plain http must be refused.
set -eu
. ci/vm.sh
. ci/names.sh

target=out/install-target.img
rm -f "$target"
# Two 4096 MiB slots, the ESP and room for data (M3.3b). Zeros written to
# it stay holes, so the copied slot costs the runner no disk.
truncate -s 10G "$target"
tty_disk=out/install-tty.img
tty_sock=out/install-tty.sock
rm -f "$tty_disk" "$tty_sock"
truncate -s 10G "$tty_disk"
log=out/install-test.log
fail() {
	cat "$1"
	echo "FAIL: $2"
	exit 1
}

tls=out/install-tls
rm -rf "$tls"
mkdir -p "$tls/www"
openssl req -x509 -newkey rsa:2048 -nodes -days 1 -subj /CN=edel-ci-authority \
	-addext basicConstraints=critical,CA:TRUE -addext keyUsage=critical,keyCertSign,cRLSign \
	-keyout "$tls/ca.key" -out "$tls/www/ca.pem" 2>/dev/null
openssl req -newkey rsa:2048 -nodes -subj /CN=10.0.2.2 -keyout "$tls/server.key" -out "$tls/server.csr" 2>/dev/null
printf 'subjectAltName=IP:10.0.2.2\nextendedKeyUsage=serverAuth\nbasicConstraints=CA:FALSE\n' >"$tls/server.ext"
openssl x509 -req -in "$tls/server.csr" -CA "$tls/www/ca.pem" -CAkey "$tls/ca.key" -CAcreateserial \
	-days 1 -extfile "$tls/server.ext" -out "$tls/server.pem" 2>/dev/null
cp "ci/install-test/files$share_dir/ci/install.toml" "$tls/www/"
python3 ci/https-serve.py "$tls/www" "$tls/server.pem" "$tls/server.key" >out/install-https.log 2>&1 &
server=$!
trap 'kill "$server" 2>/dev/null || :' EXIT

# The answers, in the order edel install asks (edel::install::QUESTIONS);
# an empty answer takes the suggestion.
python3 ci/serial.py "$tty_sock" \
	'yes or no [no]: =yes' 'Language [en_US.UTF-8]: =de_DE.UTF-8' 'Keyboard [de]: =' \
	'Your username: =ali' 'Your password: =edel-ci-tty' 'The same password again: =edel-ci-tty' \
	'Time zone [Europe/Berlin]: =' 'Computer name [ali-vm]: =' 'Layout (=mac-like' 'Profile (=developer' 'such as sda: =vdc' 'Type vdc to erase it=vdc' \
	>out/install-tty.log 2>&1 &
typist=$!
run_vm "$log" 'INSTALL-TEST: (installed|FAIL)' "${INSTALL_TEST_TIMEOUT:-600}" -no-reboot \
	-drive if=none,id=disk0,format=raw,file=out/install-test/edel-vm-x86_64.img,snapshot=on \
	-device virtio-blk-pci,drive=disk0,bootindex=0 \
	-drive if=none,id=disk1,format=raw,file="$target",discard=unmap,detect-zeroes=unmap \
	-device virtio-blk-pci,drive=disk1 \
	-drive if=none,id=disk2,format=raw,file="$tty_disk",discard=unmap,detect-zeroes=unmap \
	-device virtio-blk-pci,drive=disk2 \
	-chardev socket,id=tty1,path="$tty_sock",server=on,wait=off -serial chardev:tty1
kill "$typist" 2>/dev/null || :
grep -q 'INSTALL-TEST: installed' "$log" || fail "$log" "the live image did not install"
grep -q 'Install Edel OS on /dev/vdb' "$log" || fail "$log" "install showed no plan"
grep -q 'INSTALL-TEST: authority trusted' "$log" || fail "$log" "the guest could not fetch the test authority over http"
grep -q 'INSTALL-TEST: http refused: refused: http://10.0.2.2:8080/install.toml is plain http' "$log" ||
	fail "$log" "edel install did not refuse the settings file over plain http"
grep -q 'GET /install.toml' out/install-https.log || fail out/install-https.log "the installer never fetched the settings file over https"
# M1.12: slot A is copied as it was signed, checked against the stick's.
grep -q 'edel install: slot A is the running slot byte for byte' "$log" ||
	fail "$log" "install did not find slot A the running slot byte for byte"
grep -q 'INSTALL-TEST: exit code 3 without --yes, disk unchanged' "$log" ||
	fail "$log" "install without --yes did not exit 3 with the disk unchanged"
# M6.6b: the answers typed on the serial port made the new disk's
# settings file, only the answers, and its password file, root's only.
tty_fail() {
	cat out/install-tty.log
	fail "$log" "$1"
}
value() {
	tr -d '\r' <"$log" | sed -n "s/.*INSTALL-TEST: $1 //p" | tail -n 1
}
[ "$(value 'tty exit')" = 0 ] || tty_fail "edel install with no arguments did not install on vdc (exit $(value 'tty exit'))"
answers=$(value 'tty settings')
for line in '[users.ali]' 'admin = true' 'hostname = "ali-vm"' 'language = "de_DE.UTF-8"' 'keyboard = "de"' 'preset = "mac-like"' 'profiles = ["developer"]'; do
	case $answers in
	*"$line"*) ;;
	*) tty_fail "the installed settings file lacks $line: $answers" ;;
	esac
done
case $answers in
*classic*) tty_fail "the installed settings file holds a default or a key not asked: $answers" ;;
esac
case $answers in
*'timezone = "Europe/Berlin"'*) ;;
*) tty_fail "the installed settings file lacks the time zone answered: $answers" ;;
esac
[ "$(value 'tty passwords')" = '600 ali:$6$' ] ||
	tty_fail "the password file is not ali's hash, root's only: $(value 'tty passwords')"
if grep -q edel-ci-tty out/install-tty.log; then
	tty_fail "the password was echoed on the terminal"
fi
tr -d '\r' <"$log" | sed -n 's/.*INSTALL-TEST: host key //p' | head -n 1 >out/install-live-key
[ -s out/install-live-key ] || fail "$log" "the live image printed no ssh host key"
cat "$log"
echo "PASS: install refused its settings file over plain http, took it over https from a server its own authority vouched for, showed its plan and exited 3 without --yes, then installed onto the blank disk, slot A byte for byte the stick's; with no arguments it asked its questions on a serial terminal, the password unechoed, and installed vdc with only the answers in its settings file and ali's password hash root's only, the profile picked among them (${waited}s)"
