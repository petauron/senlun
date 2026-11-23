// Select the CI jobs a change can affect. The classifier fails closed: unknown
// paths, workflow or classifier edits, and unverifiable version bumps select the
// complete suite. A version-only Release Please change is proven by content, not
// by path, before it may skip code builds.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { appendFileSync } from 'node:fs';
import process from 'node:process';
import { pathToFileURL } from 'node:url';
import { isDeepStrictEqual } from 'node:util';

export const CODE_JOBS = ['web', 'rust', 'security', 'e2e', 'container', 'source_docker'];
export const JOBS = [...CODE_JOBS, 'version'];
const RUST_BUILD = ['rust', 'e2e', 'container'];
const WORKSPACE_CRATES = new Set(['senlun-agent', 'senlun-protocol', 'senlun-service']);
const FULL = Symbol('full');

// Release Please may touch only these files in a version-only change.
const VERSION_FILES = new Set([
  '.release-please-manifest.json',
  'version.txt',
  'crates/senlun-agent/version.txt',
  'CHANGELOG.md',
  'crates/senlun-agent/CHANGELOG.md',
  'Cargo.toml',
  'crates/senlun-agent/Cargo.toml',
  'Cargo.lock',
  'web/package.json',
  'web/package-lock.json',
]);

const exact = (...paths) => path => paths.includes(path);
const prefix = (...prefixes) => path => prefixes.some(value => path.startsWith(value));

