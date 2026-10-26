#!/usr/bin/env python3
"""Strict data-only semantics for complete local Auth ABI 4 publications."""
from __future__ import annotations
import argparse
import hashlib
import os
import stat
from pathlib import Path
import re
import subprocess
import sys
import tempfile

PIN = "07e073b30d6de0df7343f8a11c4dccaf69aa87518d94917375eda6e63cc506e1"
TARGETS = (
    "aarch64-apple-darwin", "aarch64-apple-ios", "aarch64-apple-ios-sim",
    "aarch64-linux-android", "aarch64-pc-windows-msvc", "aarch64-unknown-linux-gnu",
    "x86_64-apple-darwin", "x86_64-linux-android", "x86_64-pc-windows-msvc",
    "x86_64-unknown-linux-gnu",
)
WINDOWS = ("aarch64-pc-windows-msvc", "x86_64-pc-windows-msvc")
RAW_KEYS = (
    "format", "mode", "abi", "target", "artifact", "artifact_sha256", "artifact_size",
    "source_revision", "source_tree_state", "source_date_epoch", "profile", "paths",
    "debug", "symbols", "dead_code", "audit_profile", "abi_allowlist_sha256",
    "global_defined_symbols_sha256", "section_inventory_sha256", "object_format_count",
    "debug_section_count", "link_validation", "link_execution", "target_platform",
    "minimum_platform_version", "linked_binary_sha256", "obfuscation_review",
    "protection_tool_sha256", "review_binding_sha256", "linux_glibc_baseline",
    "linux_sysroot", "zig_version", "zig_sha256", "zig_sysroot_sha256",
    "cargo_zigbuild_version", "cargo_zigbuild_sha256", "toolchain",
)
LOCAL_KEYS = ("staging_format", "release_build_id", "openpencil_revision",
              "signer_revision", "execution_evidence_sha256")
SYMBOLS = tuple(sorted((
    "op_auth_abi_version", "op_auth_cancel", "op_auth_collab_relay_token_begin",
    "op_auth_collab_ticket_begin", "op_auth_collab_ticket_cancel", "op_auth_collab_ticket_poll",
    "op_auth_hosted_access_begin", "op_auth_hosted_access_cancel", "op_auth_hosted_access_poll",
    "op_auth_hosted_session_epoch", "op_auth_login_begin", "op_auth_poll", "op_auth_restore",
    "op_auth_runtime_init", "op_auth_sign_out", "op_auth_string_free",
)))
HEX40 = re.compile(r"[0-9a-f]{40}\Z")
HEX64 = re.compile(r"[0-9a-f]{64}\Z")
VERSION = re.compile(r"[0-9]+\.[0-9]+\.[0-9]+(?:[+-][0-9A-Za-z.-]+)?\Z")
BUILD = re.compile(r"(?:[A-Za-z0-9]|[A-Za-z0-9][A-Za-z0-9._-]{0,57}[A-Za-z0-9])\Z")


class Invalid(ValueError):
    pass


def require(ok, message):
    if not ok:
        raise Invalid(message)


def safe_stat(info, directory=False):
    expected = stat.S_ISDIR(info.st_mode) if directory else stat.S_ISREG(info.st_mode)
    return expected and not stat.S_ISLNK(info.st_mode) and not (getattr(info, "st_file_attributes", 0) & 0x400)


def regular(path):
    require(safe_stat(path.lstat()), f"Not a regular non-reparse file: {path}")


def directory(path):
    require(safe_stat(path.lstat(), directory=True), f"Not a non-reparse directory: {path}")


def same_stat(a, b):
    return (a.st_dev, a.st_ino, a.st_size, a.st_mtime_ns) == (b.st_dev, b.st_ino, b.st_size, b.st_mtime_ns)


def open_regular(path):
    before = path.lstat()
    require(safe_stat(before), "Non-regular or reparse input")
    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_NONBLOCK", 0) | getattr(os, "O_BINARY", 0)
    descriptor = os.open(path, flags)
    try:
        opened = os.fstat(descriptor)
        require(safe_stat(opened) and same_stat(before, opened), "Input changed while opening")
    except BaseException:
        os.close(descriptor)
        raise
    return descriptor, opened


def file_digest(path):
    descriptor, before = open_regular(path)
    with os.fdopen(descriptor, "rb") as stream:
        h = hashlib.sha256()
        for data in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(data)
        after = os.fstat(stream.fileno())
    require((before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns) ==
            (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns), "Input changed during hashing")
    return h.hexdigest(), before.st_size


def digest(path):
    return file_digest(path)[0]


