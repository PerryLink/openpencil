#!/usr/bin/env bash
# Secret-free Git fixture tests for an exact signed-matrix adoption transition.
# shellcheck disable=SC2016 # Generated fixture scripts must retain variables.

set -euo pipefail

script_dir=$(CDPATH='' cd "$(dirname "$0")" && pwd)
temp_root=$(mktemp -d "${TMPDIR:-/tmp}/op-auth-commit-test.XXXXXX")
fixture=$temp_root/repo

cleanup() {
    rm -rf "$temp_root"
}
trap cleanup EXIT

mkdir -p \
    "$fixture/tools" "$fixture/scripts" \
    "$fixture/crates/op-auth-bridge/prebuilt/aarch64-apple-ios"
cp "$script_dir/check-op-auth-artifact-commit.sh" "$fixture/tools/"
grep -Fq \
    'unexpected=$(LC_ALL=C comm -23 "$actual_changes" "$allowed_changes")' \
    "$fixture/tools/check-op-auth-artifact-commit.sh"
printf '#!/usr/bin/env bash\nprintf "1.2.3\\n"\n' \
    > "$fixture/scripts/workspace-version.sh"
printf '%s\n' \
    '#!/usr/bin/env bash' \
    'set -euo pipefail' \
    '[[ "$OP_AUTH_PREBUILT_ROOT" == "$PWD/crates/op-auth-bridge/prebuilt" ]]' \
    'if [[ -n "${OP_AUTH_RELEASE_WORKSPACE_VERSION:-}" || -n "${OP_AUTH_RELEASE_EXPECTED_OPENPENCIL_REVISION:-}" ]]; then' \
    '    [[ "$OP_AUTH_RELEASE_EXPECTED_OPENPENCIL_REVISION" == "$EXPECTED_FIXTURE_PARENT" ]]' \
    '    [[ "$OP_AUTH_RELEASE_WORKSPACE_VERSION" == 1.2.3 ]]' \
    '    printf "strict\n" >> "$MATRIX_CHECK_LOG"' \
    'else' \
    '    printf "consumer\n" >> "$MATRIX_CHECK_LOG"' \
    'fi' \
    > "$fixture/tools/check-op-auth-release-matrix.sh"
printf '%s\n' \
    '#!/usr/bin/env bash' \
    'set -euo pipefail' \
    '[[ "$1" == --require-hardened ]]' \
    '[[ "$OP_AUTH_PREBUILT_ROOT" == "$PWD/crates/op-auth-bridge/prebuilt" ]]' \
    > "$fixture/tools/check-op-auth-prebuilt.sh"
chmod +x "$fixture/tools/"*.sh "$fixture/scripts/workspace-version.sh"

git -C "$fixture" init -q
git -C "$fixture" config user.name 'OpenPencil fixture'
git -C "$fixture" config user.email 'fixture@openpencil.invalid'
printf 'trusted-key\n' \
    > "$fixture/crates/op-auth-bridge/prebuilt/PROVENANCE_PUBKEY"
printf 'old source adoption policy\n' \
    > "$fixture/crates/op-auth-bridge/AUTH-RELEASE-POLICY"
git -C "$fixture" add .
git -C "$fixture" commit -q -m 'fixture source S'
source_sha=$(git -C "$fixture" rev-parse HEAD)

printf 'manifest\n' \
    > "$fixture/crates/op-auth-bridge/prebuilt/RELEASE-MANIFEST"
printf 'signature\n' \
    > "$fixture/crates/op-auth-bridge/prebuilt/RELEASE-MANIFEST.sig"
printf 'ios archive\n' \
    > "$fixture/crates/op-auth-bridge/prebuilt/aarch64-apple-ios/libop_auth.a"
printf 'new matrix adoption policy\n' \
    > "$fixture/crates/op-auth-bridge/AUTH-RELEASE-POLICY"
git -C "$fixture" add \
    crates/op-auth-bridge/prebuilt \
    crates/op-auth-bridge/AUTH-RELEASE-POLICY
git -C "$fixture" commit -q -m 'fixture auth-only A'
artifact_sha=$(git -C "$fixture" rev-parse HEAD)