// First match wins. `jobs` may be FULL; `crate` narrows Rust checks.
const RULES = [
  { match: prefix('.github/workflows/'), jobs: FULL },
  { match: exact('.github/dependabot.yml', 'scripts/ci-scope.mjs', 'scripts/ci-scope.test.mjs', '.gitignore'), jobs: FULL },
  // Fast repository checks for these files run unconditionally in `changes`.
  { match: exact('.env.example', 'scripts/brand-names.test.mjs'), jobs: [] },
  { match: prefix('deploy/'), jobs: [] },
  // Release-source tests need the Web toolchain's YAML parser.
  { match: exact('scripts/release-source.mjs', 'scripts/release-source.test.mjs'), jobs: ['web'] },
  // The Service compiles and serves the bundled theme package in its tests.
  { match: path => /^compat\/luminaplus\/.+\.zip$/.test(path), jobs: RUST_BUILD, crate: 'senlun-service' },
  // Release-only packaging and manual tools have no CI-executable coverage.
  { match: exact('scripts/publish-agent-draft.sh', 'scripts/benchmark-storage.py', '.editorconfig'), jobs: [] },
  { match: prefix('compat/'), jobs: [] },
  { match: exact('web/UPSTREAM.md', 'web/THIRD_PARTY_NOTICES.md'), jobs: ['web'] },
  { match: path => /^web\/LICENSE\./.test(path), jobs: ['web'] },
  { match: prefix('docs/', '.github/ISSUE_TEMPLATE/'), jobs: [] },
  { match: exact('.github/PULL_REQUEST_TEMPLATE.md'), jobs: [] },
  { match: path => path.endsWith('.md') && !/THIRD_PARTY|NOTICE|LICENSE/i.test(path), jobs: [] },
  { match: exact('.release-please-manifest.json', 'version.txt', 'release-please-config.json', 'crates/senlun-agent/version.txt'), jobs: ['rust'] },
  { match: exact('Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml'), jobs: [...RUST_BUILD, 'security', 'source_docker'], crate: 'workspace' },
  { match: exact('rustfmt.toml'), jobs: ['rust'] },
  // check-release-please.mjs also asserts the bundle keeps version-free attributions.
  { match: exact('deny.toml', 'about.toml', 'about.hbs', 'RUST_THIRD_PARTY_LICENSES.html', 'RUST_STDLIB_LICENSES.html', 'scripts/rust-licenses.mjs'), jobs: ['security', 'rust'] },
  { match: exact('crates/senlun-agent/dependencies.sha256', 'scripts/check-release-please.mjs', 'scripts/agent-dependencies.mjs'), jobs: ['rust'] },
  { match: path => /^crates\/senlun-(agent|protocol|service)\/Cargo\.toml$/.test(path), jobs: [...RUST_BUILD, 'security', 'source_docker'], crate: 'path' },
  { match: path => /^crates\/senlun-(agent|protocol|service)\//.test(path), jobs: RUST_BUILD, crate: 'path' },
  { match: exact('web/package.json', 'web/package-lock.json'), jobs: ['web', 'e2e', 'security', 'source_docker', 'rust', 'container'] },
  { match: prefix('web/e2e/'), jobs: ['web', 'e2e'] },
  { match: prefix('web/'), jobs: ['web', 'e2e'] },
  { match: exact('scripts/build-linux-release.sh', 'scripts/build-linux-service.sh', 'scripts/verify-linux-service.sh', 'scripts/Dockerfile.native-builder'), jobs: ['container', 'rust'] },
  { match: exact('Dockerfile.release'), jobs: ['container'] },
  { match: exact('LICENSE', 'NOTICE'), jobs: ['container', 'source_docker'] },
  { match: exact('Dockerfile', '.dockerignore'), jobs: ['source_docker'] },
];

function crateOf(path) {
  return /^crates\/(senlun-(?:agent|protocol|service))\//.exec(path)?.[1];
}

export function rustPackages(crates) {
  if (crates.size === 1) {
    const [only] = crates;
    if (only === 'senlun-agent' || only === 'senlun-service') return `-p ${only}`;
  }
  return '--workspace';
}

function fullScope(reason) {
  return { jobs: new Set(CODE_JOBS), rustPackages: '--workspace', reason };
}

// Classify ordinary changes by path. Returns the selected job set.
export function classifyPaths(paths) {
  const jobs = new Set();
  const crates = new Set();
  for (const path of paths) {
    const rule = RULES.find(candidate => candidate.match(path));
    if (!rule || rule.jobs === FULL) return fullScope(rule ? `full suite required by ${path}` : `unclassified path ${path}`);
    for (const job of rule.jobs) jobs.add(job);
    if (rule.crate === 'workspace') crates.add('workspace');
    else if (rule.crate === 'path') crates.add(crateOf(path));
    else if (rule.crate) crates.add(rule.crate);
  }
  const reason = jobs.size === 0 ? 'no CI-executable changes' : `change-scoped: ${[...jobs].sort().join(', ')}`;
  return { jobs, rustPackages: rustPackages(crates), reason };
}

const versionPattern = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/;

const versionLine = /^version = "[^"]+"\r?$/;
const nameLine = /^name = "([^"]+)"\r?$/m;

function sameLinesExceptVersion(before, after) {
  const left = before.split('\n');
  const right = after.split('\n');
  if (left.length !== right.length) return false;
  return left.every((line, index) => line === right[index]
    || (versionLine.test(line) && versionLine.test(right[index])));
}

function lockBlocks(text) {
  return text.split(/\n(?=\[\[package\]\]\r?\n)/);
}

function sameLockExceptWorkspaceVersions(before, after) {
  const left = lockBlocks(before);
  const right = lockBlocks(after);
  if (left.length !== right.length) return false;
  return left.every((block, index) => {
    if (block === right[index]) return true;
    const name = nameLine.exec(block)?.[1];
    return name !== undefined
      && WORKSPACE_CRATES.has(name)
      && nameLine.exec(right[index])?.[1] === name
      && sameLinesExceptVersion(block, right[index]);
  });
}

function sameJsonExcept(before, after, strip) {
  try {
    const left = JSON.parse(before);
    const right = JSON.parse(after);
    strip(left);
    strip(right);
    return isDeepStrictEqual(left, right);
  }
  catch {
    return false;
  }
}

