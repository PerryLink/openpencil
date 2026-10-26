#!/usr/bin/env python3
"""Validate and execute a locally cross-linked Auth harness on Windows.

This does not build private source, sign an archive, or upload an artifact.
Only the wrapper's JSON result is sent to the GitHub command channel.
"""
import argparse
import ctypes
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import struct
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parent.parent
INPUT_DIRECTORY = ".auth-validation/windows"
PREFIX = "OP_AUTH_WINDOWS_RUNTIME_PROOF="
TARGETS = {
    "aarch64-pc-windows-msvc": (0xAA64, "ARM64", 12),
    "x86_64-pc-windows-msvc": (0x8664, "X64", 9),
}
ABI_SYMBOLS = {
    "op_auth_abi_version", "op_auth_cancel", "op_auth_collab_relay_token_begin",
    "op_auth_collab_ticket_begin", "op_auth_collab_ticket_cancel", "op_auth_collab_ticket_poll",
    "op_auth_hosted_access_begin", "op_auth_hosted_access_cancel", "op_auth_hosted_access_poll",
    "op_auth_hosted_session_epoch", "op_auth_login_begin", "op_auth_poll", "op_auth_restore",
    "op_auth_runtime_init", "op_auth_sign_out", "op_auth_string_free",
}
MAX_FILE = 100 * 1024 * 1024
MAX_OUTPUT = 1024 * 1024
TIMEOUT = 120


class ValidationError(Exception):
    pass


def require(condition, reason):
    if not condition:
        raise ValidationError(reason)


def plain_path(path, directory=False):
    info = path.lstat()
    require(not path.is_symlink() and not (getattr(info, "st_file_attributes", 0) & 0x400),
            "symlink or reparse point is forbidden")
    require(path.is_dir() if directory else path.is_file(), "unexpected path type")
    if not directory:
        require(0 < info.st_size <= MAX_FILE, "input file size is outside the reviewed bound")
    return path


def digest(path):
    value = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            value.update(chunk)
    return value.hexdigest()


def hex_value(value, length):
    return isinstance(value, str) and re.fullmatch(r"[0-9a-f]{" + str(length) + r"}", value) is not None


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, "duplicate JSON key")
        result[key] = value
    return result


def validate_input_location(root, repository=ROOT):
    expected = repository / INPUT_DIRECTORY
    require(root.absolute() == expected.absolute(), "input root must be the reviewed checkout directory")
    plain_path(repository / ".auth-validation", directory=True)
    plain_path(root, directory=True)
    require(root.resolve().parent.parent == repository.resolve(), "input directory escapes the checkout")


def load_inputs(root, expected):
    require(hex_value(expected, 64), "manifest digest must be reviewed lowercase SHA-256")
    plain_path(root, directory=True)
    path = plain_path(root / "manifest.json")
    require(path.stat().st_size <= 65536, "manifest is oversized")
    require(digest(path) == expected, "manifest digest mismatch")
    data = json.loads(path.read_bytes().decode("utf-8"), object_pairs_hook=unique_object)
    require(isinstance(data, dict), "manifest must be a JSON object")
    require(set(data) == {"schema", "abi", "private_source_sha", "public_source_sha",
                          "harness_source_sha256", "targets"}, "manifest fields are not canonical")
    require(data["schema"] == "op-auth-windows-runtime-input-v1" and type(data["abi"]) is int
            and data["abi"] == 4, "only the reviewed ABI 4 input schema is supported")
    for name, length in [("private_source_sha", 40), ("public_source_sha", 40), ("harness_source_sha256", 64)]:
        require(hex_value(data[name], length), "malformed source binding")
    require(isinstance(data["targets"], list) and len(data["targets"]) == 2, "both Windows targets are required")
    require(all(isinstance(entry, dict) for entry in data["targets"]), "target entries must be objects")
    require([entry.get("target") for entry in data["targets"]] == sorted(TARGETS), "target set or order mismatch")
    require({p.name for p in root.iterdir()} == {"manifest.json", *TARGETS}, "extra or missing input root entries")
    entries = {}
    for entry in data["targets"]:
        require(set(entry) == {"target", "archive", "archive_sha256", "harness", "harness_sha256"}, "target fields mismatch")
        target = entry["target"]
        directory = plain_path(root / target, directory=True)
        require({p.name for p in directory.iterdir()} == {"op_auth.lib", "FINAL-LINK-BINARY.exe"}, "target file set mismatch")
        for name, filename in [("archive", "op_auth.lib"), ("harness", "FINAL-LINK-BINARY.exe")]:
            require(entry[name] == target + "/" + filename, "non-canonical candidate path")
            require(hex_value(entry[name + "_sha256"], 64), "malformed candidate digest")
            file = plain_path(directory / filename)
            require(digest(file) == entry[name + "_sha256"], "candidate digest mismatch")
        entries[target] = entry
    return data, entries