def read(path):
    descriptor, before = open_regular(path)
    with os.fdopen(descriptor, "rb") as stream:
        require(before.st_size <= 131072, "Oversized metadata")
        data = stream.read(131073)
        after = os.fstat(stream.fileno())
    require(len(data) <= 131072 and len(data) == before.st_size and
            (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns) ==
            (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns), "Metadata changed during bounded read")
    return data


def canonical_fields(data, keys, label):
    require(data.endswith(b"\n") and b"\r" not in data and b"\0" not in data,
            f"Non-canonical text: {label}")
    lines = data[:-1].decode("utf-8").split("\n")
    require(len(lines) == len(keys), f"Wrong field count: {label}")
    values = {}
    for line, key in zip(lines, keys):
        require(line.startswith(key + "=") and len(line) > len(key) + 1,
                f"Non-canonical field {key}: {label}")
        values[key] = line[len(key) + 1:]
    return values


def artifact_name(target):
    return "op_auth.lib" if target in WINDOWS else "libop_auth.a"


def platform(target):
    if target == "aarch64-apple-ios": return "ios"
    if target == "aarch64-apple-ios-sim": return "ios-simulator"
    if target.endswith("-linux-android"): return "android"
    if target.endswith("-apple-darwin"): return "macos"
    if target.endswith("-unknown-linux-gnu"): return "linux"
    return "windows"


def identity(version, source, public, build):
    require(VERSION.fullmatch(version) is not None, "Invalid version")
    require(HEX40.fullmatch(source) is not None and HEX40.fullmatch(public) is not None,
            "Invalid source revision")
    require(BUILD.fullmatch(build) is not None and ".." not in build and not build.endswith(".lock"),
            "Invalid build id")


def validate_raw(value, target, source, sha, size, *, pending=False):
    require(tuple(value) == RAW_KEYS, "Raw attestation keys/order differ")
    expected = {
        "format": "3", "mode": "local-cross-pending" if pending else "production", "abi": "4",
        "target": target, "artifact": artifact_name(target), "artifact_sha256": sha,
        "artifact_size": str(size), "source_revision": source, "source_tree_state": "clean",
        "profile": "private-ci-fat-lto", "paths": "remapped", "debug": "none",
        "symbols": "stripped", "dead_code": "eliminated", "audit_profile": "op-auth-hardened-v3",
        "target_platform": platform(target), "obfuscation_review": "source-" + source,
    }
    for key, wanted in expected.items():
        require(value[key] == wanted, f"Hardening {key} mismatch: {target}")
    for key in ("abi_allowlist_sha256", "global_defined_symbols_sha256", "section_inventory_sha256",
                "linked_binary_sha256", "protection_tool_sha256", "review_binding_sha256"):
        require(HEX64.fullmatch(value[key]) is not None, f"Invalid {key}: {target}")
    for key in ("source_date_epoch", "object_format_count", "debug_section_count"):
        require(re.fullmatch(r"(?:0|[1-9][0-9]*)", value[key]) is not None, f"Invalid {key}")
    require(int(value["source_date_epoch"]) > 0 and int(value["object_format_count"]) > 0, "Missing build identity")
    decoration = "_" if "-apple-" in target else ""
    symbol_hash = hashlib.sha256("".join(decoration + symbol + "\n" for symbol in SYMBOLS).encode()).hexdigest()
    require(value["abi_allowlist_sha256"] == symbol_hash, f"Wrong 16-symbol allowlist: {target}")
    if target not in WINDOWS:
        require(value["global_defined_symbols_sha256"] == symbol_hash, f"Unexpected globals: {target}")
        require(value["debug_section_count"] == "0", f"Debug sections remain: {target}")
    binding = (f"op-auth-protector-binding-v1\ntarget={target}\nreview_id=source-{source}\n"
               f"protector_sha256={value['protection_tool_sha256']}\n").encode()
    require(value["review_binding_sha256"] == hashlib.sha256(binding).hexdigest(), "Protector review binding mismatch")
    if pending:
        require(target in WINDOWS or target == "x86_64-apple-darwin", "Unexpected deferred target")
        require((value["link_validation"], value["link_execution"], value["minimum_platform_version"]) ==
                ("cross-final-link", "pending", "native"), "Pending cross state is not canonical")
    elif platform(target) in ("ios", "ios-simulator", "android"):
        minimum = "21" if platform(target) == "android" else "15.0"
        require((value["link_validation"], value["link_execution"], value["minimum_platform_version"]) ==
                ("cross-final-link", "not-applicable", minimum), "Mobile link contract mismatch")
    else:
        allowed = {"native-link-run"}
        if target in WINDOWS or target == "x86_64-apple-darwin": allowed.add("cross-final-link")
        require(value["link_validation"] in allowed and value["link_execution"] == "passed"
                and value["minimum_platform_version"] == "native", "Desktop execution contract mismatch")
    if target.endswith("-unknown-linux-gnu") and value["linux_glibc_baseline"] == "2.17":
        require(value["linux_sysroot"] == "zig-glibc-2.17", "Wrong glibc sysroot")
        for key in ("zig_sha256", "zig_sysroot_sha256", "cargo_zigbuild_sha256"):
            require(HEX64.fullmatch(value[key]) is not None, "Malformed reviewed tool digest")
        for key in ("zig_version", "cargo_zigbuild_version"):
            require(re.fullmatch(r"[0-9A-Za-z.+-]{1,64}", value[key]) is not None and value[key] != "none", "Missing reviewed tool version")
    else:
        for key in ("linux_glibc_baseline", "linux_sysroot", "zig_version", "zig_sha256",
                    "zig_sysroot_sha256", "cargo_zigbuild_version", "cargo_zigbuild_sha256"):
            require(value[key] == "none", f"Unexpected toolchain claim: {key}")
    require(re.match(r"rustc [0-9]+\.[0-9]+\.[0-9]+ \(", value["toolchain"]) is not None,
            "Missing actual Rust toolchain")
    return value