output=$temp_root/output
matrix_log=$temp_root/matrix-log
: > "$output"
: > "$matrix_log"
(
    cd "$fixture"
    LC_ALL=C \
    EXPECTED_FIXTURE_PARENT=$source_sha \
    MATRIX_CHECK_LOG=$matrix_log \
    OP_AUTH_ARTIFACT_COMMIT=$artifact_sha \
    OP_AUTH_ARTIFACT_SELECTED_COMMIT=$artifact_sha \
    OP_AUTH_ARTIFACT_REF=refs/tags/v1.2.3 \
    OP_AUTH_ARTIFACT_OUTPUT=$output \
        tools/check-op-auth-artifact-commit.sh >/dev/null
)
grep -Fxq "artifact_sha=$artifact_sha" "$output"
grep -Fxq "base_sha=$source_sha" "$output"
grep -Fxq 'version=1.2.3' "$output"
[[ "$(cat "$matrix_log")" == $'strict\nconsumer' ]]

# A valid iOS archive used to be reported as unexpected when the caller's
# collation differed from the C locale used to sort the comm inputs.
non_c_locale=
for candidate in \
    en_US.UTF-8 en_US.utf8 \
    de_DE.UTF-8 de_DE.utf8 \
    fr_FR.UTF-8 fr_FR.utf8; do
    if LC_ALL="$candidate" locale charmap >/dev/null 2>&1; then
        non_c_locale=$candidate
        break
    fi
done
if [[ -n "$non_c_locale" ]]; then
    locale_output=$temp_root/locale-output
    locale_matrix_log=$temp_root/locale-matrix-log
    : > "$locale_output"
    : > "$locale_matrix_log"
    (
        cd "$fixture"
        LC_ALL="$non_c_locale" \
        EXPECTED_FIXTURE_PARENT=$source_sha \
        MATRIX_CHECK_LOG=$locale_matrix_log \
        OP_AUTH_ARTIFACT_COMMIT=$artifact_sha \
        OP_AUTH_ARTIFACT_SELECTED_COMMIT=$artifact_sha \
        OP_AUTH_ARTIFACT_REF=refs/tags/v1.2.3 \
        OP_AUTH_ARTIFACT_OUTPUT=$locale_output \
            tools/check-op-auth-artifact-commit.sh >/dev/null
    )
    grep -Fxq "artifact_sha=$artifact_sha" "$locale_output"
    grep -Fxq "base_sha=$source_sha" "$locale_output"
    grep -Fxq 'version=1.2.3' "$locale_output"
    [[ "$(cat "$locale_matrix_log")" == $'strict\nconsumer' ]]
fi

# A complete matrix replacement without the source-owned policy update must
# not be promotable, even though every historical matrix path changed.
git -C "$fixture" checkout -q --detach "$source_sha"
mkdir -p "$fixture/crates/op-auth-bridge/prebuilt/aarch64-apple-ios"
printf 'manifest without adoption\n' \
    > "$fixture/crates/op-auth-bridge/prebuilt/RELEASE-MANIFEST"
printf 'signature without adoption\n' \
    > "$fixture/crates/op-auth-bridge/prebuilt/RELEASE-MANIFEST.sig"
printf 'archive without adoption\n' \
    > "$fixture/crates/op-auth-bridge/prebuilt/aarch64-apple-ios/libop_auth.a"
git -C "$fixture" add crates/op-auth-bridge/prebuilt
git -C "$fixture" commit -q -m 'fixture matrix without adoption policy'
missing_policy_sha=$(git -C "$fixture" rev-parse HEAD)
if (
    cd "$fixture"
    EXPECTED_FIXTURE_PARENT=$source_sha \
    OP_AUTH_ARTIFACT_COMMIT=$missing_policy_sha \
    OP_AUTH_ARTIFACT_SELECTED_COMMIT=$missing_policy_sha \
    OP_AUTH_ARTIFACT_REF=refs/tags/v1.2.3 \
        tools/check-op-auth-artifact-commit.sh >/dev/null 2>&1
); then
    printf 'error: complete matrix without adoption policy was accepted\n' >&2
    exit 1
fi
git -C "$fixture" checkout -q --detach "$artifact_sha"