def inspect_pe(path, machine):
    with path.open("rb") as stream:
        header = stream.read(64)
        require(len(header) == 64 and header[:2] == b"MZ", "harness is not a PE executable")
        offset = struct.unpack_from("<I", header, 60)[0]
        require(64 <= offset <= path.stat().st_size - 48, "invalid PE header offset")
        stream.seek(offset)
        pe = stream.read(48)
    require(pe[:4] == b"PE\0\0", "invalid PE signature")
    actual, sections = struct.unpack_from("<HH", pe, 4)
    optional_size, flags = struct.unpack_from("<HH", pe, 20)
    require(actual == machine and 1 <= sections <= 96, "PE architecture or sections mismatch")
    require(optional_size >= 112 and struct.unpack_from("<H", pe, 24)[0] == 0x20B, "PE must use PE32+")
    require(flags & 0x0002 and not flags & 0x2000, "harness must be an executable, not a DLL")


def inspect_archive(path, machine):
    symbols = set()
    object_count = 0
    with path.open("rb") as stream:
        require(stream.read(8) == b"!<arch>\n", "candidate is not a COFF archive")
        member_index = 0
        while True:
            header = stream.read(60)
            if not header:
                break
            require(len(header) == 60 and header[58:] == b"`\n", "invalid archive member header")
            name = header[:16].decode("ascii").strip()
            require(re.fullmatch(r"[0-9 ]+", header[48:58].decode("ascii")) is not None, "invalid archive member size")
            size = int(header[48:58])
            require(0 < size <= MAX_FILE, "invalid archive member size")
            payload = stream.read(size)
            require(len(payload) == size, "truncated archive member")
            if size & 1:
                require(stream.read(1) == b"\n", "invalid archive padding")
            if name == "/":
                # The first MSVC linker member contains a big-endian count and offsets.
                if member_index == 0:
                    require(size >= 4, "truncated linker table")
                    count = struct.unpack_from(">I", payload)[0]
                    start = 4 + 4 * count
                    require(count <= 1000000 and start <= size, "invalid linker symbol table")
                    names = payload[start:].split(b"\0")
                    require(len(names) >= count, "truncated linker symbol names")
                    symbols.update(names[:count])
            elif name != "//":
                require(size >= 20, "truncated COFF object")
                actual = struct.unpack_from("<H", payload)[0]
                if payload[:4] == b"\0\0\xff\xff":
                    actual = struct.unpack_from("<H", payload, 6)[0]
                require(actual == machine, "COFF member architecture mismatch")
                object_count += 1
            member_index += 1
            require(member_index <= 100000, "too many archive members")
    require(object_count > 0 and {name.encode("ascii") for name in ABI_SYMBOLS} <= symbols, "COFF archive is missing the 16 required C ABI symbols")
    return object_count