def validate_local_hardening(path, target, source, public, build, sha, size):
    value = canonical_fields(path if isinstance(path, bytes) else read(path), RAW_KEYS + LOCAL_KEYS, target)
    raw = {key: value[key] for key in RAW_KEYS}
    validate_raw(raw, target, source, sha, size)
    require(value["staging_format"] == "local-full-v1", "Not a local complete-matrix attestation")
    require(value["release_build_id"] == build and value["openpencil_revision"] == public,
            "Local release binding mismatch")
    require(HEX40.fullmatch(value["signer_revision"]) is not None, "Invalid signer source")
    require(HEX64.fullmatch(value["execution_evidence_sha256"]) is not None
            and value["execution_evidence_sha256"] != "0" * 64, "Missing execution evidence digest")
    return value


def has_local_marker(root):
    for target in TARGETS:
        path = root / target / "HARDENING-ATTESTATION"
        if path.is_file() and not path.is_symlink():
            data = read(path)
            if any(token in data for token in (b"staging_format=local-", b"signer_revision=", b"execution_evidence_sha256=")):
                return True
    return False


def verify_signature(payload, signature, pem, temporary):
    sig = signature
    require(re.fullmatch(rb"[0-9a-f]{128}\n", sig) is not None, "Non-canonical signature")
    captured = temporary / "captured-payload.bin"
    captured.write_bytes(payload)
    binary = temporary / "signature.bin"
    binary.write_bytes(bytes.fromhex(sig.decode().strip()))
    p = subprocess.run(["openssl", "pkeyutl", "-verify", "-rawin", "-pubin", "-inkey", str(pem),
                        "-in", str(captured), "-sigfile", str(binary)], capture_output=True)
    require(p.returncode == 0, "Invalid signature over captured metadata")


