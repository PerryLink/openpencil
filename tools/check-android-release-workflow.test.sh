#!/usr/bin/env bash

set -euo pipefail

script_dir=$(CDPATH='' cd "$(dirname "$0")" && pwd)
repo_root=$(CDPATH='' cd "$script_dir/.." && pwd)
checker=$script_dir/check-android-release-workflow.sh

"$checker" >/dev/null

temporary=$(mktemp -d)
trap 'rm -rf "$temporary"' EXIT
mutation_index=0

expect_rejected() {
    local label=$1 override=$2 source=$3 from=$4 to=$5
    local fixture log
    mutation_index=$((mutation_index + 1))
    fixture=$temporary/mutation-$mutation_index
    log=$temporary/mutation-$mutation_index.log
    ruby - "$source" "$fixture" "$from" "$to" <<'RUBY'
source, destination, from, to = ARGV
data = File.binread(source)
abort "mutation source literal is missing" unless data.sub!(from, to)
File.binwrite(destination, data)
RUBY
    if env "$override=$fixture" "$checker" >"$log" 2>&1; then
        printf 'error: Android release mutation was accepted: %s\n' "$label" >&2
        exit 1
    fi
}

workflow=$repo_root/.github/workflows/android-release.yml
caller=$repo_root/.github/workflows/rust-release.yml
signer=$repo_root/scripts/sign-android-release.sh
app_gradle=$repo_root/packaging/android/app/build.gradle.kts
sdk_installer=$repo_root/tools/install-pinned-android-sdk.sh
verification=$repo_root/packaging/android/gradle/verification-metadata.xml

for name in \
    ANDROID_RELEASE_KEYSTORE_BASE64 \
    ANDROID_RELEASE_KEYSTORE_PASSWORD \
    ANDROID_RELEASE_KEY_ALIAS \
    ANDROID_RELEASE_KEY_PASSWORD; do
    declaration=$(printf '      %s:\n        description: Android release signing credential scoped to the protected signer\n        required: true' "$name")
    forwarding=$(printf '      %s: ${{ secrets.%s }}' "$name" "$name")
    misrouted=$(printf '      %s: ${{ secrets.OPENPENCIL_BUILD_COLLAB_BOOTSTRAP_URL_CN }}' "$name")
    expect_rejected \
        "missing signing secret declaration: $name" \
        OPENPENCIL_ANDROID_RELEASE_WORKFLOW "$workflow" "$declaration" ''
    expect_rejected \
        "optional signing secret declaration: $name" \
        OPENPENCIL_ANDROID_RELEASE_WORKFLOW "$workflow" \
        "$declaration" "${declaration/required: true/required: false}"
    expect_rejected \
        "missing signing secret forwarding: $name" \
        OPENPENCIL_ANDROID_RELEASE_CALLER "$caller" "$forwarding" ''
    expect_rejected \
        "misrouted signing secret forwarding: $name" \
        OPENPENCIL_ANDROID_RELEASE_CALLER "$caller" "$forwarding" "$misrouted"
done
caller_secret_block='    secrets:'
for name in \
    OPENPENCIL_BUILD_COLLAB_BOOTSTRAP_URL_CN \
    OPENPENCIL_BUILD_COLLAB_BOOTSTRAP_URL_GLOBAL \
    ANDROID_RELEASE_KEYSTORE_BASE64 \
    ANDROID_RELEASE_KEYSTORE_PASSWORD \
    ANDROID_RELEASE_KEY_ALIAS \
    ANDROID_RELEASE_KEY_PASSWORD; do
    forwarding=$(printf '      %s: ${{ secrets.%s }}' "$name" "$name")
    caller_secret_block+=$'\n'"$forwarding"
done
expect_rejected \
    'broad secret inheritance' OPENPENCIL_ANDROID_RELEASE_CALLER "$caller" \
    "$caller_secret_block" '    secrets: inherit'
expect_rejected \
    'signer receives the wrong password secret' OPENPENCIL_ANDROID_RELEASE_WORKFLOW "$workflow" \
    '${{ secrets.ANDROID_RELEASE_KEY_PASSWORD }}' \
    '${{ secrets.ANDROID_RELEASE_KEY_ALIAS }}'

