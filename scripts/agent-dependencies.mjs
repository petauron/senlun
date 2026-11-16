// Record only the locked dependency closure and inherited build settings of
// the Agent. The TOML parser is already bundled with the pinned Release Please
// engine used by the existing CI version check; no project dependency is added.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { resolve } from 'node:path';

assert.ok(process.env.RELEASE_PLEASE_TEST_ROOT, 'Set the existing Release Please engine directory');
const require = createRequire(resolve(process.env.RELEASE_PLEASE_TEST_ROOT, 'package.json'));
const engineRequire = createRequire(require.resolve('release-please'));
const toml = engineRequire('@iarna/toml');
const read = path => toml.parse(readFileSync(path, 'utf8'));
const workspace = read('Cargo.toml').workspace;
const manifests = ['crates/senlun-agent/Cargo.toml', 'crates/senlun-protocol/Cargo.toml'].map(read);
const lock = read('Cargo.lock');
const local = new Set(['senlun-agent', 'senlun-protocol', 'senlun-service']);

function sorted(value) {
  if (Array.isArray(value)) return value.map(sorted);
  if (value && typeof value === 'object') return Object.fromEntries(Object.keys(value).sort().map(key => [key, sorted(value[key])]));
  return value;
}

const inherited = new Set();
for (const manifest of manifests) {
  const scopes = [manifest, ...Object.values(manifest.target ?? {})];
  for (const scope of scopes) for (const kind of ['dependencies', 'build-dependencies']) {
    for (const [name, specification] of Object.entries(scope[kind] ?? {})) {
      if (specification && typeof specification === 'object' && specification.workspace === true) inherited.add(name);
    }
  }
}
const workspaceDependencies = {};
for (const name of [...inherited].sort()) {
  assert.ok(Object.hasOwn(workspace.dependencies, name), `Missing inherited dependency ${name}`);
  workspaceDependencies[name] = workspace.dependencies[name];
}

const byName = new Map();
for (const pkg of lock.package) {
  const existing = byName.get(pkg.name) ?? [];
  existing.push(pkg);
  byName.set(pkg.name, existing);
}
function resolveDependency(reference) {
  const parsed = /^(\S+)(?:\s+(\S+))?(?:\s+\(([^)]+)\))?$/.exec(reference);
  assert.ok(parsed, `Unrecognized locked dependency ${reference}`);
  const [, name, version, source] = parsed;
  const matches = (byName.get(name) ?? []).filter(pkg => (!version || pkg.version === version) && (!source || pkg.source === source));
  assert.equal(matches.length, 1, `Ambiguous locked dependency ${reference}`);
  return matches[0];
}
const agent = byName.get('senlun-agent') ?? [];
assert.equal(agent.length, 1, 'Expected one locked Agent package');
const pending = [...agent], seen = new Set(), packages = [];
while (pending.length) {
  const pkg = pending.pop();
  const identity = `${pkg.name}\0${pkg.version}\0${pkg.source ?? ''}`;
  if (seen.has(identity)) continue;
  seen.add(identity);
  packages.push({ name: pkg.name, version: local.has(pkg.name) ? '<workspace>' : pkg.version,
    source: pkg.source ?? '', checksum: pkg.checksum ?? '' });
  for (const dependency of pkg.dependencies ?? []) pending.push(resolveDependency(dependency));
}
packages.sort((a, b) => JSON.stringify(a) < JSON.stringify(b) ? -1 : JSON.stringify(a) > JSON.stringify(b) ? 1 : 0);
const root = read('Cargo.toml');
const buildFiles = Object.fromEntries(['rust-toolchain.toml', 'scripts/build-linux-release.sh', 'scripts/Dockerfile.native-builder']
  .map(path => [path, createHash('sha256').update(readFileSync(path)).digest('hex')]));
const inputs = sorted({ packages, workspaceDependencies,
  edition: workspace.package.edition, rustVersion: workspace.package['rust-version'],
  profile: root.profile, patch: root.patch, replace: root.replace, buildFiles });
const fingerprint = `sha256:${createHash('sha256').update(JSON.stringify(inputs)).digest('hex')}\n`;
const path = 'crates/senlun-agent/dependencies.sha256';
if (process.argv[2] === '--write') writeFileSync(path, fingerprint);
else {
  assert.equal(process.argv[2], '--check', 'Use --write or --check');
  assert.equal(readFileSync(path, 'utf8'), fingerprint, 'Agent dependencies changed; run node scripts/agent-dependencies.mjs --write and include the result');
}
