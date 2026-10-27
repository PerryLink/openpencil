#!/usr/bin/env python3
"""Test the packaged JNI byte contract; synthetic payloads are not valid ELF."""

import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import warnings
import zipfile


CHECKER = Path(__file__).with_name("check-android-packaged-native.py")
ABIS = ("arm64-v8a", "x86_64")
LIBRARY = "libop_engine_jni.so"
RAW = {abi: b"raw native input\x00" + abi.encode() + b"\xffsymbols" for abi in ABIS}
SHIPPING = {abi: b"canonical AGP output\x00" + abi.encode() + b"\xfe" for abi in ABIS}


def digest(payload):
    return hashlib.sha256(payload).hexdigest()


class Fixture:
    def __init__(self, root):
        self.root = root
        self.raw = root / "raw JNI"
        self.stripped = root / "stripped JNI"
        self.apk = root / "unsigned app.apk"
        self.aab = root / "unsigned app.aab"
        self.audit = root / "native audit.json"
        for directory, payloads in ((self.raw, RAW), (self.stripped, SHIPPING)):
            for abi, payload in payloads.items():
                native = directory / abi / LIBRARY
                native.parent.mkdir(parents=True)
                native.write_bytes(payload)
        self.write_archive("apk")
        self.write_archive("aab")

    def entries(self, kind):
        prefix = "lib" if kind == "apk" else "base/lib"
        return [(f"{prefix}/{abi}/{LIBRARY}", SHIPPING[abi]) for abi in ABIS]

    def write_archive(self, kind, entries=None):
        with zipfile.ZipFile(getattr(self, kind), "w", zipfile.ZIP_DEFLATED) as archive:
            archive.writestr("assets/unrelated.txt", b"unrelated packaged asset")
            for name, payload in self.entries(kind) if entries is None else entries:
                archive.writestr(name, payload)

    def run(self):
        return subprocess.run(
            [
                sys.executable,
                str(CHECKER),
                "--raw-jni-root", str(self.raw),
                "--stripped-jni-root", str(self.stripped),
                "--apk", str(self.apk),
                "--aab", str(self.aab),
                "--audit-output", str(self.audit),
            ],
            capture_output=True,
            text=True,
            timeout=10,
        )


