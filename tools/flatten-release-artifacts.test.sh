#!/usr/bin/env bash

set -euo pipefail

repo_root=$(cd "$(dirname "$0")/.." && pwd)
helper=$repo_root/tools/flatten-release-artifacts.sh
temporary=$(mktemp -d)
cleanup() {
    rm -rf "$temporary"
}
trap cleanup EXIT HUP INT TERM

make_fixture() {
    local root=$1
    mkdir -p "$root/desktop" "$root/cli" "$root/sdk" "$root/vsix" "$root/android"
    printf 'desktop\n' > "$root/desktop/openpencil-desktop-linux-x86_64.tar.gz"
    for asset in OpenPencil-0.8.5-x64-win-setup.exe openpencil-desktop-windows-x86_64.zip \
        OpenPencil-0.8.5-arm64-mac.dmg OpenPencil-0.8.5-x64-linux.deb OpenPencil-0.8.5-x64-linux.AppImage; do
        printf 'desktop-%s\n' "$asset" > "$root/desktop/$asset"
    done
    printf 'cli\n' > "$root/cli/op-cli-linux-x86_64.tar.gz"
    for index in 1 2 3; do
        printf 'sdk-%s\n' "$index" > "$root/sdk/sdk-${index}.tgz"
    done
    for index in 1 2 3 4 5 6; do
        printf 'vsix-%s\n' "$index" > "$root/vsix/editor-${index}.vsix"
    done
    printf 'apk\n' > "$root/android/OpenPencil-0.8.5-android.apk"
    printf 'aab\n' > "$root/android/OpenPencil-0.8.5-android.aab"
    (
        cd "$root/android"
        sha256sum OpenPencil-0.8.5-android.aab OpenPencil-0.8.5-android.apk \
            > SHA256SUMS.android.txt
    )
    printf 'ignored\n' > "$root/android/internal-build.log"
}

make_daemon_handoffs() {
    local root=$1 label binary
    for label in linux-aarch64 linux-x86_64 macos-aarch64 macos-x86_64 windows-aarch64 windows-x86_64; do
        binary=op-host-web-server
        case "$label" in windows-*) binary=$binary.exe;; esac
        mkdir -p "$root/openpencil-daemon-$label"
        printf 'daemon-%s\n' "$label" > "$root/openpencil-daemon-$label/$binary"
    done
}

expect_rejected() {
    local label=$1
    local input=$2
    local output=$3
    if "$helper" "$input" "$output" 0.8.5 >/dev/null 2>&1; then
        printf 'error: accepted invalid fixture: %s\n' "$label" >&2
        exit 1
    fi
    if [ -e "$output" ]; then
        printf 'error: invalid fixture left release output: %s\n' "$label" >&2
        exit 1
    fi
}

valid=$temporary/valid
make_fixture "$valid"
"$helper" "$valid" "$temporary/release" 0.8.5 >/dev/null
test -f "$temporary/release/OpenPencil-0.8.5-android.apk"
test -f "$temporary/release/OpenPencil-0.8.5-android.aab"
test ! -e "$temporary/release/internal-build.log"

with_daemons=$temporary/with-daemons
make_fixture "$with_daemons"
make_daemon_handoffs "$with_daemons"
"$helper" "$with_daemons/" "$temporary/daemon-free-release" 0.8.5 >/dev/null
diff -r "$temporary/release" "$temporary/daemon-free-release"
for label in linux-aarch64 linux-x86_64 macos-aarch64 macos-x86_64 windows-aarch64 windows-x86_64; do
    binary=op-host-web-server
    case "$label" in windows-*) binary=$binary.exe;; esac
    test -s "$with_daemons/openpencil-daemon-$label/$binary"
done

outside_handoff=$temporary/outside-handoff
make_fixture "$outside_handoff"
make_daemon_handoffs "$outside_handoff"
printf 'public executable\n' > "$outside_handoff/desktop/op-host-web-server.exe"
"$helper" "$outside_handoff" "$temporary/outside-release" 0.8.5 >/dev/null
cmp "$outside_handoff/desktop/op-host-web-server.exe" "$temporary/outside-release/op-host-web-server.exe"

for invalid in extra-file hidden-file extra-directory wrong-name empty missing unknown nested directory-file root-file symlink; do
    input=$temporary/daemon-$invalid
    make_fixture "$input"
    handoff=$input/openpencil-daemon-windows-x86_64
    mkdir -p "$handoff"
    printf 'daemon\n' > "$handoff/op-host-web-server.exe"
    case "$invalid" in
        extra-file) printf extra > "$handoff/extra.exe";;
        hidden-file) printf extra > "$handoff/.metadata";;
        extra-directory) mkdir "$handoff/extra";;
        wrong-name) mv "$handoff/op-host-web-server.exe" "$handoff/op-host-web-server";;
        empty) : > "$handoff/op-host-web-server.exe";;
        missing) rm "$handoff/op-host-web-server.exe";;
        unknown) mv "$handoff" "$input/openpencil-daemon-freebsd-x86_64";;
        nested) mkdir "$input/other"; mv "$handoff" "$input/other/";;
        directory-file) rm "$handoff/op-host-web-server.exe"; mkdir "$handoff/op-host-web-server.exe";;
        root-file) rm -r "$handoff"; printf invalid > "$handoff";;
        symlink) rm "$handoff/op-host-web-server.exe"; ln -s ../sdk/sdk-1.tgz "$handoff/op-host-web-server.exe";;
    esac
    expect_rejected "daemon-$invalid" "$input" "$temporary/daemon-$invalid-output"
done

duplicate_daemon_name=$temporary/duplicate-daemon-name
make_fixture "$duplicate_daemon_name"
make_daemon_handoffs "$duplicate_daemon_name"
mkdir -p "$duplicate_daemon_name/other-one" "$duplicate_daemon_name/other-two"
printf one > "$duplicate_daemon_name/other-one/op-host-web-server.exe"
printf two > "$duplicate_daemon_name/other-two/op-host-web-server.exe"
expect_rejected unrelated-daemon-basename "$duplicate_daemon_name" "$temporary/duplicate-daemon-name-output"

duplicate=$temporary/duplicate
make_fixture "$duplicate"
mkdir -p "$duplicate/other"
cp "$duplicate/sdk/sdk-1.tgz" "$duplicate/other/sdk-1.tgz"
expect_rejected duplicate-basename "$duplicate" "$temporary/duplicate-output"

bad_checksum=$temporary/bad-checksum
make_fixture "$bad_checksum"
printf 'tampered\n' >> "$bad_checksum/android/OpenPencil-0.8.5-android.apk"
expect_rejected invalid-android-checksum "$bad_checksum" "$temporary/checksum-output"

extra_apk=$temporary/extra-apk
make_fixture "$extra_apk"
printf 'extra\n' > "$extra_apk/android/OpenPencil-0.8.5-debug.apk"
expect_rejected extra-android-apk "$extra_apk" "$temporary/extra-apk-output"

symlinked=$temporary/symlinked
make_fixture "$symlinked"
ln -s ../sdk/sdk-1.tgz "$symlinked/android/linked.tgz"
expect_rejected symlink "$symlinked" "$temporary/symlink-output"

printf 'flatten-release-artifacts.test.sh: release asset fixtures passed.\n'
