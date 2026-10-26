#!/usr/bin/env bash

set -euo pipefail

script_dir=$(CDPATH= cd "$(dirname "$0")" && pwd)
checker=$script_dir/check-mobile-auth-link-input.sh
test_dir=$(mktemp -d "${TMPDIR:-/tmp}/op-mobile-auth-link-test.XXXXXX")
cleanup() {
    rm -rf "$test_dir"
}
trap cleanup EXIT

archive=$test_dir/libop_auth.a
printf '!<arch>\n' > "$archive"

if grep -Fq 'workspace_version' "$checker" \
    || ! grep -Fq '"version=$artifact_version"' "$checker" \
    || ! grep -Fq '[[ "$abi" == 3 || "$abi" == 4 ]]' "$checker"; then
    printf 'not ok - Release link gate must trust signed version metadata and require ABI 3 or 4\n' >&2
    exit 1
fi

CONFIGURATION=Debug \
OP_AUTH_ARCHIVE="$archive" \
OPENPENCIL_DEV_OP_AUTH_ABI_VERSION=3 \
    bash "$checker" >/dev/null

CONFIGURATION=Debug \
OP_AUTH_ARCHIVE="$archive" \
OPENPENCIL_DEV_OP_AUTH_ABI_VERSION=4 \
    bash "$checker" >/dev/null

signed_archive=$script_dir/../crates/op-auth-bridge/prebuilt/aarch64-apple-ios-sim/libop_auth.a
CONFIGURATION=Release \
OP_AUTH_ARCHIVE="$(cd "$(dirname "$signed_archive")" && pwd -P)/libop_auth.a" \
OP_AUTH_TARGET=aarch64-apple-ios-sim \
    bash "$checker" >/dev/null

expect_failure() {
    label=$1
    expected=$2
    shift 2
    set +e
    output=$("$@" 2>&1)
    result=$?
    set -e
    if [[ "$result" -eq 0 || "$output" != *"$expected"* ]]; then
        printf 'not ok - %s\n%s\n' "$label" "$output" >&2
        exit 1
    fi
    printf 'ok - %s\n' "$label"
}

expect_failure \
    "Debug rejects unsupported ABI" \
    "requires OPENPENCIL_DEV_OP_AUTH_ABI_VERSION=2, 3, or 4" \
    env CONFIGURATION=Debug OP_AUTH_ARCHIVE="$archive" \
        OPENPENCIL_DEV_OP_AUTH_ABI_VERSION=1 bash "$checker"

expect_failure \
    "Release rejects an unsigned external archive" \
    "Release may link only the repository signed archive" \
    env CONFIGURATION=Release OP_AUTH_ARCHIVE="$archive" \
        OP_AUTH_TARGET=aarch64-apple-ios-sim bash "$checker"

expect_failure \
    "Android Release rejects an unsigned external archive" \
    "Release may link only the repository signed archive" \
    env CONFIGURATION=Release OP_AUTH_ARCHIVE="$archive" \
        OP_AUTH_TARGET=aarch64-linux-android bash "$checker"

expect_failure \
    "Release rejects the development ABI selector" \
    "unsigned development auth lane is forbidden in Release" \
    env CONFIGURATION=Release OP_AUTH_ARCHIVE="$archive" \
        OP_AUTH_TARGET=aarch64-apple-ios-sim \
        OPENPENCIL_DEV_OP_AUTH_ABI_VERSION=3 bash "$checker"

expect_failure \
    "Release rejects an unsupported mobile target" \
    "requires an explicit supported OP_AUTH_TARGET" \
    env CONFIGURATION=Release OP_AUTH_ARCHIVE="$archive" \
        OP_AUTH_TARGET=armv7-linux-androideabi bash "$checker"

# Use the genuine signed archive in a disposable source checkout. Changing only
# the adoption ABI must fail even with ambient strict-promotion inputs set.
fixture_repo=$test_dir/repo
fixture_prebuilt=$fixture_repo/crates/op-auth-bridge/prebuilt
mkdir -p "$fixture_repo/tools" "$fixture_prebuilt/aarch64-apple-ios-sim"
cp "$checker" "$script_dir/check-op-auth-release-matrix.sh" "$fixture_repo/tools/"
cp "$script_dir/../crates/op-auth-bridge/prebuilt/PROVENANCE_PUBKEY" \
    "$script_dir/../crates/op-auth-bridge/prebuilt/RELEASE-MANIFEST" \
    "$script_dir/../crates/op-auth-bridge/prebuilt/RELEASE-MANIFEST.sig" \
    "$fixture_prebuilt/"
cp "$(dirname "$signed_archive")/"* "$fixture_prebuilt/aarch64-apple-ios-sim/"
adopted_abi=$(sed -n '2s/^abi=//p' "$script_dir/../crates/op-auth-bridge/AUTH-RELEASE-POLICY")
other_abi=4
[[ "$adopted_abi" != 4 ]] || other_abi=3
sed "s/^abi=$adopted_abi$/abi=$other_abi/" "$script_dir/../crates/op-auth-bridge/AUTH-RELEASE-POLICY" \
    > "$fixture_repo/crates/op-auth-bridge/AUTH-RELEASE-POLICY"
expect_failure \
    "Release requires source adoption even with promotion inputs" \
    "release matrix ABI does not match source policy" \
    env CONFIGURATION=Release \
        OP_AUTH_ARCHIVE="$fixture_prebuilt/aarch64-apple-ios-sim/libop_auth.a" \
        OP_AUTH_TARGET=aarch64-apple-ios-sim \
        OP_AUTH_RELEASE_WORKSPACE_VERSION=0.8.5 \
        OP_AUTH_RELEASE_EXPECTED_OPENPENCIL_REVISION=1111111111111111111111111111111111111111 \
        bash "$fixture_repo/tools/check-mobile-auth-link-input.sh"

printf 'mobile auth link-input contract tests pass\n'
