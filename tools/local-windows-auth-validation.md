# Runtime validation of locally built Windows Auth candidates

This lane verifies locally cross-linked ABI 4 harnesses on matching Windows
runners. It does not compile private source, sign a release, change adoption
policy, or use Actions artifact storage. Its result is explicitly
`cross-link-native-run`, not a claim that linking happened on Windows.

A reviewed temporary verification branch contains exactly these candidate files:

```text
.auth-validation/windows/manifest.json
.auth-validation/windows/aarch64-pc-windows-msvc/op_auth.lib
.auth-validation/windows/aarch64-pc-windows-msvc/FINAL-LINK-BINARY.exe
.auth-validation/windows/x86_64-pc-windows-msvc/op_auth.lib
.auth-validation/windows/x86_64-pc-windows-msvc/FINAL-LINK-BINARY.exe
```

Do not add private source, keys, passwords, or unrelated files. Each binary must
be below 100 MiB and must already have passed the local source-bound archive and
cross-link audits. The manifest is reviewed locally before its digest is supplied
to the dispatch; a digest supplied by an untrusted artifact is not authorization.

The JSON manifest has exactly these fields:

- `schema`: `op-auth-windows-runtime-input-v1`
- `abi`: integer `4`
- `private_source_sha`: full lowercase private source commit SHA
- `public_source_sha`: full lowercase public source/context commit SHA
- `harness_source_sha256`: SHA-256 of the exact private harness source used locally
- `targets`: two entries in the target order shown above. Each entry has exactly
  `target`, `archive`, `archive_sha256`, `harness`, and `harness_sha256`. Paths are
  the fixed target-relative paths shown above; hashes are lowercase SHA-256.

Dispatch the existing `rust-release.yml` workflow from the reviewed verification
branch with `auth_validation_only=true` and
`auth_validation_manifest_sha256=<reviewed manifest digest>`. The publishing
version prerequisite is skipped in this mode, so normal release jobs cannot run.
The default remains the existing release behavior.

The reusable jobs have `contents: read`, no protected environment, no secrets,
and no credential persistence. The runner verifies both targets' input hashes,
the selected PE/COFF architecture and required C ABI symbol index, and the actual
Windows OS architecture. It executes the selected prelinked harness with a
scrubbed environment, fixed temporary working directory, 120-second timeout and
bounded captured output. Candidate stdout/stderr are not forwarded to GitHub's
workflow-command channel. All inputs and verifier source are rehashed afterward.

Each job emits exactly one wrapper-owned line starting with
`OP_AUTH_WINDOWS_RUNTIME_PROOF=` followed by JSON. A successful result binds the
manifest, library, harness, private/public source, helper/workflow hashes,
workflow head commit, run ID, attempt and target, and records actual execution.
The numeric GitHub job ID is obtained independently from the GitHub job API; bind
that exact job and its logs using the target job name and reported runner name.
A failed result exits nonzero and is never acceptance evidence.

The consuming release controller must independently check the GitHub API run and
job conclusions, repository, event, head commit and run attempt. Require exactly
one successful proof for each target and compare all source/digest/workflow
bindings with the reviewed local inputs. Do not accept a copied log line, a green
workflow alone, or self-declared source fields. Keep the local cross-build receipt
and this runtime receipt together. Only the separately reviewed complete-matrix
signer may decide whether the accumulated evidence permits production adoption.
