// Fingerprint the graph Cargo actually resolves for the two supported Agent
// targets, not the union of every workspace feature in Cargo.lock.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { resolve } from 'node:path';

assert.ok(process.env.RELEASE_PLEASE_TEST_ROOT, 'Set the existing Release Please engine directory');
const require = createRequire(resolve(process.env.RELEASE_PLEASE_TEST_ROOT, 'package.json'));
const engineRequire = createRequire(require.resolve('release-please'));
const toml = engineRequire('@iarna/toml');
const read = path => toml.parse(readFileSync(path, 'utf8'));

function sorted(value) {
  if (Array.isArray(value)) return value.map(sorted);
  if (value && typeof value === 'object') return Object.fromEntries(Object.keys(value).sort().map(key => [key, sorted(value[key])]));
  return value;
}

const targets = ['x86_64-unknown-linux-musl', 'aarch64-unknown-linux-musl'];
const graph = {}, packageNames = new Set(['senlun-agent', 'senlun-protocol']), selected = new Set();
for (const target of targets) {
  const output = execFileSync('cargo', ['tree', '--locked', '-p', 'senlun-agent', '--target', target,
    '--edges', 'normal,build', '--prefix', 'none', '--format', '{p}|{f}'],
  { encoding: 'utf8', maxBuffer: 8 * 1024 * 1024 });
  const lines = output.trim().split('\n').map(line => {
    const match = /^([^ ]+) v([^ |]+)/.exec(line);
    assert.ok(match, `Unexpected Cargo tree line: ${line}`);
    const [, name, version] = match;
    packageNames.add(name);
    selected.add(`${name}\0${version}`);
    return line.replace(/ \(\*\)$/, '').replace(/^(senlun-(?:agent|protocol)) v[^ |]+(?: \([^)]*\))?\|/, '$1 <workspace>|');
  });
  graph[target] = [...new Set(lines)].sort();
}

// Per-package profile overrides for Service-only crates cannot affect the
// Agent; global overrides and overrides of resolved dependencies can.
const root = read('Cargo.toml');
const profile = structuredClone(root.profile ?? {});
for (const settings of Object.values(profile)) {
  if (!settings.package) continue;
  settings.package = Object.fromEntries(Object.entries(settings.package).filter(([name]) => name === '*' || name.includes('/') || name.includes('#') || packageNames.has(name.split(/[:@]/)[0])));
  if (!Object.keys(settings.package).length) delete settings.package;
}
const buildFiles = Object.fromEntries(['scripts/build-linux-release.sh', 'scripts/Dockerfile.native-builder']
  .map(path => [path, createHash('sha256').update(readFileSync(path)).digest('hex')]));
if (existsSync('.cargo/config.toml')) buildFiles['.cargo/config.toml'] = createHash('sha256').update(readFileSync('.cargo/config.toml')).digest('hex');
const workspace = { ...root.workspace.package };
delete workspace.version;
const locked = read('Cargo.lock').package.filter(pkg => selected.has(`${pkg.name}\0${pkg.version}`)).map(pkg => ({
  name: pkg.name, version: ['senlun-agent', 'senlun-protocol'].includes(pkg.name) ? '<workspace>' : pkg.version,
  source: pkg.source ?? '', checksum: pkg.checksum ?? '',
})).sort((a, b) => JSON.stringify(a) < JSON.stringify(b) ? -1 : JSON.stringify(a) > JSON.stringify(b) ? 1 : 0);
const input = sorted({ graph, locked, workspace, profile, toolchain: read('rust-toolchain.toml').toolchain.channel, buildFiles });
const fingerprint = `sha256:${createHash('sha256').update(JSON.stringify(input)).digest('hex')}\n`;
const path = 'crates/senlun-agent/dependencies.sha256';
if (process.argv[2] === '--write') writeFileSync(path, fingerprint);
else {
  assert.equal(process.argv[2], '--check', 'Use --write or --check');
  assert.equal(readFileSync(path, 'utf8'), fingerprint, `Agent build inputs changed; update the marker to ${fingerprint.trim()}`);
}