if (
    cd "$fixture"
    EXPECTED_FIXTURE_PARENT=$source_sha \
    OP_AUTH_ARTIFACT_COMMIT=$artifact_sha \
    OP_AUTH_ARTIFACT_SELECTED_COMMIT=3333333333333333333333333333333333333333 \
    OP_AUTH_ARTIFACT_REF=refs/tags/v1.2.3 \
        tools/check-op-auth-artifact-commit.sh >/dev/null 2>&1
); then
    printf 'error: mismatched selected artifact commit was accepted\n' >&2
    exit 1
fi

if (
    cd "$fixture"
    EXPECTED_FIXTURE_PARENT=$source_sha \
    OP_AUTH_ARTIFACT_COMMIT=$artifact_sha \
    OP_AUTH_ARTIFACT_SELECTED_COMMIT=$artifact_sha \
    OP_AUTH_ARTIFACT_REF=refs/tags/v1.2.4 \
        tools/check-op-auth-artifact-commit.sh >/dev/null 2>&1
); then
    printf 'error: mismatched release ref version was accepted\n' >&2
    exit 1
fi

printf 'missing required archive change\n' \
    > "$fixture/crates/op-auth-bridge/prebuilt/RELEASE-MANIFEST"
git -C "$fixture" add crates/op-auth-bridge/prebuilt/RELEASE-MANIFEST
git -C "$fixture" commit -q -m 'fixture incomplete auth transition'
incomplete_sha=$(git -C "$fixture" rev-parse HEAD)
if (
    cd "$fixture"
    EXPECTED_FIXTURE_PARENT=$artifact_sha \
    OP_AUTH_ARTIFACT_COMMIT=$incomplete_sha \
    OP_AUTH_ARTIFACT_SELECTED_COMMIT=$incomplete_sha \
    OP_AUTH_ARTIFACT_REF=refs/heads/main \
        tools/check-op-auth-artifact-commit.sh >/dev/null 2>&1
); then
    printf 'error: artifact commit missing required changes was accepted\n' >&2
    exit 1
fi

printf 'changed manifest\n' \
    > "$fixture/crates/op-auth-bridge/prebuilt/RELEASE-MANIFEST"
printf 'changed signature\n' \
    > "$fixture/crates/op-auth-bridge/prebuilt/RELEASE-MANIFEST.sig"
printf 'changed iOS archive\n' \
    > "$fixture/crates/op-auth-bridge/prebuilt/aarch64-apple-ios/libop_auth.a"
printf 'forged key\n' \
    > "$fixture/crates/op-auth-bridge/prebuilt/PROVENANCE_PUBKEY"
git -C "$fixture" add .
git -C "$fixture" commit -q -m 'fixture forged-key transition'
forbidden_sha=$(git -C "$fixture" rev-parse HEAD)
if (
    cd "$fixture"
    EXPECTED_FIXTURE_PARENT=$incomplete_sha \
    OP_AUTH_ARTIFACT_COMMIT=$forbidden_sha \
    OP_AUTH_ARTIFACT_SELECTED_COMMIT=$forbidden_sha \
    OP_AUTH_ARTIFACT_REF=refs/heads/main \
        tools/check-op-auth-artifact-commit.sh >/dev/null 2>&1
); then
    printf 'error: artifact commit replacing the trusted key was accepted\n' >&2
    exit 1
fi

# Exercise the exception with real Ed25519 signatures and the production
# semantic verifier, changing its fixed root only inside this temporary fixture.
python3 -I - "$script_dir" "$temp_root" <<'PY'
import hashlib
import importlib.util
import os
from pathlib import Path
import shutil
import subprocess
import sys

source, temporary = map(Path, sys.argv[1:])
repo = temporary / "rebind-repo"
repo.mkdir()
key = temporary / "ephemeral-rebind-key.pem"


def run(args, **kwargs):
    return subprocess.check_output(args, stderr=subprocess.PIPE, **kwargs)


def git(*args):
    return run(["git", "-C", str(repo), *args]).decode().strip()


