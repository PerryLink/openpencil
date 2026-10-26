#!/usr/bin/env bash
# Exercise build argument selection without running Cargo or a mobile SDK.
set -euo pipefail

script_dir=$(CDPATH='' cd "$(dirname "$0")" && pwd)
test_root=$(mktemp -d "${TMPDIR:-/tmp}/op-mobile-auth-build.XXXXXX")
trap 'rm -rf "$test_root"' EXIT
mkdir -p "$test_root/bin"
printf '!<arch>\n' > "$test_root/libop_auth.a"
cat > "$test_root/bin/cargo" <<'EOF'
#!/bin/bash
printf '%s\n' "$@" > "$OP_AUTH_TEST_ARGS"
printf '%s\n' "$OPENPENCIL_DEV_OP_AUTH_ABI_VERSION" > "$OP_AUTH_TEST_ABI"
printf '%s\n' "${BINDGEN_EXTRA_CLANG_ARGS_aarch64_linux_android:-}${BINDGEN_EXTRA_CLANG_ARGS_x86_64_linux_android:-}" > "$OP_AUTH_TEST_BINDGEN"
EOF
touch "$test_root/bin/cargo-ndk"
chmod +x "$test_root/bin/cargo" "$test_root/bin/cargo-ndk"
export PATH="$test_root/bin:$PATH"
export OP_AUTH_TEST_ARGS="$test_root/args" OP_AUTH_TEST_ABI="$test_root/abi"
export OP_AUTH_TEST_BINDGEN="$test_root/bindgen"
export ANDROID_NDK_HOME="$test_root/ndk with spaces"
for host_tag in darwin-x86_64 linux-x86_64 windows-x86_64; do
    mkdir -p "$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/$host_tag/sysroot"
    mkdir -p "$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/$host_tag/bin"
    for clang in clang clang.exe; do
        touch "$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/$host_tag/bin/$clang"
        chmod +x "$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/$host_tag/bin/$clang"
    done
done
ANDROID_NDK_HOME=$(cd "$ANDROID_NDK_HOME" && pwd -P)

for platform in ios-simulator ios-device android-arm64 android-x86_64; do
    for abi in 2 3 4; do
        bash "$script_dir/build-mobile-auth-dev.sh" --platform "$platform" \
            --archive "$test_root/libop_auth.a" --abi "$abi" >/dev/null
        feature=mobile-auth-dev
        [[ "$abi" != 4 ]] || feature=mobile-hosted-dev
        grep -q ",$feature$" "$OP_AUTH_TEST_ARGS"
        [[ "$(cat "$OP_AUTH_TEST_ABI")" == "$abi" ]]
        ! grep -Fxq -- '--release' "$OP_AUTH_TEST_ARGS"
        if [[ "$platform" == android-* ]]; then
            grep -Fxq 'packaging/android/app/src/debug/jniLibs' "$OP_AUTH_TEST_ARGS"
            grep -Fxq -- '--platform' "$OP_AUTH_TEST_ARGS"
            grep -Fxq '26' "$OP_AUTH_TEST_ARGS"
            grep -Fq -- "--sysroot=\"$ANDROID_NDK_HOME/" "$OP_AUTH_TEST_BINDGEN"
            target=aarch64-linux-android
            [[ "$platform" != android-x86_64 ]] || target=x86_64-linux-android
            grep -Fq -- "--target=${target}26" "$OP_AUTH_TEST_BINDGEN"
        fi
    done
done
if ANDROID_NDK_HOME="$test_root/missing" bash "$script_dir/build-mobile-auth-dev.sh" \
    --platform android-arm64 --archive "$test_root/libop_auth.a" --abi 4 >/dev/null 2>&1; then
    printf 'error: missing Android NDK was accepted\n' >&2
    exit 1
fi
if bash "$script_dir/build-mobile-auth-dev.sh" --platform ios-simulator \
    --archive "$test_root/libop_auth.a" --abi 5 >/dev/null 2>&1; then
    printf 'error: unsupported ABI was accepted\n' >&2
    exit 1
fi
if bash "$script_dir/build-mobile-auth-dev.sh" --platform ios-simulator \
    --archive "$test_root/libop_auth.a" --abi 4 --release >/dev/null 2>&1; then
    printf 'error: unsigned release build was accepted\n' >&2
    exit 1
fi
printf 'mobile auth Debug build feature and output isolation tests pass\n'
