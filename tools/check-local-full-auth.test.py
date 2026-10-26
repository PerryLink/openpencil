#!/usr/bin/env python3
"""Ephemeral-root tests for complete local ABI 4 semantics; no production key."""
import hashlib
import importlib.util
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import sys

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("local_full", HERE / "check-local-full-auth.py")
m = importlib.util.module_from_spec(spec); spec.loader.exec_module(m)
P, B, Q = "1" * 40, "2" * 40, "3" * 40
BUILD, VERSION, EVIDENCE = "local-full-test", "0.8.5", "4" * 64
checks = []


def run(args): return subprocess.check_output(args, stderr=subprocess.DEVNULL)
def sha(value): return hashlib.sha256(value).hexdigest()


def raw_fields(target, payload):
    prefix = "_" if "-apple-" in target else ""
    abi = sha("".join(prefix + name + "\n" for name in m.SYMBOLS).encode())
    protector = sha(("protector:" + target).encode())
    bind = sha((f"op-auth-protector-binding-v1\ntarget={target}\nreview_id=source-{P}\nprotector_sha256={protector}\n").encode())
    mobile = m.platform(target) in ("android", "ios", "ios-simulator")
    cross = mobile or target in m.WINDOWS or target == "x86_64-apple-darwin"
    minimum = "21" if m.platform(target) == "android" else "15.0" if mobile else "native"
    values = ["3", "production", "4", target, m.artifact_name(target), sha(payload), str(len(payload)),
              P, "clean", "1792991219", "private-ci-fat-lto", "remapped", "none", "stripped",
              "eliminated", "op-auth-hardened-v3", abi, sha(b"coff-globals") if target in m.WINDOWS else abi,
              sha(("sections:" + target).encode()), "1", "0", "cross-final-link" if cross else "native-link-run",
              "not-applicable" if mobile else "passed", m.platform(target), minimum, sha(("binary:" + target).encode()),
              "source-" + P, protector, bind, "none", "none", "none", "none", "none", "none", "none",
              "rustc 1.95.0 (synthetic 2026-01-01);host: aarch64-apple-darwin;"]
    assert len(values) == len(m.RAW_KEYS)
    return dict(zip(m.RAW_KEYS, values))


def sign(payload, key):
    binary = payload.with_name(payload.name + ".bin-sig")
    run(["openssl", "pkeyutl", "-sign", "-rawin", "-inkey", str(key), "-in", str(payload), "-out", str(binary)])
    payload.with_name(payload.name + ".sig").write_text(binary.read_bytes().hex() + "\n")
    binary.unlink()


def resign(root, key):
    rows = ["format=op-auth-release-matrix-v1", "version=" + VERSION, "abi=4", "source_revision=" + P,
            "openpencil_revision=" + B, "build_id=" + BUILD, "target_count=10"]
    for target in m.TARGETS:
        d = root / target; archive = m.artifact_name(target)
        ah, hh = m.digest(d / archive), m.digest(d / "HARDENING-ATTESTATION")
        (d / "VERSION").write_text(VERSION + "\n")
        (d / "ABI_VERSION").write_text("4\n")
        (d / "SHA256").write_text(ah + "\n")
        profile = "op-auth-signed-unobfuscated-v1" if target in m.WINDOWS else "op-auth-hardened-v1"
        (d / "PROVENANCE").write_text(f"format=1\ntarget={target}\nartifact={archive}\nversion={VERSION}\nabi=4\nsha256={ah}\nhardening={profile}\nsource_revision={P}\nbuild_id={BUILD}.a4.{hh}\n")
        sign(d / "PROVENANCE", key)
        rows.append(f"target={target}|artifact={archive}|sha256={ah}|provenance_sha256={m.digest(d/'PROVENANCE')}|hardening_sha256={hh}")
    (root / "RELEASE-MANIFEST").write_text("\n".join(rows) + "\n")
    sign(root / "RELEASE-MANIFEST", key)


def change(path, key, value):
    lines = path.read_text().splitlines()
    index = next(i for i, line in enumerate(lines) if line.startswith(key + "="))
    lines[index] = key + "=" + value
    path.write_text("\n".join(lines) + "\n")


