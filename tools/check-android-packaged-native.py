#!/usr/bin/env python3
"""Bind Android package JNI bytes to the fresh, canonical AGP strip outputs."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import stat
import sys
import tempfile
import zipfile


ABIS = ("arm64-v8a", "x86_64")
LIBRARY = "libop_engine_jni.so"
MAX_NATIVE_BYTES = 512 * 1024 * 1024


def require(condition, message):
    if not condition:
        raise ValueError(message)


def directory(path):
    require(path.is_dir() and not path.is_symlink(), "JNI roots and ABI directories must be ordinary directories")


def regular(path):
    require(path.is_file() and not path.is_symlink(), "Native inputs must be regular, non-symlink files")


def native_digest(stream, expected_size):
    digest = hashlib.sha256()
    size = 0
    while block := stream.read(1024 * 1024):
        size += len(block)
        require(size <= MAX_NATIVE_BYTES, "Native payload exceeds its size bound")
        digest.update(block)
    require(size == expected_size, "Native payload size changed while hashing")
    return digest.hexdigest()


def native_file(root, abi):
    directory(root)
    directory(root / abi)
    path = root / abi / LIBRARY
    regular(path)
    before = path.stat()
    require(0 < before.st_size <= MAX_NATIVE_BYTES, "Native input is empty or exceeds its size bound")
    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_BINARY", 0)
    descriptor = os.open(path, flags)
    with os.fdopen(descriptor, "rb") as stream:
        opened = os.fstat(stream.fileno())
        require(stat.S_ISREG(opened.st_mode) and (before.st_dev, before.st_ino) ==
                (opened.st_dev, opened.st_ino), "Native input changed while opening")
        digest = native_digest(stream, opened.st_size)
        after = os.fstat(stream.fileno())
    require((opened.st_size, opened.st_mtime_ns) == (after.st_size, after.st_mtime_ns),
            "Native input changed while hashing")
    return digest, opened.st_size


def packaged_files(path, prefix):
    regular(path)
    wanted = {prefix + abi + "/" + LIBRARY for abi in ABIS}
    result = {}
    with zipfile.ZipFile(path) as archive:
        native = [info for info in archive.infolist()
                  if info.filename == LIBRARY or info.filename.endswith("/" + LIBRARY)
                  or info.filename.endswith("\\" + LIBRARY)]
        require(len(native) == len(wanted) and {info.filename for info in native} == wanted,
                "Package must contain exactly one JNI entry for each required ABI")
        for info in native:
            require(not info.is_dir() and not info.flag_bits & 1
                    and stat.S_IFMT(info.external_attr >> 16) in (0, stat.S_IFREG),
                    "Packaged JNI entry must be an ordinary unencrypted file")
            require(0 < info.file_size <= MAX_NATIVE_BYTES, "Packaged JNI entry is empty or exceeds its size bound")
            with archive.open(info) as stream:
                digest = native_digest(stream, info.file_size)
            abi = info.filename.removeprefix(prefix).split("/", 1)[0]
            result[abi] = (digest, info.file_size)
    return result


def publish_audit(path, audit):
    directory(path.parent)
    require(not path.exists() and not path.is_symlink(), "Native audit output must not already exist")
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", dir=path.parent,
                                         prefix=".android-native-audit-", delete=False) as stream:
            temporary = Path(stream.name)
            json.dump(audit, stream, indent=2, sort_keys=True)
            stream.write("\n")
        os.link(temporary, path)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def check(args):
    require(not args.audit_output.exists() and not args.audit_output.is_symlink(),
            "Native audit output must not already exist")
    apk = packaged_files(args.apk, "lib/")
    aab = packaged_files(args.aab, "base/lib/")
    records = []
    for abi in ABIS:
        raw_sha, raw_size = native_file(args.raw_jni_root, abi)
        shipping_sha, shipping_size = native_file(args.stripped_jni_root, abi)
        require(apk[abi] == (shipping_sha, shipping_size),
                "APK JNI differs from the canonical Gradle stripped output for " + abi)
        require(aab[abi] == (shipping_sha, shipping_size),
                "AAB JNI differs from the canonical Gradle stripped output for " + abi)
        records.append({"abi": abi, "raw_sha256": raw_sha, "raw_bytes": raw_size,
                        "shipping_sha256": shipping_sha, "shipping_bytes": shipping_size})
    publish_audit(args.audit_output, {"format": "openpencil-android-native-inventory-v1",
                                    "canonical_stage": "fresh-agp-stripReleaseDebugSymbols",
                                    "targets": records})
    for field, record in zip(("arm64_v8a_sha256", "x86_64_sha256"), records):
        print(field + "=" + record["shipping_sha256"])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("raw-jni-root", "stripped-jni-root", "apk", "aab", "audit-output"):
        parser.add_argument("--" + name, type=Path, required=True)
    try:
        check(parser.parse_args())
    except (OSError, ValueError, RuntimeError, zipfile.BadZipFile) as error:
        print("error: " + str(error), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