expect_rejected \
    'independent dispatch trigger' OPENPENCIL_ANDROID_RELEASE_WORKFLOW "$workflow" \
    $'"on":\n  workflow_call:' \
    $'"on":\n  workflow_dispatch:\n  workflow_call:'
expect_rejected \
    'mutable checkout action' OPENPENCIL_ANDROID_RELEASE_WORKFLOW "$workflow" \
    'actions/checkout@08eba0b27e820071cde6df949e0beb9ba4906955' \
    'actions/checkout@v4'
expect_rejected \
    'signer outside protected environment' OPENPENCIL_ANDROID_RELEASE_WORKFLOW "$workflow" \
    'environment: release-production' 'environment: release-staging'
expect_rejected \
    'keystore exposed to build runner' OPENPENCIL_ANDROID_RELEASE_WORKFLOW "$workflow" \
    $'          OPENPENCIL_BUILD_COLLAB_BOOTSTRAP_URL_GLOBAL: ${{ secrets.OPENPENCIL_BUILD_COLLAB_BOOTSTRAP_URL_GLOBAL }}\n          OPENPENCIL_RELEASE_SOURCE_SHA:' \
    $'          OPENPENCIL_BUILD_COLLAB_BOOTSTRAP_URL_GLOBAL: ${{ secrets.OPENPENCIL_BUILD_COLLAB_BOOTSTRAP_URL_GLOBAL }}\n          ANDROID_RELEASE_KEYSTORE_BASE64: ${{ secrets.ANDROID_RELEASE_KEYSTORE_BASE64 }}\n          OPENPENCIL_RELEASE_SOURCE_SHA:'
expect_rejected \
    'certificate fingerprint moved to a secret' OPENPENCIL_ANDROID_RELEASE_WORKFLOW "$workflow" \
    '${{ vars.ANDROID_RELEASE_CERT_SHA256 }}' \
    '${{ secrets.ANDROID_RELEASE_CERT_SHA256 }}'
expect_rejected \
    'nested artifact extraction' OPENPENCIL_ANDROID_RELEASE_WORKFLOW "$workflow" \
    'merge-multiple: true' 'merge-multiple: false'
expect_rejected \
    'sdkmanager on protected signer' OPENPENCIL_ANDROID_RELEASE_WORKFLOW "$workflow" \
    $'          tools/pinned-release-tools.sh android-signing-tools \\\n            "$RUNNER_TEMP/android-signing-tools"' \
    $'          sdkmanager "build-tools;36.0.0"\n          tools/pinned-release-tools.sh android-signing-tools \\\n            "$RUNNER_TEMP/android-signing-tools"'
expect_rejected \
    'Cargo invocation on protected signer' OPENPENCIL_ANDROID_RELEASE_SIGNER "$signer" \
    'chmod 0644 "$ANDROID_SIGNED_OUTPUT_DIR"/*' \
    $'cargo build --release\nchmod 0644 "$ANDROID_SIGNED_OUTPUT_DIR"/*'
expect_rejected \
    'API target downgrade' OPENPENCIL_ANDROID_APP_GRADLE "$app_gradle" \
    'targetSdk = 36' 'targetSdk = 35'
expect_rejected \
    'NDK digest drift' OPENPENCIL_ANDROID_SDK_INSTALLER "$sdk_installer" \
    'dfb20d396df28ca02a8c708314b814a4d961dc9074f9a161932746f815aa552f' \
    '0000000000000000000000000000000000000000000000000000000000000000'
expect_rejected \
    'Gradle dependency verification disabled' \
    OPENPENCIL_ANDROID_VERIFICATION_METADATA "$verification" \
    '<verify-metadata>true</verify-metadata>' \
    '<verify-metadata>false</verify-metadata>'

