#!/usr/bin/env python3
"""Synthetic input/runner-boundary tests; no private archive is used."""
import copy
import contextlib
import io
import importlib.util
import json
import os
from pathlib import Path
import struct
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("runtime_verifier", Path(__file__).with_name("validate-local-windows-auth.py"))
V = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(V)


def member(name, payload):
    header = f"{name:<16}{0:<12}{0:<6}{0:<6}{0:<8}{len(payload):<10}`\n".encode()
    return header + payload + (b"\n" if len(payload) & 1 else b"")


def archive(machine, missing=False):
    names = sorted(V.ABI_SYMBOLS)
    if missing:
        names.pop()
    symbols = struct.pack(">I", len(names)) + bytes(4 * len(names)) + b"\0".join(name.encode() for name in names) + b"\0"
    obj = struct.pack("<HHIIIHH", machine, 1, 0, 0, 0, 0, 0)
    return b"!<arch>\n" + member("/", symbols) + member("fixture.obj/", obj)


def executable(machine):
    result = bytearray(256)
    result[:2] = b"MZ"
    struct.pack_into("<I", result, 60, 64)
    result[64:68] = b"PE\0\0"
    struct.pack_into("<HH", result, 68, machine, 1)
    struct.pack_into("<HH", result, 84, 112, 0x0002)
    struct.pack_into("<H", result, 88, 0x20B)
    return bytes(result)


class InputValidationTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.manifest = {"schema": "op-auth-windows-runtime-input-v1", "abi": 4,
                         "private_source_sha": "a" * 40, "public_source_sha": "b" * 40,
                         "harness_source_sha256": "c" * 64, "targets": []}
        for target, (machine, _, _) in sorted(V.TARGETS.items()):
            directory = self.root / target
            directory.mkdir()
            lib, exe = directory / "op_auth.lib", directory / "FINAL-LINK-BINARY.exe"
            lib.write_bytes(archive(machine)); exe.write_bytes(executable(machine))
            self.manifest["targets"].append({"target": target, "archive": f"{target}/op_auth.lib",
                "archive_sha256": V.digest(lib), "harness": f"{target}/FINAL-LINK-BINARY.exe", "harness_sha256": V.digest(exe)})
        self.write()

    def write(self):
        (self.root / "manifest.json").write_text(json.dumps(self.manifest))
        self.expected = V.digest(self.root / "manifest.json")

    def rejected(self):
        with self.assertRaises((V.ValidationError, ValueError, OSError)):
            V.load_inputs(self.root, self.expected)

    def test_complete_input_and_real_format_headers(self):
        data, entries = V.load_inputs(self.root, self.expected)
        self.assertEqual(data["abi"], 4)
        for target, entry in entries.items():
            self.assertEqual(V.inspect_archive(self.root / entry["archive"], V.TARGETS[target][0]), 1)
            V.inspect_pe(self.root / entry["harness"], V.TARGETS[target][0])

    def test_reviewed_manifest_digest_is_required(self):
        self.expected = "0" * 64
        self.rejected()

    def test_rejects_archive_or_executable_tampering(self):
        for field in ["archive", "harness"]:
            file = self.root / self.manifest["targets"][0][field]
            original = file.read_bytes()
            file.write_bytes(original + b"tamper")
            self.rejected()
            file.write_bytes(original)

    def test_rejects_wrong_abi_and_boolean_abi(self):
        for value in [3, 5, True, "4"]:
            self.manifest["abi"] = value
            self.write(); self.rejected()

    def test_rejects_nonobject_target_entries(self):
        self.manifest["targets"] = ["aarch64-pc-windows-msvc", "x86_64-pc-windows-msvc"]
        self.write(); self.rejected()

    def test_rejects_duplicate_json_keys(self):
        path = self.root / "manifest.json"
        path.write_text(path.read_text().replace('"abi": 4', '"abi": 4, "abi": 4'))
        self.expected = V.digest(path)
        self.rejected()

    def test_rejects_target_downgrade_and_duplicate(self):
        original = copy.deepcopy(self.manifest)
        for targets in [self.manifest["targets"][:1], [self.manifest["targets"][0]] * 2, list(reversed(self.manifest["targets"]))]:
            self.manifest["targets"] = targets
            self.write(); self.rejected()
            self.manifest = copy.deepcopy(original)

    def test_rejects_unknown_fields_and_path_escape(self):
        original = copy.deepcopy(self.manifest)
        self.manifest["extra"] = 1
        self.write(); self.rejected()
        self.manifest = original
        self.manifest["targets"][0]["harness"] = "../outside.exe"
        self.write(); self.rejected()

    def test_rejects_extra_files(self):
        (self.root / "unexpected").write_text("extra")
        self.rejected()

    def test_rejects_symlinked_input_ancestor(self):
        repository = self.root / "repository"
        repository.mkdir()
        outside = self.root / "outside"
        (outside / "windows").mkdir(parents=True)
        (repository / ".auth-validation").symlink_to(outside, target_is_directory=True)
        with self.assertRaises(V.ValidationError):
            V.validate_input_location(repository / ".auth-validation/windows", repository)

    def test_accepts_only_fixed_checkout_input_location(self):
        repository = self.root / "repository"
        location = repository / ".auth-validation/windows"
        location.mkdir(parents=True)
        V.validate_input_location(location, repository)
        with self.assertRaises(V.ValidationError):
            V.validate_input_location(self.root, repository)

    def test_rejects_manifest_symlink(self):
        file = self.root / "manifest.json"
        moved = self.root / "actual.json"
        file.rename(moved)
        file.symlink_to(moved)
        self.rejected()

    def test_rejects_coff_machine_or_required_symbol_mismatch(self):
        file = self.root / "sample.lib"
        for data in [archive(0x8664), archive(0xAA64, missing=True)]:
            file.write_bytes(data)
            with self.assertRaises(V.ValidationError):
                V.inspect_archive(file, 0xAA64)

    def test_rejects_malformed_archive_and_pe(self):
        file = self.root / "sample.lib"
        file.write_bytes(b"!<arch>\n" + b"bad")
        with self.assertRaises(V.ValidationError): V.inspect_archive(file, 0xAA64)
        exe = self.root / "sample.exe"
        for data in [b"MZ", executable(0x8664), b"not an executable" * 10]:
            exe.write_bytes(data)
            with self.assertRaises(V.ValidationError): V.inspect_pe(exe, 0xAA64)

    def test_run_context_must_match_windows_target_and_repository(self):
        env = {"GITHUB_REPOSITORY": "ZSeven-W/openpencil", "GITHUB_EVENT_NAME": "workflow_dispatch",
               "GITHUB_SHA": "d" * 40, "GITHUB_WORKFLOW_REF": "ZSeven-W/openpencil/.github/workflows/rust-release.yml@refs/heads/ci/auth",
               "RUNNER_OS": "Windows", "RUNNER_ARCH": "ARM64", "GITHUB_RUN_ID": "123", "GITHUB_RUN_ATTEMPT": "1",
               "GITHUB_JOB": "validate", "RUNNER_NAME": "GitHub Actions 1"}
        self.assertEqual(V.context("aarch64-pc-windows-msvc", env)["run_id"], 123)
        for name, value in [("GITHUB_REPOSITORY", "attacker/fork"), ("GITHUB_EVENT_NAME", "push"),
                            ("RUNNER_ARCH", "X64"), ("GITHUB_SHA", "main"), ("GITHUB_RUN_ATTEMPT", "0")]:
            altered = {**env, name: value}
            with self.assertRaises(V.ValidationError): V.context("aarch64-pc-windows-msvc", altered)

    def test_child_environment_excludes_github_commands_and_tokens(self):
        environment = {"SystemRoot": r"C:\Windows", "WINDIR": r"C:\Windows", "GITHUB_TOKEN": "fixture",
                       "GITHUB_OUTPUT": "fixture", "GITHUB_STEP_SUMMARY": "fixture", "ACTIONS_RUNTIME_TOKEN": "fixture",
                       "USERPROFILE": "fixture", "PATH": "untrusted"}
        with patch.dict(os.environ, environment, clear=True):
            child = V.child_environment(self.root)
        self.assertFalse(any(key.startswith(("GITHUB_", "ACTIONS_")) for key in child))
        self.assertNotIn("USERPROFILE", child)
        self.assertNotEqual(child["PATH"], "untrusted")