class PackagedNativeContract(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="android native contract ")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.fixture_index = 0

    def fixture(self):
        self.fixture_index += 1
        return Fixture(self.root / str(self.fixture_index))

    def assert_rejected(self, fixture, audit_already_exists=False):
        result = fixture.run()
        self.assertNotEqual(result.returncode, 0, "invalid JNI inventory was accepted")
        self.assertEqual(result.stdout, "", "failure emitted a partial digest handoff")
        self.assertTrue(result.stderr.strip(), "failure must report a diagnostic")
        if not audit_already_exists:
            self.assertFalse(fixture.audit.exists(), "failure created an audit receipt")
            self.assertFalse(fixture.audit.is_symlink(), "failure created an audit symlink")

    def test_stripped_shipping_bytes_drive_manifest_and_audit(self):
        fixture = self.fixture()
        result = fixture.run()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(
            result.stdout,
            f"arm64_v8a_sha256={digest(SHIPPING['arm64-v8a'])}\n"
            f"x86_64_sha256={digest(SHIPPING['x86_64'])}\n",
        )
        audit = json.loads(fixture.audit.read_text())
        records = audit["targets"]
        self.assertEqual(len(records), len(ABIS))
        targets = {record["abi"]: record for record in records}
        self.assertEqual(set(targets), set(ABIS))
        for abi in ABIS:
            with self.subTest(abi=abi):
                self.assertNotEqual(RAW[abi], SHIPPING[abi])
                self.assertEqual(targets[abi]["raw_sha256"], digest(RAW[abi]))
                self.assertEqual(targets[abi]["shipping_sha256"], digest(SHIPPING[abi]))

    def test_changed_packaged_bytes_are_rejected(self):
        for kind in ("apk", "aab"):
            for abi_index, abi in enumerate(ABIS):
                with self.subTest(archive=kind, abi=abi):
                    fixture = self.fixture()
                    entries = fixture.entries(kind)
                    name, payload = entries[abi_index]
                    entries[abi_index] = (name, payload + b"tampered")
                    fixture.write_archive(kind, entries)
                    self.assert_rejected(fixture)

    def test_agreeing_packages_cannot_override_canonical_stripped_bytes(self):
        fixture = self.fixture()
        for kind in ("apk", "aab"):
            entries = [(name, payload + b"same tampering") for name, payload in fixture.entries(kind)]
            fixture.write_archive(kind, entries)
        self.assert_rejected(fixture)

    def test_raw_bytes_cannot_replace_canonical_stripped_bytes(self):
        fixture = self.fixture()
        for kind in ("apk", "aab"):
            entries = [(name, RAW[abi]) for abi, (name, _) in zip(ABIS, fixture.entries(kind))]
            fixture.write_archive(kind, entries)
        self.assert_rejected(fixture)

    def test_missing_required_archive_entry_is_rejected(self):
        for kind in ("apk", "aab"):
            for abi_index, abi in enumerate(ABIS):
                with self.subTest(archive=kind, abi=abi):
                    fixture = self.fixture()
                    entries = fixture.entries(kind)
                    del entries[abi_index]
                    fixture.write_archive(kind, entries)
                    self.assert_rejected(fixture)

    def test_duplicate_required_archive_entry_is_rejected(self):
        for kind in ("apk", "aab"):
            for abi_index, abi in enumerate(ABIS):
                with self.subTest(archive=kind, abi=abi):
                    fixture = self.fixture()
                    entries = fixture.entries(kind)
                    entries.append(entries[abi_index])
                    with warnings.catch_warnings():
                        warnings.filterwarnings("ignore", message="Duplicate name:", category=UserWarning)
                        fixture.write_archive(kind, entries)
                    self.assert_rejected(fixture)

    def test_unexpected_packaged_jni_abi_is_rejected(self):
        for kind in ("apk", "aab"):
            with self.subTest(archive=kind):
                fixture = self.fixture()
                prefix = "lib" if kind == "apk" else "base/lib"
                entries = fixture.entries(kind) + [(f"{prefix}/armeabi-v7a/{LIBRARY}", b"unexpected ABI")]
                fixture.write_archive(kind, entries)
                self.assert_rejected(fixture)

    def test_empty_raw_or_canonical_native_file_is_rejected(self):
        for kind in ("raw", "stripped"):
            for abi in ABIS:
                with self.subTest(root=kind, abi=abi):
                    fixture = self.fixture()
                    (getattr(fixture, kind) / abi / LIBRARY).write_bytes(b"")
                    self.assert_rejected(fixture)

    def test_empty_packaged_native_payload_is_rejected(self):
        for kind in ("apk", "aab"):
            for abi_index, abi in enumerate(ABIS):
                with self.subTest(archive=kind, abi=abi):
                    fixture = self.fixture()
                    entries = fixture.entries(kind)
                    entries[abi_index] = (entries[abi_index][0], b"")
                    fixture.write_archive(kind, entries)
                    self.assert_rejected(fixture)

    def test_empty_canonical_and_matching_packages_are_rejected(self):
        fixture = self.fixture()
        for abi in ABIS:
            (fixture.stripped / abi / LIBRARY).write_bytes(b"")
        for kind in ("apk", "aab"):
            fixture.write_archive(kind, [(name, b"") for name, _ in fixture.entries(kind)])
        self.assert_rejected(fixture)

    def test_symlinked_raw_or_stripped_native_file_is_rejected(self):
        for kind in ("raw", "stripped"):
            for abi in ABIS:
                with self.subTest(root=kind, abi=abi):
                    fixture = self.fixture()
                    native = getattr(fixture, kind) / abi / LIBRARY
                    target = fixture.root / "real native file.so"
                    native.rename(target)
                    native.symlink_to(target)
                    self.assert_rejected(fixture)

    def test_symlinked_raw_or_stripped_abi_directory_is_rejected(self):
        for kind in ("raw", "stripped"):
            for abi in ABIS:
                with self.subTest(root=kind, abi=abi):
                    fixture = self.fixture()
                    directory = getattr(fixture, kind) / abi
                    target = fixture.root / "real ABI directory"
                    directory.rename(target)
                    directory.symlink_to(target, target_is_directory=True)
                    self.assert_rejected(fixture)

    def test_existing_audit_file_is_preserved_and_rejected(self):
        fixture = self.fixture()
        previous = b"previous audit receipt\x00\xff"
        fixture.audit.write_bytes(previous)
        self.assert_rejected(fixture, audit_already_exists=True)
        self.assertEqual(fixture.audit.read_bytes(), previous)


if __name__ == "__main__":
    unittest.main()
