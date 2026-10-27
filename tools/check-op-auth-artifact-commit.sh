#!/usr/bin/env bash
# Validate a private-production promotion commit: one parent, the complete auth
# matrix, and the source-owned policy that adopts exactly that matrix.

set -euo pipefail

script_dir=$(CDPATH='' cd "$(dirname "$0")" && pwd)
repo_root=$(CDPATH='' cd "$script_dir/.." && pwd)
artifact_commit=${OP_AUTH_ARTIFACT_COMMIT:-}
selected_commit=${OP_AUTH_ARTIFACT_SELECTED_COMMIT:-}
source_ref=${OP_AUTH_ARTIFACT_REF:-}
output_file=${OP_AUTH_ARTIFACT_OUTPUT:-}

if [[ "$#" -ne 0 ]]; then
    printf 'usage: OP_AUTH_ARTIFACT_COMMIT=<sha> %s\n' "$0" >&2
    exit 2
fi
for value in "$artifact_commit" "$selected_commit"; do
    [[ "$value" =~ ^[0-9a-f]{40}$ ]] || {
        printf 'error: auth artifact selection must use full lowercase commit SHAs\n' >&2
        exit 2
    }
done
[[ "$selected_commit" == "$artifact_commit" ]] || {
    printf 'error: run the release from the exact selected auth artifact commit\n' >&2
    exit 2
}
[[ "$(git --no-replace-objects -C "$repo_root" rev-parse HEAD)" == "$artifact_commit" ]] || {
    printf 'error: checked-out source is not the selected auth artifact commit\n' >&2
    exit 1
}

ref_version=
if [[ "$source_ref" == refs/heads/main ]]; then
    :
