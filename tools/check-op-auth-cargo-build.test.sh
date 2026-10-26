#!/usr/bin/env bash
# Secret-free regression tests for the Cargo production-auth cfg gate.

set -euo pipefail

script_dir=$(CDPATH='' cd "$(dirname "$0")" && pwd)
temp_root=$(mktemp -d "${TMPDIR:-/tmp}/op-auth-cargo-test.XXXXXX")
target=x86_64-unknown-linux-gnu
output_dir=$temp_root/$target/release/build/op-auth-bridge-fixture
output=$output_dir/output

cleanup() {
    rm -rf "$temp_root"
}
trap cleanup EXIT

mkdir -p "$output_dir"
legacy_repo=$temp_root/abi3-repo
mkdir -p "$legacy_repo/tools" "$legacy_repo/crates/op-auth-bridge"
cp "$script_dir/check-op-auth-cargo-build.sh" "$legacy_repo/tools/"
printf 'format=op-auth-release-policy-v1\nabi=3\n' > "$legacy_repo/crates/op-auth-bridge/AUTH-RELEASE-POLICY"
checker=$legacy_repo/tools/check-op-auth-cargo-build.sh
write_valid_output() {
    printf '%s\n' \
        cargo:rustc-cfg=op_auth_prebuilt \
        cargo:rustc-cfg=op_auth_collab_ticket_prebuilt \
        cargo:rustc-cfg=op_auth_collab_relay_token_prebuilt \
        cargo:rustc-env=OP_AUTH_PREBUILT_ABI_VERSION=3 \
        > "$output"
}

write_valid_output
CARGO_TARGET_DIR=$temp_root \
OP_AUTH_CARGO_TARGET=$target \
    "$checker" >/dev/null

android_target=aarch64-linux-android
android_output_dir=$temp_root/$android_target/release/build/op-auth-bridge-fixture
mkdir -p "$android_output_dir"
cp "$output" "$android_output_dir/output"
CARGO_TARGET_DIR=$temp_root \
OP_AUTH_CARGO_TARGET=$android_target \
    "$checker" >/dev/null

ios_target=aarch64-apple-ios
ios_output_dir=$temp_root/$ios_target/release/build/op-auth-bridge-fixture
mkdir -p "$ios_output_dir"
cp "$output" "$ios_output_dir/output"
CARGO_TARGET_DIR=$temp_root \
OP_AUTH_CARGO_TARGET=$ios_target \
    "$checker" >/dev/null

printf '%s\n' \
    cargo:rustc-cfg=op_auth_prebuilt \
    cargo:rustc-cfg=op_auth_collab_ticket_prebuilt \
    cargo:rustc-env=OP_AUTH_PREBUILT_ABI_VERSION=3 \
    > "$output"
if CARGO_TARGET_DIR=$temp_root \
    OP_AUTH_CARGO_TARGET=$target \
    "$checker" >/dev/null 2>&1; then
    printf 'error: missing ABI 3 relay-token cfg was accepted\n' >&2
    exit 1
fi

write_valid_output
printf '%s\n' cargo:rustc-cfg=op_auth_development_prebuilt >> "$output"
if CARGO_TARGET_DIR=$temp_root \
    OP_AUTH_CARGO_TARGET=$target \
    "$checker" >/dev/null 2>&1; then
    printf 'error: development auth cfg was accepted\n' >&2
    exit 1
fi

printf 'check-op-auth-cargo-build.test.sh: production cfg gates passed.\n'

fixture_repo=$temp_root/abi4-repo
mkdir -p "$fixture_repo/tools" "$fixture_repo/crates/op-auth-bridge"
cp "$script_dir/check-op-auth-cargo-build.sh" "$fixture_repo/tools/"
printf 'format=op-auth-release-policy-v1\nabi=4\n' > "$fixture_repo/crates/op-auth-bridge/AUTH-RELEASE-POLICY"
write_valid_output
sed 's/ABI_VERSION=3/ABI_VERSION=4/' "$output" > "$output.next"
mv "$output.next" "$output"
if CARGO_TARGET_DIR=$temp_root OP_AUTH_CARGO_TARGET=$target \
    "$fixture_repo/tools/check-op-auth-cargo-build.sh" >/dev/null 2>&1; then
    printf 'error: ABI 4 without Hosted cfg was accepted\n' >&2
    exit 1
fi
printf '%s\n' cargo:rustc-cfg=op_auth_hosted_access_prebuilt >> "$output"
CARGO_TARGET_DIR=$temp_root OP_AUTH_CARGO_TARGET=$target \
    "$fixture_repo/tools/check-op-auth-cargo-build.sh" >/dev/null
if CARGO_TARGET_DIR=$temp_root OP_AUTH_CARGO_TARGET=$target \
    "$checker" >/dev/null 2>&1; then
    printf 'error: ABI 4 Cargo output was accepted under the ABI 3 policy\n' >&2
    exit 1
fi
printf 'check-op-auth-cargo-build.test.sh: adopted Hosted ABI 4 cfg gates passed.\n'
