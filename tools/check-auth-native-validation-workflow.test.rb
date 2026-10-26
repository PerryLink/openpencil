#!/usr/bin/env ruby
# Policy mutation tests; never run a candidate executable.
require 'yaml'
require 'tmpdir'
require 'fileutils'
require 'open3'
root = File.expand_path('..', __dir__)
caller_path = File.join(root, '.github/workflows/rust-release.yml')
child_path = File.join(root, '.github/workflows/auth-native-validation.yml')
checker = File.join(root, 'tools/check-auth-native-validation-workflow.rb')
caller_original = YAML.safe_load(File.read(caller_path), aliases: true)
child_original = YAML.safe_load(File.read(child_path), aliases: true)
mutations = {
  'caller-write' => ->(c, _) { c['jobs']['auth-native-validation']['permissions']['contents'] = 'write' },
  'caller-environment' => ->(c, _) { c['jobs']['auth-native-validation']['environment'] = 'release-production' },
  'caller-secrets' => ->(c, _) { c['jobs']['auth-native-validation']['secrets'] = 'inherit' },
  'caller-broad-if' => ->(c, _) { c['jobs']['auth-native-validation']['if'] = 'always()' },
  'caller-mutable-path' => ->(c, _) { c['jobs']['auth-native-validation']['uses'] = 'attacker/repo/test.yml@main' },
  'caller-wrong-digest' => ->(c, _) { c['jobs']['auth-native-validation']['with']['manifest_sha256'] = '${{ github.sha }}' },
  'publishing-not-skipped' => ->(c, _) { c['jobs']['version'].delete('if') },
  'child-write' => ->(_, c) { c['permissions']['contents'] = 'write' },
  'optional-digest' => ->(_, c) { c['on']['workflow_call']['inputs']['manifest_sha256']['required'] = false },
  'child-secret' => ->(_, c) { c['jobs']['validate']['env'] = {'TOKEN' => '${{ secrets.TOKEN }}'} },
  'child-environment' => ->(_, c) { c['jobs']['validate']['environment'] = 'release-production' },
  'wrong-runner' => ->(_, c) { c['jobs']['validate']['strategy']['matrix']['include'][0]['runner'] = 'windows-latest' },
  'missing-target' => ->(_, c) { c['jobs']['validate']['strategy']['matrix']['include'].pop },
  'credentials-persist' => ->(_, c) { c['jobs']['validate']['steps'][0]['with']['persist-credentials'] = true },
  'mutable-checkout' => ->(_, c) { c['jobs']['validate']['steps'][0]['uses'] = 'actions/checkout@v4' },
  'wrong-source' => ->(_, c) { c['jobs']['validate']['steps'][0]['with']['ref'] = 'main' },
  'private-source-build' => ->(_, c) { c['jobs']['validate']['steps'] << {'run' => 'cargo build --release'} },
  'artifact-upload' => ->(_, c) { c['jobs']['validate']['steps'] << {'uses' => 'actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02'} },
  'unchecked-execution' => ->(_, c) { c['jobs']['validate']['steps'][1]['run'] = './candidate.exe' },
}
_, baseline = Open3.capture2e('ruby', checker)
raise 'normal runtime verification contract failed' unless baseline.success?
Dir.mktmpdir('op-auth-runtime-policy-') do |directory|
  mutations.each do |name, mutation|
    caller = Marshal.load(Marshal.dump(caller_original))
    child = Marshal.load(Marshal.dump(child_original))
    mutation.call(caller, child)
    c = File.join(directory, 'caller.yml'); v = File.join(directory, 'child.yml')
    File.write(c, YAML.dump(caller)); File.write(v, YAML.dump(child))
    _, status = Open3.capture2e('ruby', checker, c, v)
    raise "policy mutation accepted: #{name}" if status.success?
  end
end
puts "check-auth-native-validation-workflow.test: #{mutations.size} rejected policy mutations passed."