def native_windows_architecture():
    require(os.name == "nt", "runtime verification must execute on Windows")
    class Processor(ctypes.Structure):
        _fields_ = [("architecture", ctypes.c_ushort), ("reserved", ctypes.c_ushort)]
    class ProcessorUnion(ctypes.Union):
        _fields_ = [("oem", ctypes.c_ulong), ("processor", Processor)]
    class SystemInfo(ctypes.Structure):
        _fields_ = [("u", ProcessorUnion), ("page_size", ctypes.c_ulong),
                    ("minimum", ctypes.c_void_p), ("maximum", ctypes.c_void_p),
                    ("mask", ctypes.c_size_t), ("processors", ctypes.c_ulong),
                    ("processor_type", ctypes.c_ulong), ("granularity", ctypes.c_ulong),
                    ("level", ctypes.c_ushort), ("revision", ctypes.c_ushort)]
    value = SystemInfo()
    ctypes.windll.kernel32.GetNativeSystemInfo(ctypes.byref(value))
    return value.u.processor.architecture


def context(target, environment):
    require(environment.get("GITHUB_REPOSITORY") == "ZSeven-W/openpencil", "unexpected verification repository")
    require(environment.get("GITHUB_EVENT_NAME") == "workflow_dispatch", "only explicit dispatch may verify candidates")
    require(environment.get("RUNNER_OS") == "Windows" and environment.get("RUNNER_ARCH") == TARGETS[target][1], "runner architecture mismatch")
    require(hex_value(environment.get("GITHUB_SHA"), 40), "invalid workflow commit")
    workflow_ref = environment.get("GITHUB_WORKFLOW_REF", "")
    require(workflow_ref.startswith("ZSeven-W/openpencil/.github/workflows/rust-release.yml@refs/heads/"), "unexpected workflow identity")
    for name in ["GITHUB_RUN_ID", "GITHUB_RUN_ATTEMPT"]:
        require(re.fullmatch(r"[1-9][0-9]{0,19}", environment.get(name, "")) is not None, "invalid run identity")
    require(re.fullmatch(r"[A-Za-z0-9_-]{1,100}", environment.get("GITHUB_JOB", "")) is not None, "invalid job identifier")
    require(re.fullmatch(r"[A-Za-z0-9._ -]{1,200}", environment.get("RUNNER_NAME", "")) is not None, "invalid runner identifier")
    return {"github_job": environment["GITHUB_JOB"], "runner_name": environment["RUNNER_NAME"],
            "runner_os": environment["RUNNER_OS"], "repository": environment["GITHUB_REPOSITORY"], "workflow_head_sha": environment["GITHUB_SHA"],
            "workflow_ref": workflow_ref, "run_id": int(environment["GITHUB_RUN_ID"]),
            "run_attempt": int(environment["GITHUB_RUN_ATTEMPT"]), "runner_arch": environment["RUNNER_ARCH"]}


def child_environment(directory):
    # Do not pass GitHub command files, tokens, checkout credentials or user profiles.
    required = ["SystemRoot", "WINDIR", "SystemDrive", "COMSPEC", "PATHEXT", "RUNNER_TRACKING_ID"]
    result = {name: os.environ[name] for name in required if name in os.environ}
    require("SystemRoot" in result, "Windows system directory is unavailable")
    result.update({"TEMP": str(directory), "TMP": str(directory),
                   "PATH": str(Path(result["SystemRoot"]) / "System32")})
    return result


