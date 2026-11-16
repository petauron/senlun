// Exercise the same release-please engine as the pinned release action without
// GitHub writes, source changes, builds, or manually chosen release versions.
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { resolve } from 'node:path';

assert.ok(process.env.RELEASE_PLEASE_TEST_ROOT, 'Set the isolated engine install directory');
const require = createRequire(resolve(process.env.RELEASE_PLEASE_TEST_ROOT, 'package.json'));
const engineRequire = createRequire(require.resolve('release-please'));
const { buildStrategy } = require('release-please/build/src/factory.js');
const { Version } = require('release-please/build/src/version.js');
const { TagName } = require('release-please/build/src/util/tag-name.js');
const toml = engineRequire('@iarna/toml');
const read = (path) => readFileSync(path, 'utf8');
const json = (path) => JSON.parse(read(path));

assert.equal(require('release-please/package.json').version, '17.6.0');
const actionPins = [...read('.github/workflows/release.yml').matchAll(/googleapis\/release-please-action@([a-f0-9]{40})/g)];
assert.equal(actionPins.length, 2);
for (const [, sha] of actionPins) {
  assert.equal(sha, '45996ed1f6d02564a971a2fa1b5860e934307cf7', 'Review the regression engine pin when upgrading the action');
}

const config = json('release-please-config.json').packages['.'];
assert.equal(config['release-type'], 'simple');
const version = json('.release-please-manifest.json')['.'];
const agentVersion = json('.release-please-manifest.json').crates;
const cargo = toml.parse(read('Cargo.toml'));
const lock = toml.parse(read('Cargo.lock'));
const memberPaths = cargo.workspace.members.map((path) => `${path}/Cargo.toml`);
const memberFiles = new Map(memberPaths.map((path) => [path, read(path)]));
const names = new Set([...memberFiles.values()].map((content) => {
  const pkg = toml.parse(content).package;
  if (pkg.name === 'senlun-agent') assert.equal(pkg.version, agentVersion);
  else assert.deepEqual(pkg.version, { workspace: true });
  return pkg.name;
}));
assert.equal(cargo.workspace.package.version, version);
assert.equal(read(config['version-file']).trim(), version);
assert.equal(json('web/package.json').version, version);
assert.equal(json('web/package-lock.json').version, version);
assert.equal(json('web/package-lock.json').packages[''].version, version);
assert.equal(lock.package.filter((pkg) => names.has(pkg.name)).length, names.size);
for (const pkg of lock.package.filter((pkg) => names.has(pkg.name))) {
  assert.equal(pkg.version, pkg.name === 'senlun-agent' ? agentVersion : version);
}
// License texts do not change when our own release number advances. Keep the
// workspace attributions, but do not bake its version into the license bundle.
const licenses = read('RUST_THIRD_PARTY_LICENSES.html');
for (const name of names) {
  assert.ok(licenses.includes(`>${name}</a>`), `Keep a version-independent attribution for ${name}`);
  assert.ok(!licenses.includes(`>${name} `), `Do not embed the product version for ${name}`);
}

const options = Object.fromEntries(Object.entries(config).map(([key, value]) => [
  key.replace(/-([a-z])/g, (_, letter) => letter.toUpperCase()), value,
]));
const strategy = await buildStrategy({
  ...options,
  path: '.',
  targetBranch: 'main',
  // No network methods are provided: generating this single-product candidate
  // must not require a GitHub client or the native Rust per-crate updater.
  github: { repository: { owner: 'example', repo: 'release-fixture', defaultBranch: 'main' } },
});
const candidate = await strategy.buildReleasePullRequest([{
  sha: 'a'.repeat(40),
  message: 'fix: exercise workspace release generation',
  bareMessage: 'exercise workspace release generation',
  type: 'fix', scope: null, notes: [], references: [], breaking: false,
}], { tag: new TagName(Version.parse(version)), sha: 'b'.repeat(40), notes: '' });
assert.ok(candidate, 'A releasable fix must generate a candidate');
const next = candidate.version.toString();
assert.notEqual(next, version);
const files = new Map();
for (const update of candidate.updates) {
  assert.ok(!memberFiles.has(update.path), 'Inherited crate manifests must not be rewritten');
  const original = files.get(update.path) ?? read(update.path);
  files.set(update.path, update.updater.updateContent(original));
}
assert.deepEqual([...files.keys()].sort(), [
  'CHANGELOG.md', 'Cargo.lock', 'Cargo.toml', 'version.txt',
  'web/package-lock.json', 'web/package.json',
].sort());
assert.equal(files.get('version.txt'), `${next}\n`);
assert.ok(files.get('CHANGELOG.md').includes(next));

