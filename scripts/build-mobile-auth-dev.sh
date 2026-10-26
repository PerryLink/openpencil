#!/usr/bin/env bash
# Build one mobile editor target against an unsigned local op-auth archive.
# This script intentionally has no release mode.

set -euo pipefail

script_dir=$(CDPATH= cd "$(dirname "$0")" && pwd)
repo_root=$(CDPATH= cd "$script_dir/.." && pwd)
platform=
archive=
abi=

usage() {
    cat >&2 <<'EOF'
usage: scripts/build-mobile-auth-dev.sh \
  --platform ios-simulator|ios-device|android-arm64|android-x86_64 \
  --archive /absolute/path/to/libop_auth.a --abi 2|3|4

Builds a Cargo debug artifact only. Android output is isolated under
packaging/android/app/src/debug/jniLibs; iOS keeps libop_auth.a as an
explicit final Xcode link input.
EOF
}

while [[ "$#" -gt 0 ]]; do
    case "$1" in
        --platform|--archive|--abi)
            [[ "$#" -ge 2 ]] || {
                usage
                exit 2
            }
            name=${1#--}
            printf -v "$name" '%s' "$2"
            shift 2
            ;;
        *)
            usage
            exit 2
            ;;
    esac
done

[[ -n "$platform" && -n "$archive" && -n "$abi" ]] || {
    usage
    exit 2
}
case "$abi" in
    2|3|4) ;;
    *)
        printf 'error: --abi must be 2, 3, or 4\n' >&2
        exit 2
        ;;
esac
case "$archive" in
    /*) ;;
    *)
        printf 'error: --archive must be an absolute path\n' >&2
        exit 2
        ;;
esac
if [[ -L "$archive" || ! -f "$archive" || "$(basename "$archive")" != libop_auth.a ]]; then
    printf 'error: --archive must select a regular non-symlink libop_auth.a\n' >&2
    exit 2
fi
archive=$(cd "$(dirname "$archive")" && pwd -P)/libop_auth.a

export OPENPENCIL_DEV_OP_AUTH_ARCHIVE="$archive"
export OPENPENCIL_DEV_OP_AUTH_ABI_VERSION="$abi"
cd "$repo_root"

feature=mobile-auth-dev
if [[ "$abi" == 4 ]]; then
    feature=mobile-hosted-dev
fi

configure_android_bindgen() {
    local target=$1 ndk_root host_tag toolchain sysroot bindgen_key extra_args clang
    ndk_root=${ANDROID_NDK_HOME:-${ANDROID_NDK_ROOT:-${ANDROID_NDK:-}}}
    if [[ -z "$ndk_root" || ! -d "$ndk_root" ]]; then
        printf 'error: Android Debug builds require ANDROID_NDK_HOME pointing to an installed NDK\n' >&2
        exit 1
    fi
    ndk_root=$(cd "$ndk_root" && pwd -P)
    case "$(uname -s)" in
        Darwin) host_tag=darwin-x86_64 ;;
        Linux) host_tag=linux-x86_64 ;;
        MINGW*|MSYS*|CYGWIN*) host_tag=windows-x86_64 ;;
        *) printf 'error: unsupported Android build host\n' >&2; exit 1 ;;
    esac
    toolchain=$ndk_root/toolchains/llvm/prebuilt/$host_tag
    sysroot=$toolchain/sysroot
    clang=$toolchain/bin/clang
    [[ "$host_tag" != windows-x86_64 ]] || clang=$clang.exe
    if [[ ! -d "$sysroot" || ! -x "$clang" ]]; then
        printf 'error: Android NDK sysroot or clang is missing for %s\n' "$host_tag" >&2
        exit 1
    fi
    # cargo-ndk 3.x does not configure bindgen by default. QuickJS must parse
    # against the same Android API/sysroot as cc, never the host's SDK.
    bindgen_key=BINDGEN_EXTRA_CLANG_ARGS_${target//-/_}
    extra_args=${!bindgen_key-}
    printf -v "$bindgen_key" '%s' "--target=${target}26 --sysroot=\"$sysroot\" $extra_args"
    export "$bindgen_key"
    export ANDROID_NDK_HOME="$ndk_root" ANDROID_NDK="$ndk_root" CLANG_PATH="$clang"
}

case "$platform" in
    ios-simulator)
        target=aarch64-apple-ios-sim
        cargo build --locked -p op-engine-ffi --target "$target" \
            --features "metal,editor,$feature"
        printf 'built %s\n' "$repo_root/target/$target/debug/libop_engine_ffi.a"
        printf 'Xcode auth link input: OP_AUTH_ARCHIVE=%s\n' "$archive"
        ;;
    ios-device)
        target=aarch64-apple-ios
        cargo build --locked -p op-engine-ffi --target "$target" \
            --features "metal,editor,$feature"
        printf 'built %s\n' "$repo_root/target/$target/debug/libop_engine_ffi.a"
        printf 'Xcode auth link input: OP_AUTH_ARCHIVE=%s\n' "$archive"
        ;;
    android-arm64)
        command -v cargo-ndk >/dev/null 2>&1 || {
            printf 'error: cargo-ndk is required\n' >&2
            exit 1
        }
        configure_android_bindgen aarch64-linux-android
        cargo ndk -t arm64-v8a --platform 26 \
            -o packaging/android/app/src/debug/jniLibs \
            build --locked -p op-engine-jni --features "gl,editor,$feature"
        ;;
    android-x86_64)
        command -v cargo-ndk >/dev/null 2>&1 || {
            printf 'error: cargo-ndk is required\n' >&2
            exit 1
        }
        configure_android_bindgen x86_64-linux-android
        cargo ndk -t x86_64 --platform 26 \
            -o packaging/android/app/src/debug/jniLibs \
            build --locked -p op-engine-jni --features "gl,editor,$feature"
        ;;
    *)
        printf 'error: unsupported --platform: %s\n' "$platform" >&2
        usage
        exit 2
        ;;
esac