def run_harness(executable):
    with tempfile.TemporaryDirectory(prefix="op-auth-runtime-") as directory:
        directory = Path(directory)
        out_path, err_path = directory / "stdout", directory / "stderr"
        with out_path.open("wb") as out, err_path.open("wb") as err:
            process = subprocess.Popen([str(executable)], cwd=directory,
                                       env=child_environment(directory), stdin=subprocess.DEVNULL,
                                       stdout=out, stderr=err, creationflags=subprocess.CREATE_NEW_PROCESS_GROUP)
            start = time.monotonic()
            try:
                while process.poll() is None:
                    require(time.monotonic() - start < TIMEOUT, "harness runtime exceeded its limit")
                    require(out_path.stat().st_size <= MAX_OUTPUT and err_path.stat().st_size <= MAX_OUTPUT, "harness output exceeded its limit")
                    time.sleep(0.1)
            finally:
                if process.poll() is None:
                    try:
                        subprocess.run([str(Path(os.environ["SystemRoot"]) / "System32/taskkill.exe"), "/PID", str(process.pid), "/T", "/F"],
                                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False)
                    except OSError:
                        pass
                    finally:
                        process.kill()
                process.wait(timeout=10)
        require(out_path.stat().st_size <= MAX_OUTPUT and err_path.stat().st_size <= MAX_OUTPUT, "harness output exceeded its limit")
        result = {"harness_exit_code": process.returncode, "stdout_sha256": digest(out_path),
                  "stderr_sha256": digest(err_path), "stdout_bytes": out_path.stat().st_size,
                  "stderr_bytes": err_path.stat().st_size}
        # Only hashes of candidate-controlled output enter the proof/command channel.
        return result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--input-root", type=Path, default=ROOT / INPUT_DIRECTORY)
    parser.add_argument("--expected-manifest-sha256", required=True)
    parser.add_argument("--target", choices=sorted(TARGETS), required=True)
    args = parser.parse_args()
    result = {"schema": "op-auth-windows-runtime-proof-v1", "status": "failed", "target": args.target,
              "abi": 4, "validation": "cross-link-native-run", "link_validation": "cross-final-link"}
    try:
        validate_input_location(args.input_root)
        result.update(context(args.target, os.environ))
        actual_arch = native_windows_architecture()
        require(actual_arch == TARGETS[args.target][2], "actual Windows OS architecture mismatch")
        result["os_native_architecture"] = TARGETS[args.target][1]
        result["pe_machine"] = TARGETS[args.target][0]
        current = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
        require(current == result["workflow_head_sha"], "checkout differs from workflow commit")
        data, entries = load_inputs(args.input_root, args.expected_manifest_sha256)
        entry = entries[args.target]
        archive, harness = args.input_root / entry["archive"], args.input_root / entry["harness"]
        count = inspect_archive(archive, TARGETS[args.target][0])
        inspect_pe(harness, TARGETS[args.target][0])
        protected = [Path(__file__).resolve(), ROOT / ".github/workflows/auth-native-validation.yml", ROOT / ".github/workflows/rust-release.yml"]
        hashes = [digest(path) for path in protected]
        result.update({name: data[name] for name in ["private_source_sha", "public_source_sha", "harness_source_sha256"]})
        result.update({"input_manifest_sha256": args.expected_manifest_sha256,
                       "archive_sha256": entry["archive_sha256"], "harness_sha256": entry["harness_sha256"],
                       "helper_sha256": hashes[0], "verification_workflow_sha256": hashes[1],
                       "caller_workflow_sha256": hashes[2], "coff_object_count": count,
                       "required_abi_symbols": len(ABI_SYMBOLS),
                       "archive_inspection": "coff-machine-and-linker-index"})
        result.update(run_harness(harness))
        require(result["harness_exit_code"] == 0, "ABI harness failed native execution")
        validate_input_location(args.input_root)
        load_inputs(args.input_root, args.expected_manifest_sha256)
        require([digest(path) for path in protected] == hashes, "verification source changed during execution")
        result.update({"status": "success", "execution": "passed"})
    except (ValidationError, OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        result["error"] = str(error)[:500]
    result["completed_at_utc"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    encoded = json.dumps(result, sort_keys=True, separators=(",", ":"), ensure_ascii=True)
    print(PREFIX + encoded, flush=True)
    if os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(os.environ["GITHUB_STEP_SUMMARY"], "a", encoding="utf-8") as summary:
            summary.write("\n```json\n" + encoded + "\n```\n")
    return 0 if result["status"] == "success" else 1


if __name__ == "__main__":
    raise SystemExit(main())