with tempfile.TemporaryDirectory(prefix="local-full-contract-tests-") as directory:
    temp = Path(directory); key = temp / "key.pem"; attacker = temp / "attacker.pem"
    run(["openssl", "genpkey", "-algorithm", "ED25519", "-out", str(key)])
    run(["openssl", "genpkey", "-algorithm", "ED25519", "-out", str(attacker)])
    der = run(["openssl", "pkey", "-in", str(key), "-pubout", "-outform", "DER"])
    m.PIN = der[-32:].hex(); trusted = temp / "PROVENANCE_PUBKEY"; trusted.write_text(m.PIN + "\n")
    valid = temp / "valid"; valid.mkdir()
    for target in m.TARGETS:
        d = valid / target; d.mkdir(); payload = ("fixture:" + target).encode()
        (d / m.artifact_name(target)).write_bytes(payload)
        fields = raw_fields(target, payload)
        fields.update(dict(zip(m.LOCAL_KEYS, ["local-full-v1", BUILD, B, Q, EVIDENCE])))
        (d / "HARDENING-ATTESTATION").write_text("".join(k + "=" + v + "\n" for k, v in fields.items()))
    resign(valid, key)
    policy = temp / "policy"
    policy.write_text(f"format=op-auth-release-policy-v1\nabi=4\npublic_key={m.PIN}\nrelease_manifest_sha256={m.digest(valid/'RELEASE-MANIFEST')}\nsource_revision={P}\nbuild_id={BUILD}\n")
    identity = {"version": VERSION, "source_revision": P, "openpencil_revision": B, "build_id": BUILD}
    result = m.verify(valid, trusted, identity, policy=policy)
    assert result["target_count"] == "10" and result["signer_revision"] == Q
    checks.append("complete10-target-positive")
    assert m.has_local_marker(valid)

    def reject(name, mutation, *, re_sign=True):
        root = temp / name; shutil.copytree(valid, root)
        mutation(root)
        if re_sign: resign(root, key)
        try: m.verify(root, trusted, identity)
        except (m.Invalid, OSError, ValueError, KeyError): checks.append(name)
        else: raise AssertionError("Accepted " + name)

    one = m.TARGETS[0]; win = m.WINDOWS[0]
    def h(root, target=one): return root / target / "HARDENING-ATTESTATION"
    for name, field, value in [
        ("abi-downgrade", "abi", "3"), ("source-substitution", "source_revision", "5" * 40),
        ("dirty-build", "source_tree_state", "dirty"), ("debug-profile", "profile", "dev"),
        ("no-hardening", "symbols", "none"), ("wrong-review", "obfuscation_review", "source-" + "5" * 40),
        ("wrong-review-binding", "review_binding_sha256", "5" * 64),
        ("wrong-allowlist", "abi_allowlist_sha256", "5" * 64),
        ("private-globals", "global_defined_symbols_sha256", "5" * 64),
        ("debug-sections", "debug_section_count", "1"),
        ("bad-staging-marker", "staging_format", "local-full-v2"),
        ("wrong-build", "release_build_id", "other"),
        ("wrong-public-source", "openpencil_revision", "5" * 40),
        ("mixed-signer", "signer_revision", "5" * 40),
        ("mixed-receipt", "execution_evidence_sha256", "5" * 64),
        ("zero-receipt", "execution_evidence_sha256", "0" * 64),
        ("malformed-receipt", "execution_evidence_sha256", "oops"),
        ("no-runtime", "link_execution", "pending"),
    ]:
        reject(name, lambda root, field=field, value=value: change(h(root), field, value))
    reject("windows-no-runtime", lambda root: change(h(root, win), "link_execution", "not-applicable"))
    reject("linux-cross-claim", lambda root: change(h(root, "aarch64-unknown-linux-gnu"), "link_validation", "cross-final-link"))
    reject("mobile-runtime-claim", lambda root: change(h(root, "aarch64-apple-ios"), "link_execution", "passed"))
    reject("mobile-sdk-downgrade", lambda root: change(h(root, "aarch64-apple-ios"), "minimum_platform_version", "14.0"))
    reject("duplicate-empty-field", lambda root: h(root).write_bytes(h(root).read_bytes() + b"abi=\n"))
    reject("mixed-legacy-staging", lambda root: h(root).write_bytes(b"\n".join(h(root).read_bytes().splitlines()[:37]) + f"\nstaging_format=1\nrelease_build_id={BUILD}\nopenpencil_revision={B}\n".encode()))
    reject("optional-directory", lambda root: (root / "README.md").mkdir(), re_sign=False)
    reject("optional-key-directory", lambda root: (root / "PROVENANCE_PUBKEY").mkdir(), re_sign=False)
    reject("missing-target", lambda root: shutil.rmtree(root / one), re_sign=False)
    reject("extra-target", lambda root: (root / "unexpected-target").mkdir(), re_sign=False)
    reject("extra-target-file", lambda root: (root / one / "extra").write_text("x"), re_sign=False)
    reject("artifact-tamper", lambda root: (root / one / m.artifact_name(one)).write_bytes(b"tamper"), re_sign=False)
    reject("manifest-order", lambda root: (root / "RELEASE-MANIFEST").write_bytes((root / "RELEASE-MANIFEST").read_bytes().replace(b"target_count=10", b"target_count=4")), re_sign=False)
    reject("signature-tamper", lambda root: (root / "RELEASE-MANIFEST.sig").write_text("0" * 128 + "\n"), re_sign=False)
    reject("wrong-root-signed", lambda root: sign(root / "RELEASE-MANIFEST", attacker), re_sign=False)
    reject("manifest-symlink", lambda root: ((root / "RELEASE-MANIFEST").unlink(), (root / "RELEASE-MANIFEST").symlink_to(valid / "RELEASE-MANIFEST")), re_sign=False)
    bad_policy = temp / "bad-policy"; bad_policy.write_bytes(policy.read_bytes().replace(b"abi=4", b"abi=3"))
    try: m.verify(valid, trusted, identity, policy=bad_policy)
    except m.Invalid: checks.append("wrong-source-policy")
    else: raise AssertionError("Accepted wrong source policy")
    legacy = temp / "legacy"; shutil.copytree(valid, legacy)
    for target in m.TARGETS:
        path = legacy / target / "HARDENING-ATTESTATION"
        path.write_bytes(b"\n".join(path.read_bytes().splitlines()[:37]) + b"\nstaging_format=1\nrelease_build_id=old\nopenpencil_revision=" + B.encode() + b"\n")
    assert not m.has_local_marker(legacy)
    checks.append("legacy-dispatch-unchanged")
    # Signature verification receives the already captured payload bytes, never
    # a mutable source path. This specifically covers the parse/signature gap.
    seen_payloads = []
    original_signature = m.verify_signature
    def capture_signature(payload, signature, pem, temporary):
        assert isinstance(payload, bytes) and isinstance(signature, bytes)
        seen_payloads.append(sha(payload))
        return original_signature(payload, signature, pem, temporary)
    m.verify_signature = capture_signature
    m.verify(valid, trusted, identity)
    m.verify_signature = original_signature
    assert len(seen_payloads) == 11
    checks.append("signature-binds-captured-bytes")
    # Windows lacks POSIX O_NOFOLLOW. The fallback still rejects reparse/symlink
    # entries and binds the opened descriptor to the lstat identity before reading.
    import os
    flag = getattr(os, "O_NOFOLLOW", None)
    if flag is not None: delattr(os, "O_NOFOLLOW")
    try:
        m.verify(valid, trusted, identity)
        link = temp / "metadata-link"; link.symlink_to(trusted)
        try: m.read(link)
        except m.Invalid: pass
        else: raise AssertionError("Accepted symlink without POSIX flags")
    finally:
        if flag is not None: os.O_NOFOLLOW = flag
    checks.append("portable-nofollow-fallback")
    if hasattr(os, "mkfifo"):
        fifo = temp / "metadata-fifo"; os.mkfifo(fifo)
        try: m.read(fifo)
        except m.Invalid: checks.append("fifo-rejected-before-open")
        else: raise AssertionError("Accepted FIFO")


    # Both real shell gates route signed local-full data into the semantic
    # verifier; their canonical legacy path remains separately covered above.
    shadow = temp / "shadow"; (shadow / "tools").mkdir(parents=True)
    key_path = shadow / "crates/op-auth-bridge/prebuilt/PROVENANCE_PUBKEY"
    key_path.parent.mkdir(parents=True); key_path.write_bytes(trusted.read_bytes())
    policy_path = shadow / "crates/op-auth-bridge/AUTH-RELEASE-POLICY"
    policy_path.write_bytes(policy.read_bytes())
    for name in ["check-local-full-auth.py", "check-op-auth-release-matrix.sh", "verify-signed-op-auth-matrix.sh", "op-auth-candidate-targets.sh"]:
        data = (HERE / name).read_text()
        if name == "check-local-full-auth.py": data = data.replace("07e073b30d6de0df7343f8a11c4dccaf69aa87518d94917375eda6e63cc506e1", m.PIN)
        (shadow / "tools" / name).write_text(data)
    import os
    env = {**os.environ, "OP_AUTH_PREBUILT_ROOT": str(valid)}
    run_result = subprocess.run(["bash", str(shadow / "tools/check-op-auth-release-matrix.sh")], env=env, capture_output=True)
    assert run_result.returncode == 0, run_result.stderr.decode()
    signed_command = ["bash", str(shadow / "tools/verify-signed-op-auth-matrix.sh"), "--signed-root", str(valid), "--trusted-public-key", str(key_path), "--version", VERSION, "--private-head-sha", P, "--openpencil-sha", B, "--build-id", BUILD, "--abi", "4"]
    assert subprocess.run(signed_command, capture_output=True).returncode == 0
    checks.append("shell-dispatch-local-positive")
    mixed = temp / "mixed-legacy-staging"
    policy_path.write_text(policy.read_text().replace(m.digest(valid / "RELEASE-MANIFEST"), m.digest(mixed / "RELEASE-MANIFEST")))
    assert subprocess.run(["bash", str(shadow / "tools/check-op-auth-release-matrix.sh")], env={**env, "OP_AUTH_PREBUILT_ROOT": str(mixed)}, capture_output=True).returncode != 0
    signed_command[signed_command.index("--signed-root") + 1] = str(mixed)
    assert subprocess.run(signed_command, capture_output=True).returncode != 0
    checks.append("shell-dispatch-mixed-rejected")
    print(json.dumps({"ok": True, "checks": len(checks), "names": checks}, indent=2))