class HarnessBoundaryTests(unittest.TestCase):
    def run_fake(self, output=b"", exit_code=0, running=False, timeout=False, taskkill_missing=False):
        state = {"killed": False}
        class Process:
            pid = 12345
            returncode = None if running else exit_code
            def __init__(self, *args, **kwargs):
                kwargs["stdout"].write(output)
                kwargs["stdout"].flush()
                self.environment = kwargs["env"]
                state["environment"] = self.environment
            def poll(self): return self.returncode
            def kill(self):
                state["killed"] = True
                self.returncode = -1
            def wait(self, timeout=None): return self.returncode
        with patch.dict(os.environ, {"SystemRoot": "C:/Windows", "GITHUB_OUTPUT": "unsafe", "RUNNER_TRACKING_ID": "tracking-fixture"}, clear=True), \
             patch.object(V.subprocess, "CREATE_NEW_PROCESS_GROUP", 0, create=True), \
             patch.object(V.subprocess, "Popen", Process), \
             patch.object(V.subprocess, "run", side_effect=OSError("fixture") if taskkill_missing else None), \
             patch.object(V.time, "monotonic", side_effect=[0, V.TIMEOUT + 1] if timeout else [0, 1]), \
             contextlib.redirect_stdout(io.StringIO()) as displayed:
            try:
                result = V.run_harness(Path("fixture.exe"))
            except V.ValidationError:
                state["rejected"] = True
                result = None
        self.assertEqual(displayed.getvalue(), "")
        self.assertNotIn("GITHUB_OUTPUT", state["environment"])
        self.assertEqual(state["environment"]["RUNNER_TRACKING_ID"], "tracking-fixture")
        return result, state

    def test_success_and_nonzero_exit_are_distinguished(self):
        good, _ = self.run_fake()
        bad, _ = self.run_fake(exit_code=7)
        self.assertEqual(good["harness_exit_code"], 0)
        self.assertEqual(bad["harness_exit_code"], 7)

    def test_candidate_workflow_commands_and_forged_proofs_are_captured(self):
        payload = b'::warning::untrusted\nOP_AUTH_WINDOWS_RUNTIME_PROOF={"status":"success"}\n'
        result, _ = self.run_fake(output=payload)
        self.assertEqual(result["stdout_bytes"], len(payload))
        self.assertEqual(result["stdout_sha256"], V.hashlib.sha256(payload).hexdigest())
        self.assertNotIn("stdout", result)

    def test_timeout_kills_process_even_when_taskkill_is_unavailable(self):
        result, state = self.run_fake(running=True, timeout=True, taskkill_missing=True)
        self.assertIsNone(result)
        self.assertTrue(state["killed"] and state["rejected"])

    def test_output_limit_kills_process(self):
        result, state = self.run_fake(output=b"x" * (V.MAX_OUTPUT + 1), running=True)
        self.assertIsNone(result)
        self.assertTrue(state["killed"] and state["rejected"])


if __name__ == "__main__":
    unittest.main()