elif [[ "$source_ref" \
    =~ ^refs/heads/v([0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?)$ ]]; then
    ref_version=${BASH_REMATCH[1]}
elif [[ "$source_ref" \
    =~ ^refs/tags/v([0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?)$ ]]; then
    ref_version=${BASH_REMATCH[1]}
else
    printf '%s\n' \
        'error: production releases require main, an exact version branch, or an exact version tag' \
        >&2
    exit 2
fi

read -r commit parent extra < <(
    git --no-replace-objects -C "$repo_root" rev-list --parents -n 1 "$artifact_commit"
)
[[ "$commit" == "$artifact_commit" && "$parent" =~ ^[0-9a-f]{40}$ \
    && -z "${extra:-}" ]] || {
    printf 'error: the auth artifact commit must have exactly one parent\n' >&2
    exit 1
}

temp_root=${RUNNER_TEMP:-${TMPDIR:-/tmp}}
allowed_changes=$(mktemp "$temp_root/op-auth-allowed.XXXXXX")
actual_changes=$(mktemp "$temp_root/op-auth-actual.XXXXXX")
cleanup() {
    rm -f "$allowed_changes" "$actual_changes"
}
trap cleanup EXIT

{
    printf '%s\n' \
        crates/op-auth-bridge/AUTH-RELEASE-POLICY \
        crates/op-auth-bridge/prebuilt/RELEASE-MANIFEST \
        crates/op-auth-bridge/prebuilt/RELEASE-MANIFEST.sig
    for target in \
        aarch64-apple-darwin \
        aarch64-apple-ios \
        aarch64-apple-ios-sim \
        aarch64-linux-android \
        aarch64-pc-windows-msvc \
        aarch64-unknown-linux-gnu \
        x86_64-apple-darwin \
        x86_64-linux-android \
        x86_64-pc-windows-msvc \
        x86_64-unknown-linux-gnu; do
        artifact=libop_auth.a
        [[ "$target" == *-pc-windows-msvc ]] && artifact=op_auth.lib
        for file in \
            ABI_VERSION HARDENING-ATTESTATION PROVENANCE PROVENANCE.sig \
            SHA256 VERSION "$artifact"; do
            printf 'crates/op-auth-bridge/prebuilt/%s/%s\n' "$target" "$file"
        done
    done
} | LC_ALL=C sort > "$allowed_changes"
git --no-replace-objects -C "$repo_root" diff-tree --no-commit-id --name-only --no-renames -r \
    "$parent" "$artifact_commit" | LC_ALL=C sort > "$actual_changes"
unexpected=$(LC_ALL=C comm -23 "$actual_changes" "$allowed_changes")
[[ -z "$unexpected" ]] || {
    printf 'error: artifact commit changed a path outside the auth matrix allowlist\n' >&2
    printf '%s\n' "$unexpected" >&2
    exit 1
}
for required_change in \
    crates/op-auth-bridge/AUTH-RELEASE-POLICY \
    crates/op-auth-bridge/prebuilt/RELEASE-MANIFEST \
    crates/op-auth-bridge/prebuilt/RELEASE-MANIFEST.sig; do
    grep -Fxq "$required_change" "$actual_changes" || {
        printf 'error: artifact commit did not update %s\n' "$required_change" >&2
        exit 1
    }
done
policy_change=$(git --no-replace-objects -C "$repo_root" diff-tree --no-commit-id --name-status \
    --no-renames -r "$parent" "$artifact_commit" -- \
    crates/op-auth-bridge/AUTH-RELEASE-POLICY)
[[ "$policy_change" == $'A\tcrates/op-auth-bridge/AUTH-RELEASE-POLICY' \
    || "$policy_change" == $'M\tcrates/op-auth-bridge/AUTH-RELEASE-POLICY' ]] || {
    printf 'error: artifact commit must add or modify the auth release adoption policy\n' >&2
    exit 1
}
[[ -f "$repo_root/crates/op-auth-bridge/AUTH-RELEASE-POLICY" \
    && ! -L "$repo_root/crates/op-auth-bridge/AUTH-RELEASE-POLICY" ]] || {
    printf 'error: auth release adoption policy is missing or symlinked\n' >&2
    exit 1
}

version=$("$repo_root/scripts/workspace-version.sh")
if [[ -n "$ref_version" && "$ref_version" != "$version" ]]; then
    printf 'error: branch or tag version does not match the Cargo workspace\n' >&2
    exit 1
fi
# Ordinary promotions still require a changed iOS archive. The only exception
# is a complete, newly signed local-full ABI 4 metadata rebind of the exact
# archives already authenticated by the parent's signed matrix and policy.
if ! grep -Fxq crates/op-auth-bridge/prebuilt/aarch64-apple-ios/libop_auth.a \
    "$actual_changes"; then
    python3 -I - "$repo_root" "$parent" "$artifact_commit" "$version" <<'PY'
import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import tempfile

repo, parent, current, version = sys.argv[1:]
helper = Path(repo) / "tools/check-local-full-auth.py"
prefix = "crates/op-auth-bridge/prebuilt/"
policy_path = "crates/op-auth-bridge/AUTH-RELEASE-POLICY"


def git(*args):
    return subprocess.check_output(
        ["git", "--no-replace-objects", "-C", repo, *args],
        env={**os.environ, "GIT_NO_LAZY_FETCH": "1", "GIT_TERMINAL_PROMPT": "0"},
    )


helper_source = git("show", current + ":tools/check-local-full-auth.py")
if helper.read_bytes() != helper_source:
    raise SystemExit("error: local-full verifier differs from committed source")
spec = importlib.util.spec_from_file_location("local_full_rebind", helper)
contract = importlib.util.module_from_spec(spec)
# Execute committed source bytes, never working-file or bytecode substitutions.
exec(compile(helper_source, str(helper), "exec"), contract.__dict__)
snapshot_paths = {policy_path} | {prefix + name for name in
                                ("RELEASE-MANIFEST", "RELEASE-MANIFEST.sig", "PROVENANCE_PUBKEY", "README.md")}
for target in contract.TARGETS:
    snapshot_paths.update(prefix + target + "/" + name for name in
                          (contract.artifact_name(target), "ABI_VERSION", "VERSION", "SHA256",
                           "HARDENING-ATTESTATION", "PROVENANCE", "PROVENANCE.sig"))


def snapshot(revision, destination):
    destination.mkdir()
    entries = {}
    tree = git("ls-tree", "-r", "-z", revision, "--", prefix, policy_path)
    for record in tree.split(b"\0"):
        if not record:
            continue
        metadata, raw_path = record.split(b"\t", 1)
        mode, kind, oid = metadata.split(b" ")
        path = raw_path.decode()
        contract.require(path in snapshot_paths, "Rebind snapshot contains an unallowlisted Git path")
        contract.require(kind == b"blob" and mode in (b"100644", b"100755"),
                         "Rebind inputs must be regular Git blobs")
        entries[path] = (mode, oid)
        relative = "policy" if path == policy_path else "matrix/" + path.removeprefix(prefix)
        output = destination / relative
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_bytes(git("cat-file", "blob", oid.decode()))
    return entries


try:
    header = git("cat-file", "commit", current).split(b"\n\n", 1)[0]
    parents = [line[7:].decode() for line in header.splitlines() if line.startswith(b"parent ")]
    contract.require(parents == [parent], "Rebind immediate parent was substituted")
    expected_changes = {policy_path, prefix + "RELEASE-MANIFEST", prefix + "RELEASE-MANIFEST.sig"}
    for target in contract.TARGETS:
        expected_changes.update(prefix + target + "/" + name for name in
                                ("HARDENING-ATTESTATION", "PROVENANCE", "PROVENANCE.sig"))
    records = git("diff-tree", "--no-commit-id", "--name-status", "--no-renames", "-r", "-z",
                  parent, current).split(b"\0")
    contract.require(records[-1:] == [b""], "Malformed metadata rebind diff")
    changes = records[:-1]
    contract.require(len(changes) == 2 * len(expected_changes), "Rebind requires all ten metadata updates only")
    actual = {changes[i + 1].decode(): changes[i] for i in range(0, len(changes), 2)}
    contract.require(set(actual) == expected_changes and set(actual.values()) == {b"M"},
                     "Rebind must modify exactly the canonical complete metadata set")
    with tempfile.TemporaryDirectory(prefix="op-auth-rebind-") as temporary:
        temporary = Path(temporary)
        old, new = temporary / "parent", temporary / "current"
        before, after = snapshot(parent, old), snapshot(current, new)
        for target in contract.TARGETS:
            for name in (contract.artifact_name(target), "ABI_VERSION", "VERSION", "SHA256"):
                path = prefix + target + "/" + name
                contract.require(before.get(path) is not None and before[path] == after.get(path),
                                 "Rebind changed an archive or stable target field")
        old_result = contract.verify(old / "matrix", old / "matrix/PROVENANCE_PUBKEY", policy=old / "policy")
        new_result = contract.verify(new / "matrix", new / "matrix/PROVENANCE_PUBKEY",
                                     {"openpencil_revision": parent, "version": version}, policy=new / "policy")
        for field in ("source_revision", "version", "abi", "signer_revision"):
            contract.require(old_result[field] == new_result[field], "Rebind changed the private source, signer, version, or ABI")
        for field in ("build_id", "execution_evidence_sha256"):
            contract.require(old_result[field] != new_result[field], "Rebind reused an old build or execution receipt")
        for target in contract.TARGETS:
            fields = []
            for root in (old, new):
                data = contract.read(root / "matrix" / target / "HARDENING-ATTESTATION")
                fields.append(contract.canonical_fields(data, contract.RAW_KEYS + contract.LOCAL_KEYS, target))
            contract.require(all(fields[0][key] == fields[1][key] for key in contract.RAW_KEYS),
                             "Metadata rebind changed the original build attestation")
except (contract.Invalid, OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
    print("error: local-full metadata rebind rejected: " + str(error), file=sys.stderr)
    raise SystemExit(1)
PY
fi
OP_AUTH_PREBUILT_ROOT=$repo_root/crates/op-auth-bridge/prebuilt \
OP_AUTH_RELEASE_EXPECTED_OPENPENCIL_REVISION=$parent \
OP_AUTH_RELEASE_WORKSPACE_VERSION=$version \
    "$repo_root/tools/check-op-auth-release-matrix.sh"
# Strict promotion verifies a newly produced matrix before adoption. The same
# checkout must then pass consumer mode, proving the committed policy precisely
# adopts the key, manifest digest, private source, build id, ABI, and hardening.
OP_AUTH_PREBUILT_ROOT=$repo_root/crates/op-auth-bridge/prebuilt \
    "$repo_root/tools/check-op-auth-release-matrix.sh"
OP_AUTH_PREBUILT_ROOT=$repo_root/crates/op-auth-bridge/prebuilt \
    "$repo_root/tools/check-op-auth-prebuilt.sh" --require-hardened

if [[ -n "$output_file" ]]; then
    case "$output_file" in
        /*) ;;
        *)
            printf 'error: OP_AUTH_ARTIFACT_OUTPUT must be an absolute path\n' >&2
            exit 2
            ;;
    esac
    [[ -f "$output_file" && ! -L "$output_file" ]] || {
        printf 'error: OP_AUTH_ARTIFACT_OUTPUT must be a regular existing file\n' >&2
        exit 2
    }
    {
        printf 'artifact_sha=%s\n' "$artifact_commit"
        printf 'base_sha=%s\n' "$parent"
        printf 'version=%s\n' "$version"
    } >> "$output_file"
fi

printf 'check-op-auth-artifact-commit.sh: verified %s as auth-adoption child of %s.\n' \
    "$artifact_commit" "$parent"