def verify(root, trusted, expected=None, *, signatures=True, policy=None):
    directory(root)
    key = read(trusted)
    require(key == (PIN + "\n").encode(), "Source-owned public key is not the established root")
    required = set(TARGETS) | {"RELEASE-MANIFEST"}
    if signatures: required.add("RELEASE-MANIFEST.sig")
    names = {p.name for p in root.iterdir()}
    require(required <= names and names <= required | {"PROVENANCE_PUBKEY", "README.md"}, "Missing or extra matrix entries")
    for item in root.iterdir():
        require(not item.is_symlink(), "Symlinked root entry")
    for optional in ("PROVENANCE_PUBKEY", "README.md"):
        if (root / optional).exists(): regular(root / optional)
    manifest = root / "RELEASE-MANIFEST"
    raw = read(manifest)
    manifest_hash = hashlib.sha256(raw).hexdigest()
    require(raw.endswith(b"\n") and b"\r" not in raw, "Non-canonical manifest text")
    rows = raw[:-1].decode().split("\n")
    require(len(rows) == 17, "Complete ten-target manifest must have 17 lines")
    matrix = canonical_fields(("\n".join(rows[:7]) + "\n").encode(),
                              ("format", "version", "abi", "source_revision", "openpencil_revision", "build_id", "target_count"), "manifest")
    require(matrix["format"] == "op-auth-release-matrix-v1" and matrix["abi"] == "4" and matrix["target_count"] == "10", "Wrong complete-matrix format/ABI")
    identity(matrix["version"], matrix["source_revision"], matrix["openpencil_revision"], matrix["build_id"])
    if expected:
        for key, wanted in expected.items(): require(matrix[key] == wanted, f"Manifest {key} mismatch")
    signer = evidence = None
    with tempfile.TemporaryDirectory(prefix="op-local-full-data-") as tmp:
        temporary = Path(tmp)
        der = temporary / "public.der"; der.write_bytes(bytes.fromhex("302a300506032b6570032100" + PIN))
        pem = temporary / "public.pem"
        subprocess.run(["openssl", "pkey", "-pubin", "-inform", "DER", "-in", str(der), "-out", str(pem)], capture_output=True, check=True)
        if signatures: verify_signature(raw, read(root / "RELEASE-MANIFEST.sig"), pem, temporary)
        for target, row in zip(TARGETS, rows[7:]):
            name = artifact_name(target)
            match = re.fullmatch(r"target=([^|]+)\|artifact=([^|]+)\|sha256=([0-9a-f]{64})\|provenance_sha256=([0-9a-f]{64})\|hardening_sha256=([0-9a-f]{64})", row)
            require(match is not None and match[1] == target and match[2] == name, "Target order or artifact mismatch")
            entry = root / target; directory(entry)
            files = {name, "VERSION", "ABI_VERSION", "SHA256", "PROVENANCE", "HARDENING-ATTESTATION"}
            if signatures: files.add("PROVENANCE.sig")
            require({p.name for p in entry.iterdir()} == files, f"Target file schema mismatch: {target}")
            for file in files: regular(entry / file)
            artifact_sha, provenance_sha, hardening_sha = match[3], match[4], match[5]
            archive_hash, archive_size = file_digest(entry / name)
            captured_provenance = read(entry / "PROVENANCE")
            captured_hardening = read(entry / "HARDENING-ATTESTATION")
            require(archive_hash == artifact_sha and hashlib.sha256(captured_provenance).hexdigest() == provenance_sha
                    and hashlib.sha256(captured_hardening).hexdigest() == hardening_sha, f"Target bytes mismatch: {target}")
            for file, value in (("VERSION", matrix["version"]), ("ABI_VERSION", "4"), ("SHA256", artifact_sha)):
                require(read(entry / file) == (value + "\n").encode(), f"Invalid {file}")
            profile = "op-auth-signed-unobfuscated-v1" if target in WINDOWS else "op-auth-hardened-v1"
            provenance = (f"format=1\ntarget={target}\nartifact={name}\nversion={matrix['version']}\nabi=4\n"
                          f"sha256={artifact_sha}\nhardening={profile}\nsource_revision={matrix['source_revision']}\n"
                          f"build_id={matrix['build_id']}.a4.{hardening_sha}\n").encode()
            require(captured_provenance == provenance, "Non-canonical target provenance")
            if signatures: verify_signature(captured_provenance, read(entry / "PROVENANCE.sig"), pem, temporary)
            hat = validate_local_hardening(captured_hardening, target, matrix["source_revision"], matrix["openpencil_revision"], matrix["build_id"], artifact_sha, archive_size)
            if signer is None: signer, evidence = hat["signer_revision"], hat["execution_evidence_sha256"]
            require((hat["signer_revision"], hat["execution_evidence_sha256"]) == (signer, evidence), "Mixed signer or execution receipt")
    if policy:
        expected_policy = (f"format=op-auth-release-policy-v1\nabi=4\npublic_key={PIN}\nrelease_manifest_sha256={manifest_hash}\n"
                           f"source_revision={matrix['source_revision']}\nbuild_id={matrix['build_id']}\n").encode()
        require(read(policy) == expected_policy, "Policy does not adopt this exact complete matrix")
    return {**matrix, "signer_revision": signer, "execution_evidence_sha256": evidence}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--signed-root", type=Path, required=True)
    parser.add_argument("--trusted-public-key", type=Path, required=True)
    parser.add_argument("--policy", type=Path)
    parser.add_argument("--if-local-full", action="store_true")
    for name in ("version", "source-revision", "openpencil-revision", "build-id"):
        parser.add_argument("--" + name)
    args = parser.parse_args()
    if args.if_local_full and not has_local_marker(args.signed_root): return
    expected = {name: getattr(args, name) for name in ("version", "source_revision", "openpencil_revision", "build_id") if getattr(args, name)}
    verify(args.signed_root, args.trusted_public_key, expected, policy=args.policy)
    print("check-local-full-auth: verified complete signed ABI 4 local-full semantics")


if __name__ == "__main__":
    try: main()
    except (Invalid, OSError, ValueError, UnicodeError, KeyError, subprocess.SubprocessError) as error:
        print(f"error: {error}", file=sys.stderr)
        raise SystemExit(1)