def write(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(value if isinstance(value, bytes) else value.encode())


def sha(value):
    return hashlib.sha256(value).hexdigest()


def commit(message):
    git("add", ".")
    git("commit", "-q", "-m", message)
    return git("rev-parse", "HEAD")


run(["openssl", "genpkey", "-algorithm", "ED25519", "-out", str(key)])
pin = run(["openssl", "pkey", "-in", str(key), "-pubout", "-outform", "DER"])[-32:].hex()
helper_source = (source / "check-local-full-auth.py").read_text()
helper_source = helper_source.replace('PIN = "07e073b30d6de0df7343f8a11c4dccaf69aa87518d94917375eda6e63cc506e1"',
                                      'PIN = "' + pin + '"')
write(repo / "tools/check-local-full-auth.py", helper_source)
spec = importlib.util.spec_from_file_location("rebind_contract", repo / "tools/check-local-full-auth.py")
c = importlib.util.module_from_spec(spec)
exec(compile(helper_source, spec.origin, "exec"), c.__dict__)
shutil.copy2(source / "check-op-auth-artifact-commit.sh", repo / "tools/check-op-auth-artifact-commit.sh")
write(repo / "scripts/workspace-version.sh", '#!/usr/bin/env bash\nprintf "0.8.5\\n"\n')
# The exception itself invokes the real verifier twice. The existing shell
# dispatch is also retained and must still request strict + consumer validation.
write(repo / "tools/check-op-auth-release-matrix.sh", '''#!/usr/bin/env bash
set -euo pipefail
if [[ -n "${OP_AUTH_RELEASE_EXPECTED_OPENPENCIL_REVISION:-}" ]]; then
    [[ "$OP_AUTH_RELEASE_EXPECTED_OPENPENCIL_REVISION" == "$EXPECTED_FIXTURE_PARENT" ]]
    [[ "$OP_AUTH_RELEASE_WORKSPACE_VERSION" == 0.8.5 ]]
    printf 'strict\n' >> "$MATRIX_CHECK_LOG"
else
    printf 'consumer\n' >> "$MATRIX_CHECK_LOG"
fi
''')
write(repo / "tools/check-op-auth-prebuilt.sh", '#!/usr/bin/env bash\n[[ "$1" == --require-hardened ]]\n')
for path in [*repo.joinpath("tools").glob("*.sh"), repo / "scripts/workspace-version.sh"]:
    path.chmod(0o755)
git("init", "-q")
git("config", "user.name", "OpenPencil fixture")
git("config", "user.email", "fixture@openpencil.invalid")
root = repo / "crates/op-auth-bridge/prebuilt"
policy = repo / "crates/op-auth-bridge/AUTH-RELEASE-POLICY"
write(root / "PROVENANCE_PUBKEY", pin + "\n")
p, q, version = "1" * 40, "3" * 40, "0.8.5"


def sign(path):
    output = temporary / "ephemeral-signature.bin"
    run(["openssl", "pkeyutl", "-sign", "-rawin", "-inkey", str(key), "-in", str(path), "-out", str(output)])
    write(path.with_name(path.name + ".sig"), output.read_bytes().hex() + "\n")


def raw(target, payload, private):
    prefix = "_" if "-apple-" in target else ""
    abi = sha("".join(prefix + symbol + "\n" for symbol in c.SYMBOLS).encode())
    protector = sha(("protector:" + target).encode())
    binding = sha((f"op-auth-protector-binding-v1\ntarget={target}\nreview_id=source-{private}\nprotector_sha256={protector}\n").encode())
    mobile = c.platform(target) in ("android", "ios", "ios-simulator")
    cross = mobile or target in c.WINDOWS or target == "x86_64-apple-darwin"
    minimum = "21" if c.platform(target) == "android" else "15.0" if mobile else "native"
    values = ["3", "production", "4", target, c.artifact_name(target), sha(payload), str(len(payload)),
              private, "clean", "1792991219", "private-ci-fat-lto", "remapped", "none", "stripped",
              "eliminated", "op-auth-hardened-v3", abi, sha(b"coff-globals") if target in c.WINDOWS else abi,
              sha(("sections:" + target).encode()), "1", "0", "cross-final-link" if cross else "native-link-run",
              "not-applicable" if mobile else "passed", c.platform(target), minimum, sha(("binary:" + target).encode()),
              "source-" + private, protector, binding, "none", "none", "none", "none", "none", "none", "none",
              "rustc 1.95.0 (synthetic 2026-01-01);host: aarch64-apple-darwin;"]
    return dict(zip(c.RAW_KEYS, values))


def matrix(public, build, evidence, *, private=p, signer=q, mutation=None, payload_change=None):
    rows = ["format=op-auth-release-matrix-v1", "version=" + version, "abi=4", "source_revision=" + private,
            "openpencil_revision=" + public, "build_id=" + build, "target_count=10"]
    for target in c.TARGETS:
        entry = root / target
        payload = ("archive:" + target).encode()
        if payload_change == target:
            payload += b"changed"
        artifact = c.artifact_name(target)
        write(entry / artifact, payload)
        fields = raw(target, payload, private)
        fields.update(dict(zip(c.LOCAL_KEYS, ["local-full-v1", build, public, signer, evidence])))
        if mutation:
            mutation(target, fields)
        write(entry / "HARDENING-ATTESTATION", "".join(k + "=" + v + "\n" for k, v in fields.items()))
        ah, hh = sha(payload), sha((entry / "HARDENING-ATTESTATION").read_bytes())
        write(entry / "ABI_VERSION", "4\n")
        write(entry / "VERSION", version + "\n")
        write(entry / "SHA256", ah + "\n")
        profile = "op-auth-signed-unobfuscated-v1" if target in c.WINDOWS else "op-auth-hardened-v1"
        write(entry / "PROVENANCE", f"format=1\ntarget={target}\nartifact={artifact}\nversion={version}\nabi=4\nsha256={ah}\nhardening={profile}\nsource_revision={private}\nbuild_id={build}.a4.{hh}\n")
        sign(entry / "PROVENANCE")
        rows.append(f"target={target}|artifact={artifact}|sha256={ah}|provenance_sha256={sha((entry/'PROVENANCE').read_bytes())}|hardening_sha256={hh}")
    write(root / "RELEASE-MANIFEST", "\n".join(rows) + "\n")
    sign(root / "RELEASE-MANIFEST")
    write(policy, f"format=op-auth-release-policy-v1\nabi=4\npublic_key={pin}\nrelease_manifest_sha256={sha((root/'RELEASE-MANIFEST').read_bytes())}\nsource_revision={private}\nbuild_id={build}\n")


matrix("2" * 40, "local-full-before", "4" * 64)
base = commit("previous signed matrix carried by newer source B2")
log = temporary / "rebind-matrix-log"
checks = []


def check(name, parent, *, passed=False, message=None, replacement_parent=None, dirty_helper=False):
    head = commit(name)
    if dirty_helper:
        write(repo / "tools/check-local-full-auth.py", "raise SystemExit('dirty helper executed')\n")
    if replacement_parent:
        original = git("--no-replace-objects", "cat-file", "commit", head)
        altered = "\n".join("parent " + replacement_parent if line.startswith("parent ") else line
                            for line in original.splitlines()) + "\n"
        replacement = run(["git", "-C", str(repo), "hash-object", "-t", "commit", "-w", "--stdin"],
                          input=altered.encode()).decode().strip()
        git("replace", head, replacement)
        assert git("rev-parse", head + "^") == replacement_parent
        assert git("--no-replace-objects", "rev-parse", head + "^") != replacement_parent
    write(log, "")
    environment = {**os.environ, "EXPECTED_FIXTURE_PARENT": parent, "MATRIX_CHECK_LOG": str(log),
                   "OP_AUTH_ARTIFACT_COMMIT": head, "OP_AUTH_ARTIFACT_SELECTED_COMMIT": head,
                   "OP_AUTH_ARTIFACT_REF": "refs/tags/v0.8.5"}
    for name_to_remove in ("OP_AUTH_ARTIFACT_OUTPUT", "OP_AUTH_RELEASE_EXPECTED_OPENPENCIL_REVISION", "OP_AUTH_RELEASE_WORKSPACE_VERSION"):
        environment.pop(name_to_remove, None)
    try:
        result = subprocess.run(["bash", "tools/check-op-auth-artifact-commit.sh"], cwd=repo,
                                env=environment, capture_output=True, text=True)
    finally:
        if replacement_parent:
            git("replace", "-d", head)
        if dirty_helper:
            git("checkout", "--", "tools/check-local-full-auth.py")
    assert (result.returncode == 0) == passed, (name, result.stdout, result.stderr)
    if passed:
        assert log.read_text() == "strict\nconsumer\n"
    elif message:
        assert message in result.stderr, (name, result.stderr)
    checks.append(name)


def reset():
    git("checkout", "-q", "--detach", base)


matrix(base, "local-full-after", "5" * 64)
check("valid-same-archives-rebind", base, passed=True)
reset()
matrix(base, "local-full-after", "5" * 64, private="6" * 40)
check("changed-private-source", base, message="private source, signer, version, or ABI")
reset()
matrix(base, "local-full-after", "5" * 64, signer="6" * 40)
check("changed-signer", base, message="private source, signer, version, or ABI")
reset()
matrix(base, "local-full-before", "5" * 64)
check("reused-build-id", base, message="reused an old build")
reset()
matrix(base, "local-full-after", "4" * 64)
check("reused-execution-receipt", base, message="reused an old build")
reset()
matrix("7" * 40, "local-full-after", "5" * 64)
check("wrong-immediate-parent", base, message="openpencil_revision mismatch")
reset()
git("commit", "--allow-empty", "-q", "-m", "substitute parent with identical matrix")
substituted_parent = git("rev-parse", "HEAD")
reset()
matrix(substituted_parent, "local-full-after", "5" * 64)
check("replacement-ref-parent-substitution", substituted_parent, replacement_parent=substituted_parent,
      message="openpencil_revision mismatch")
reset()
matrix(base, "local-full-after", "5" * 64)
check("dirty-verifier-substitution", base, dirty_helper=True, message="verifier differs from committed source")
for name, field, value in [("wrong-marker", "staging_format", "local-full-v2"),
                           ("wrong-abi", "abi", "3"),
                           ("mixed-target", "execution_evidence_sha256", "7" * 64),
                           ("changed-raw-attestation", "toolchain", "rustc 1.95.0 (changed)")]:
    reset()
    matrix(base, "local-full-after", "5" * 64,
           mutation=lambda target, fields: fields.update({field: value}) if target == c.TARGETS[0] else None)
    check(name, base)
reset()
matrix(base, "local-full-after", "5" * 64, payload_change=c.TARGETS[0])
check("non-ios-archive-changed", base, message="all ten metadata updates only")
reset()
matrix(base, "local-full-after", "5" * 64)
missing = root / c.TARGETS[0] / "PROVENANCE.sig"
write(missing, run(["git", "-C", str(repo), "show", base + ":" + str(missing.relative_to(repo))]))
check("missing-target-metadata-update", base, message="all ten metadata updates only")
reset()
matrix(base, "local-full-after", "5" * 64)
write(root / "RELEASE-MANIFEST.sig", "0" * 128 + "\n")
check("invalid-current-signature", base, message="Invalid signature")
for name, mutate in [
    ("invalid-parent-signature", lambda: write(root / "RELEASE-MANIFEST.sig", "0" * 128 + "\n")),
    ("invalid-parent-policy", lambda: write(policy, policy.read_bytes().replace(b"abi=4", b"abi=3"))),
    ("wrong-parent-root", lambda: write(root / "PROVENANCE_PUBKEY", "0" * 64 + "\n")),
    ("unallowlisted-parent-path", lambda: write(root / "unexpected/metadata", "unallowlisted")),
    ("legacy-parent-marker", lambda: matrix("2" * 40, "local-full-before", "4" * 64,
        mutation=lambda target, fields: fields.update(staging_format="1"))),
]:
    reset()
    mutate()
    parent = commit(name + " parent")
    matrix(parent, "local-full-after", "5" * 64)
    check(name, parent)
print("check-op-auth-artifact-commit.test.sh: " + str(len(checks)) + " real-signature rebind checks passed.")
PY

printf 'check-op-auth-artifact-commit.test.sh: promotion adoption gates passed.\n'
