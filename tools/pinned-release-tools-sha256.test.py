#!/usr/bin/env python3
"""Exercise both real checksum backends with escaped artifact paths."""
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

SOURCE = Path(__file__).with_name("pinned-release-tools.sh").read_text()
FUNCTIONS = SOURCE.split("\nskia_file_url() {", 1)[0]
BASH = shutil.which("bash")
AWK = shutil.which("awk")
PAYLOAD = b"binary release archive\r\n\x00\xff\x1a\n"
DIGEST = hashlib.sha256(PAYLOAD).hexdigest()


class ChecksumPaths(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.backends = {}
        for name, executable in (
            ("sha256sum", shutil.which("gsha256sum") or shutil.which("sha256sum")),
            ("shasum", shutil.which("shasum")),
        ):
            if executable:
                directory = self.root / name
                directory.mkdir()
                (directory / name).symlink_to(executable)
                (directory / "awk").symlink_to(AWK)
                self.backends[name] = {**os.environ, "PATH": str(directory)}
        self.assertTrue(self.backends, "No real checksum backend is available")

    def run_function(self, backend, command, *arguments):
        return subprocess.run(
            [BASH, "-c", FUNCTIONS + "\n" + command, "checksum-test", *arguments],
            env=self.backends[backend], capture_output=True, text=True, timeout=10,
        )

    def test_exact_binary_digest_with_escaped_names(self):
        for index, name in enumerate(("ordinary", "with spaces", r"D:\a\_temp\archive", "with\nnewline")):
            path = self.root / (str(index) + name)
            path.write_bytes(PAYLOAD)
            for backend in self.backends:
                with self.subTest(backend=backend, filename=name):
                    result = self.run_function(backend, 'sha256_file "$1"', str(path))
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(result.stdout, DIGEST + "\n")
                    verified = self.run_function(backend, 'verify_sha256 "$1" "$2"', DIGEST, str(path))
                    self.assertEqual(verified.returncode, 0, verified.stderr)

    def test_changed_binary_is_rejected(self):
        path = self.root / r"D:\a\_temp\tampered"
        path.write_bytes(PAYLOAD + b"changed")
        for backend in self.backends:
            with self.subTest(backend=backend):
                result = self.run_function(backend, 'verify_sha256 "$1" "$2"', DIGEST, str(path))
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("SHA-256 mismatch", result.stderr)

    def test_noncanonical_expected_digest_is_rejected(self):
        path = self.root / "payload"
        path.write_bytes(PAYLOAD)
        for backend in self.backends:
            for expected in ("\\" + DIGEST, DIGEST.upper(), DIGEST[:-1], DIGEST + "\n"):
                with self.subTest(backend=backend, expected=expected):
                    result = self.run_function(backend, 'verify_sha256 "$1" "$2"', expected, str(path))
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn("malformed pinned SHA-256", result.stderr)

    def test_missing_and_symlink_inputs_are_rejected(self):
        path = self.root / "payload"
        path.write_bytes(PAYLOAD)
        link = self.root / "link"
        link.symlink_to(path)
        for backend in self.backends:
            for candidate in (link, self.root / "missing"):
                with self.subTest(backend=backend, candidate=candidate.name):
                    result = self.run_function(backend, 'verify_sha256 "$1" "$2"', DIGEST, str(candidate))
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn("not a regular file", result.stderr)


if __name__ == "__main__":
    unittest.main()