const VERSION_CHECKS = {
  '.release-please-manifest.json': (before, after) => {
    try {
      const left = JSON.parse(before);
      const right = JSON.parse(after);
      return isDeepStrictEqual(Object.keys(left).sort(), Object.keys(right).sort())
        && Object.values(right).every(value => typeof value === 'string' && versionPattern.test(value));
    }
    catch {
      return false;
    }
  },
  'version.txt': (before, after) => before !== null && versionPattern.test(after.trim()),
  'crates/senlun-agent/version.txt': (before, after) => before !== null && versionPattern.test(after.trim()),
  'CHANGELOG.md': () => true,
  'crates/senlun-agent/CHANGELOG.md': () => true,
  'Cargo.toml': (before, after) => before !== null && sameLinesExceptVersion(before, after),
  'crates/senlun-agent/Cargo.toml': (before, after) => before !== null && sameLinesExceptVersion(before, after),
  'Cargo.lock': (before, after) => before !== null && sameLockExceptWorkspaceVersions(before, after),
  'web/package.json': (before, after) => before !== null && sameJsonExcept(before, after, value => { delete value.version; }),
  'web/package-lock.json': (before, after) => before !== null && sameJsonExcept(before, after, (value) => {
    delete value.version;
    if (value.packages?.['']) delete value.packages[''].version;
  }),
};

// `read(revision, path)` returns file text or null when absent at that revision.
export function isVersionOnly(paths, read) {
  if (paths.length === 0 || !paths.includes('.release-please-manifest.json')) return false;
  return paths.every((path) => {
    if (!VERSION_FILES.has(path)) return false;
    const after = read('head', path);
    if (after === null) return false;
    return VERSION_CHECKS[path](read('base', path), after);
  });
}

export function selectScope({ paths, read, verifiedTree = false }) {
  if (isVersionOnly(paths, read)) {
    return { jobs: new Set(['version', 'security']), rustPackages: '--workspace', reason: 'version-only Release Please change' };
  }
  if (verifiedTree) {
    return { jobs: new Set(), rustPackages: '--workspace', reason: 'identical tree already verified by its pull request' };
  }
  return classifyPaths(paths);
}

function git(args) {
  return execFileSync('git', args, { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });
}

function gitReader(base, head) {
  const revisions = { base, head };
  return (revision, path) => {
    try {
      return git(['show', `${revisions[revision]}:${path}`]);
    }
    catch {
      return null;
    }
  };
}

function parseArgs(argv) {
  const options = { full: false, verifiedTree: false };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === '--full') options.full = true;
    else if (arg === '--verified-tree') options.verifiedTree = true;
    else if (arg === '--base' || arg === '--head') options[arg.slice(2)] = argv[++index];
    else throw new Error(`Unknown argument ${arg}`);
  }
  return options;
}

export function scopeFromGit(base, head, verifiedTree = false) {
  assert.match(base ?? '', /^[a-f0-9]{40}$/, 'Expected a base commit SHA');
  assert.match(head ?? '', /^[a-f0-9]{40}$/, 'Expected a head commit SHA');
  const paths = git(['diff', '--name-only', '--no-renames', '-z', base, head])
    .split('\0').filter(Boolean);
  return selectScope({ paths, read: gitReader(base, head), verifiedTree });
}

function main(argv, env) {
  const options = parseArgs(argv);
  const scope = options.full
    ? fullScope('complete suite requested for this event')
    : scopeFromGit(options.base, options.head, options.verifiedTree);
  const lines = JOBS.map(job => `${job}=${scope.jobs.has(job)}`);
  lines.push(`rust_packages=${scope.rustPackages}`);
  console.log(`CI scope: ${scope.reason}`);
  for (const line of lines) console.log(line);
  if (env.GITHUB_OUTPUT) appendFileSync(env.GITHUB_OUTPUT, `${lines.join('\n')}\n`);
  if (env.GITHUB_STEP_SUMMARY) {
    appendFileSync(env.GITHUB_STEP_SUMMARY, `### CI scope\n\n${scope.reason}\n\n${lines.map(line => `- \`${line}\``).join('\n')}\n`);
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main(process.argv.slice(2), process.env);
}