const updatedCargo = toml.parse(files.get('Cargo.toml'));
assert.equal(updatedCargo.workspace.package.version, next);
updatedCargo.workspace.package.version = version;
assert.deepEqual(updatedCargo, cargo, 'Only the workspace version may change');
const updatedLock = toml.parse(files.get('Cargo.lock'));
for (const pkg of updatedLock.package.filter((pkg) => names.has(pkg.name))) {
  assert.equal(pkg.version, pkg.name === 'senlun-agent' ? agentVersion : next, `Lockfile version was not updated for ${pkg.name}`);
  pkg.version = pkg.name === 'senlun-agent' ? agentVersion : version;
}
assert.deepEqual(updatedLock, lock, 'Dependency versions and checksums must stay unchanged');
for (const path of ['web/package.json', 'web/package-lock.json']) {
  const updated = JSON.parse(files.get(path));
  assert.equal(updated.version, next);
  updated.version = version;
  if (path.endsWith('package-lock.json')) {
    assert.equal(updated.packages[''].version, next);
    updated.packages[''].version = version;
  }
  assert.deepEqual(updated, json(path), `Only product version fields may change in ${path}`);
}

// Keep the existing version-generation check covering the Agent as a separate
// component, including the shared Cargo lockfile's Agent entry.
const agentConfig = json('release-please-config.json').packages.crates;
assert.equal(agentConfig['release-type'], 'simple');
assert.equal(read('crates/senlun-agent/version.txt').trim(), agentVersion);
const agentOptions = Object.fromEntries(Object.entries(agentConfig).map(([key, value]) => [
  key.replace(/-([a-z])/g, (_, letter) => letter.toUpperCase()), value,
]));
const agentStrategy = await buildStrategy({
  ...agentOptions, path: 'crates', targetBranch: 'main',
  github: { repository: { owner: 'example', repo: 'release-fixture', defaultBranch: 'main' } },
});
const agentCandidate = await agentStrategy.buildReleasePullRequest([{
  sha: 'a'.repeat(40), message: 'fix: exercise agent release generation',
  bareMessage: 'exercise agent release generation', type: 'fix', scope: null,
  notes: [], references: [], breaking: false,
}], { tag: new TagName(Version.parse(agentVersion), 'senlun-agent'), sha: 'b'.repeat(40), notes: '' });
assert.ok(agentCandidate);
const nextAgent = agentCandidate.version.toString();
assert.notEqual(nextAgent, agentVersion);
const agentFiles = new Map();
for (const update of agentCandidate.updates) {
  const original = agentFiles.get(update.path) ?? (existsSync(update.path) ? read(update.path) : '');
  agentFiles.set(update.path, update.updater.updateContent(original));
}
assert.equal(agentFiles.get('crates/senlun-agent/version.txt'), `${nextAgent}\n`);
assert.equal(toml.parse(agentFiles.get('crates/senlun-agent/Cargo.toml')).package.version, nextAgent);
const agentLock = toml.parse(agentFiles.get('Cargo.lock'));
for (const pkg of agentLock.package.filter((pkg) => names.has(pkg.name))) {
  assert.equal(pkg.version, pkg.name === 'senlun-agent' ? nextAgent : version);
  pkg.version = pkg.name === 'senlun-agent' ? agentVersion : version;
}
assert.deepEqual(agentLock, lock, 'Agent version generation must preserve other dependencies');
console.log(`Release Please version regression passed: Service ${version} -> ${next}; Agent ${agentVersion} -> ${nextAgent}`);