# Execute the real staging step against the downloader's assignment output
# contract. No network, SDK, archive, or signing credential is needed here.
stage_fixture=$temporary/staging
mkdir -p "$stage_fixture/tools" "$stage_fixture/runner temp"
ruby - "$workflow" "$stage_fixture/stage.sh" <<'RUBY'
require "yaml"
document = YAML.safe_load(File.read(ARGV.fetch(0)), aliases: true)
step = document.fetch("jobs").fetch("build").fetch("steps").find do |item|
  item["name"] == "Stage reviewed Android Skia and bundletool inputs"
end
File.write(ARGV.fetch(1), step.fetch("run"))
RUBY
cat > "$stage_fixture/tools/pinned-release-tools.sh" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
case $1 in
    skia)
        [[ $# -eq 4 && $2 == android && -z ${GITHUB_ENV:-} ]]
        url="file://$4/skia-binaries-{key}.tar.gz"
        case ${SKIA_CASE:-valid} in
            empty) ;;
            bare-url) printf '%s\n' "$url" ;;
            relative) printf 'SKIA_BINARIES_URL=file://relative/archive\n' ;;
            empty-path) printf 'SKIA_BINARIES_URL=file:///\n' ;;
            wrong-name) printf 'OTHER_URL=%s\n' "$url" ;;
            duplicate) printf 'SKIA_BINARIES_URL=%s\nSKIA_BINARIES_URL=%s\n' "$url" "$url" ;;
            carriage-return) printf 'SKIA_BINARIES_URL=%s\r\n' "$url" ;;
            failed-download) printf 'SKIA_BINARIES_URL=%s\n' "$url"; exit 19 ;;
            failed-second-target)
                printf 'SKIA_BINARIES_URL=%s\n' "$url"
                [[ $3 != x86_64-linux-android ]] || exit 19
                ;;
            valid) printf 'SKIA_BINARIES_URL=%s\n' "$url" ;;
            *) exit 20 ;;
        esac
        ;;
    bundletool) printf '%s/bundletool-all-1.18.3.jar\n' "$2" ;;
    *) exit 21 ;;
esac
SH
chmod 0755 "$stage_fixture/tools/pinned-release-tools.sh"
stage_env=$stage_fixture/github-env
(
    cd "$stage_fixture"
    GITHUB_ENV="$stage_env" RUNNER_TEMP="$stage_fixture/runner temp" \
        SKIA_CASE=valid bash stage.sh
)
{
    printf 'ANDROID_SKIA_AARCH64_BINARIES_URL=file://%s/runner temp/skia-aarch64-linux-android/skia-binaries-{key}.tar.gz\n' "$stage_fixture"
    printf 'ANDROID_SKIA_X86_64_BINARIES_URL=file://%s/runner temp/skia-x86_64-linux-android/skia-binaries-{key}.tar.gz\n' "$stage_fixture"
    printf 'BUNDLETOOL_JAR=%s/runner temp/bundletool-1.18.3/bundletool-all-1.18.3.jar\n' "$stage_fixture"
} > "$stage_fixture/expected-env"
cmp "$stage_fixture/expected-env" "$stage_env" || {
    printf 'error: Android staging did not export bare per-ABI Skia URLs\n' >&2
    exit 1
}
staging_rejections=0
for scenario in empty bare-url relative empty-path wrong-name duplicate \
    carriage-return failed-download failed-second-target; do
    rm -f "$stage_env"
    if (
        cd "$stage_fixture"
        GITHUB_ENV="$stage_env" RUNNER_TEMP="$stage_fixture/runner temp" \
            SKIA_CASE="$scenario" bash stage.sh
    ) > "$stage_fixture/$scenario.log" 2>&1; then
        printf 'error: Android staging accepted invalid downloader output: %s\n' "$scenario" >&2
        exit 1
    fi
    [[ ! -s "$stage_env" ]] || {
        printf 'error: failed Android staging exported a partial handoff: %s\n' "$scenario" >&2
        exit 1
    }
    staging_rejections=$((staging_rejections + 1))
done
printf 'check-android-release-workflow.test.sh: staging round-trip and %s malformed/failing outputs passed.\n' \
    "$staging_rejections"

printf 'check-android-release-workflow.test.sh: %s fail-closed mutations passed.\n' \
    "$mutation_index"
