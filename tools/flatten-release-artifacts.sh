#!/usr/bin/env bash

set -euo pipefail

if [ "$#" -ne 3 ]; then
    printf 'usage: %s <download-root> <release-root> <version>\n' "$0" >&2
    exit 2
fi

source_root=$1
release_root=$2
version=$3

if [ ! -d "$source_root" ] || [ -L "$source_root" ]; then
    printf 'error: artifact download root must be a real directory: %s\n' "$source_root" >&2
    exit 1
fi
source_root=$(CDPATH='' cd "$source_root" && pwd -P)
if [ -e "$release_root" ] || [ -L "$release_root" ]; then
    printf 'error: release output must not already exist: %s\n' "$release_root" >&2
    exit 1
fi
if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    printf 'error: release version must be stable SemVer: %s\n' "$version" >&2
    exit 1
fi
if find "$source_root" -type l -print -quit | grep -q .; then
    printf 'error: artifact download tree contains a symbolic link\n' >&2
    exit 1
fi

# These are inputs to platform VSIX packages, not standalone release assets.
# Validate their exact producer layout before excluding any files from collection.
while IFS= read -r -d '' handoff; do
    handoff_name=${handoff##*/}
    case "$handoff_name" in
        openpencil-daemon-linux-aarch64 | openpencil-daemon-linux-x86_64 | \
        openpencil-daemon-macos-aarch64 | openpencil-daemon-macos-x86_64)
            daemon_name=op-host-web-server
            ;;
        openpencil-daemon-windows-aarch64 | openpencil-daemon-windows-x86_64)
            daemon_name=op-host-web-server.exe
            ;;
        *)
            printf 'error: unrecognized internal daemon artifact: %s\n' "$handoff_name" >&2
            exit 1
            ;;
    esac
    if [ "${handoff%/*}" != "$source_root" ] || [ ! -d "$handoff" ]; then
        printf 'error: internal daemon artifact must be a top-level directory: %s\n' "$handoff" >&2
        exit 1
    fi
    daemon_entries=0
    while IFS= read -r -d '' entry; do
        daemon_entries=$((daemon_entries + 1))
        if [ "$entry" != "$handoff/$daemon_name" ] || [ ! -f "$entry" ] || [ ! -s "$entry" ]; then
            printf 'error: unexpected internal daemon artifact contents: %s\n' "$handoff" >&2
            exit 1
        fi
    done < <(find "$handoff" -mindepth 1 -maxdepth 1 -print0)
    if [ "$daemon_entries" -ne 1 ]; then
        printf 'error: internal daemon artifact must contain exactly one %s: %s\n' "$daemon_name" "$handoff" >&2
        exit 1
    fi
done < <(find "$source_root" -mindepth 1 -name 'openpencil-daemon-*' -print0)

temporary=$(mktemp -d "${release_root}.tmp.XXXXXX")
cleanup() {
    rm -rf "$temporary"
}
trap cleanup EXIT HUP INT TERM

copied=0
while IFS= read -r -d '' artifact; do
    relative=${artifact#"$source_root"/}
    case "${relative%%/*}" in
        openpencil-daemon-*) continue;; # Exact directory names and contents checked above.
    esac
    name=${artifact##*/}
    case "$name" in
        *.tar.gz | *.zip | *.dmg | *.exe | *.AppImage | *.deb | *.tgz | *.vsix | *.apk | *.aab | SHA256SUMS.android.txt)
            ;;
        *)
            continue
            ;;
    esac
    if [ -e "$temporary/$name" ]; then
        printf 'error: duplicate release artifact basename: %s\n' "$name" >&2
        exit 1
    fi
    cp "$artifact" "$temporary/$name"
    copied=$((copied + 1))
done < <(find "$source_root" -type f -print0 | LC_ALL=C sort -z)

if [ "$copied" -eq 0 ]; then
    printf 'error: no release artifacts were found\n' >&2
    exit 1
fi

shopt -s nullglob
native=("$temporary"/OpenPencil-* "$temporary"/openpencil-desktop-*)
cli=("$temporary"/op-cli-*)
sdk=("$temporary"/*.tgz)
vsix=("$temporary"/*.vsix)
android_apk=("$temporary"/*.apk)
android_aab=("$temporary"/*.aab)

if [ "${#native[@]}" -eq 0 ]; then
    printf 'error: missing native desktop artifacts\n' >&2
    exit 1
fi
if [ "${#cli[@]}" -eq 0 ]; then
    printf 'error: missing op-cli artifacts\n' >&2
    exit 1
fi
for required in \
    openpencil-desktop-linux-x86_64.tar.gz \
    op-cli-linux-x86_64.tar.gz; do
    if [ ! -f "$temporary/$required" ]; then
        printf 'error: missing Linux x86_64 Nix manifest asset: %s\n' "$required" >&2
        exit 1
    fi
done
if [ "${#sdk[@]}" -ne 3 ]; then
    printf 'error: expected 3 SDK tarballs, found %s\n' "${#sdk[@]}" >&2
    exit 1
fi
if [ "${#vsix[@]}" -ne 6 ]; then
    printf 'error: expected 6 platform VS Code extension files, found %s\n' "${#vsix[@]}" >&2
    exit 1
fi

expected_apk="OpenPencil-${version}-android.apk"
expected_aab="OpenPencil-${version}-android.aab"
if [ "${#android_apk[@]}" -ne 1 ] || [ "${android_apk[0]##*/}" != "$expected_apk" ]; then
    printf 'error: missing exact signed Android APK\n' >&2
    exit 1
fi
if [ "${#android_aab[@]}" -ne 1 ] || [ "${android_aab[0]##*/}" != "$expected_aab" ]; then
    printf 'error: missing exact signed Android App Bundle\n' >&2
    exit 1
fi

android_sums=$temporary/SHA256SUMS.android.txt
if [ ! -f "$android_sums" ]; then
    printf 'error: missing Android signer checksum handoff\n' >&2
    exit 1
fi
checksum_names=$(awk 'NF == 2 && $1 ~ /^[0-9a-f]{64}$/ { print $2 }' "$android_sums" | LC_ALL=C sort)
expected_names=$(printf '%s\n%s\n' "$expected_aab" "$expected_apk" | LC_ALL=C sort)
if [ "$(awk 'END { print NR }' "$android_sums")" -ne 2 ] \
    || [ "$checksum_names" != "$expected_names" ]; then
    printf 'error: Android checksum handoff must name only the exact APK and AAB\n' >&2
    exit 1
fi
(cd "$temporary" && sha256sum --check SHA256SUMS.android.txt)

mv "$temporary" "$release_root"
trap - EXIT HUP INT TERM
printf 'flatten-release-artifacts: staged %s verified release files\n' "$copied"
