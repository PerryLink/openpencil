#!/usr/bin/env ruby
# Exact, secret-free contract for runtime-only Windows Auth verification.
require 'yaml'
root = File.expand_path('..', __dir__)
caller_path = ARGV[0] || File.join(root, '.github/workflows/rust-release.yml')
child_path = ARGV[1] || File.join(root, '.github/workflows/auth-native-validation.yml')
helper_path = ARGV[2] || File.join(root, 'tools/validate-local-windows-auth.py')
[caller_path, child_path, helper_path].each do |path|
  raise "missing regular verification source: #{path}" unless File.file?(path) && !File.symlink?(path)
end
caller = YAML.safe_load(File.read(caller_path), aliases: true)
child = YAML.safe_load(File.read(child_path), aliases: true)
def demand(value, message)
  raise message unless value
end
jobs = caller.fetch('jobs')
demand(jobs.fetch('version').fetch('if') == "github.event_name != 'workflow_dispatch' || inputs.auth_validation_only != true", 'validation mode must skip the publishing prerequisite')
expected_call = {
  'if' => "github.event_name == 'workflow_dispatch' && inputs.auth_validation_only == true",
  'permissions' => {'contents' => 'read'},
  'uses' => './.github/workflows/auth-native-validation.yml',
  'with' => {'manifest_sha256' => '${{ inputs.auth_validation_manifest_sha256 }}'},
}
demand(jobs.fetch('auth-native-validation') == expected_call, 'runtime verification caller must remain exact and unprivileged')
trigger = child['on'] || child[true]
demand(trigger == {'workflow_call' => {'inputs' => {'manifest_sha256' => {
  'description' => 'SHA-256 of the reviewed local candidate manifest',
  'required' => true, 'type' => 'string'
}}}}, 'reusable verification trigger must bind the reviewed digest')
demand(child.fetch('permissions') == {'contents' => 'read'}, 'verification cannot receive write permissions')
demand(child.keys.sort == ['name', 'on', 'permissions', 'jobs'].sort, 'verification workflow surface expanded')
demand(!child.key?('env') && !child.key?('defaults'), 'verification must not receive broad environment/defaults')
demand(child.fetch('jobs').keys == ['validate'], 'unexpected verification jobs')
job = child.fetch('jobs').fetch('validate')
demand(job.keys.sort == %w[name runs-on timeout-minutes permissions strategy steps].sort, 'verification job surface expanded')
demand(job['name'] == 'Runtime validation (${{ matrix.target }})', 'verification job title must bind target')
demand(job['runs-on'] == '${{ matrix.runner }}' && job['timeout-minutes'] == 5, 'verification runner or timeout changed')
demand(job['permissions'] == {'contents' => 'read'}, 'verification job must be read-only')
demand(job['strategy'] == {'fail-fast' => false, 'matrix' => {'include' => [
  {'target' => 'aarch64-pc-windows-msvc', 'runner' => 'windows-11-arm'},
  {'target' => 'x86_64-pc-windows-msvc', 'runner' => 'windows-latest'}
]}}, 'both matching Windows runners are required')
steps = job.fetch('steps')
demand(steps.size == 2, 'runtime verification must not build, sign, download or upload')
checkout = steps.fetch(0)
demand(checkout.keys.sort == %w[name uses with].sort, 'checkout surface expanded')
demand(checkout['uses'] == 'actions/checkout@08eba0b27e820071cde6df949e0beb9ba4906955', 'checkout action must remain pinned')
demand(checkout['with'] == {'fetch-depth' => 1, 'persist-credentials' => false, 'ref' => '${{ github.sha }}'}, 'checkout must use exact immutable run inputs without credentials')
run = steps.fetch(1)
demand(run.keys.sort == %w[name shell env run].sort && run['shell'] == 'pwsh', 'runtime step surface changed')
demand(run['env'] == {'EXPECTED_MANIFEST_SHA256' => '${{ inputs.manifest_sha256 }}', 'VALIDATION_TARGET' => '${{ matrix.target }}'}, 'runtime step may receive only non-secret manifest/target inputs')
expected = <<~'POWERSHELL'
  $ErrorActionPreference = 'Stop'
  python tools/validate-local-windows-auth.py `
    --expected-manifest-sha256 "$env:EXPECTED_MANIFEST_SHA256" `
    --target "$env:VALIDATION_TARGET"
  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
POWERSHELL
demand(run['run'] == expected, 'runtime step must call the reviewed verifier only')
text = File.read(child_path)
demand(!text.match?(/\bsecrets\.|secrets:|upload-artifact|download-artifact|cargo\s|rustc\s|msbuild\s|contents:\s*write|id-token:/i), 'verification must not receive secrets, source builds or artifact-storage actions')
helper = File.read(helper_path)
[
  'op-auth-windows-runtime-input-v1', 'op-auth-windows-runtime-proof-v1',
  'OP_AUTH_WINDOWS_RUNTIME_PROOF=', 'cross-link-native-run',
  'GetNativeSystemInfo', 'harness_source_sha256', 'input_manifest_sha256',
  'caller_workflow_sha256', 'verification_workflow_sha256',
  'candidate digest mismatch', 'verification source changed during execution',
  'stdout_sha256', 'stderr_sha256', 'child_environment(directory)',
].each { |literal| demand(helper.include?(literal), "missing verifier binding: #{literal}") }
demand(!helper.include?('shell=True'), 'harness must not execute through a shell')
demand(!helper.match?(/print\([^\n]*(?:stdout|stderr)/), 'candidate-controlled output must not be streamed to workflow commands')
puts 'check-auth-native-validation-workflow: read-only runtime validation contracts passed.'
