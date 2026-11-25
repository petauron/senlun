# Version releases

The canonical repository is `petauron/senlun`. Release jobs derive source,
workflow, attestation and new `ghcr.io/petauron/senlun` image coordinates from
`github.repository`; OCI source labels use the same canonical URL. Existing
New releases use `senlun-*` archives, binaries and crates, with the Senlun
Release Please component; the root manifest key and version history stay intact.
Do not retag, republish or delete historical images or rewrite their provenance.
The paired catalog/Vastora changes must land before operators upgrade retained
installations; see [the technical migration](docs/technical-brand-migration.md).

Release Please v5 is pinned to 45996ed1f6d02564a971a2fa1b5860e934307cf7.
Version PR creation and updates mint a short-lived installation token from the
minimal-permission organization GitHub App (`petauron-nova`: `contents: write`
and `pull-requests: write` via `RELEASE_APP_CLIENT_ID` and
`RELEASE_APP_PRIVATE_KEY`) so normal `pull_request` CI and CodeQL checks run
automatically without a personal PAT. Actual release creation and publication
continue using the built-in `GITHUB_TOKEN`. Existing branch protection and
required CI remain mandatory.

The Service and Agent are independently versioned Release Please components.
The Service and internal protocol retain `workspace.package.version`; Web
versions follow the Service. The Agent has its own literal Cargo package
version, `crates/senlun-agent/version.txt`, manifest entry and component release
tag. Bootstrap its manifest at the latest already-published combined version;
do not create a historical tag or change an existing release. Both components
use the pinned built-in `simple` strategy and TOML/JSON extra-file updaters.
The root Cargo.lock selectors update only the corresponding local packages.
Cargo.lock selectors use `name.value` because this updater queries tagged TOML
values; third-party crates are excluded.

The Agent component tracks the Agent and shared protocol source, excluding
Service-only source and standalone Agent tests. Its `dependencies.sha256` records Cargo's resolved normal
and build dependency graphs and enabled features for both supported Linux musl
targets, relevant profiles and Agent build inputs. Service-only build commands
are isolated in `scripts/build-linux-service.sh` and its smoke-check helper.
When a change affects Agent inputs, refresh the marker in the same source PR
using the pinned Rust toolchain and existing Release Please engine:

```sh
export RELEASE_PLEASE_TEST_ROOT=/path/to/existing/release-please-regression
node scripts/agent-dependencies.mjs --write
```

The existing CI version job verifies the marker; a stale marker blocks the PR
rather than silently missing an Agent release. An unrelated dependency outside
the Agent's resolved graphs does not change it. The marker never includes runtime data,
credentials or a host path. Shared protocol and Agent build changes also count
as Agent changes. CI still uses the actual Cargo lock and existing security and
license checks for the entire workspace.

CI runs `scripts/check-release-please.mjs` against that pinned engine. It builds
candidate Service and Agent version changes in memory using the repository
configuration and checks their separate version updates and unchanged dependencies.
It has no GitHub client or write credentials and does not create a PR or tag.
When updating the action SHA, review its bundled release-please version and
update the regression engine pin together. The candidate version's ordinary
CI still checks the actual generated files with Cargo's `--locked` checks.

The Rust license template keeps workspace package attributions without the
product version; registry/git dependency versions and all license texts remain
included. A version-only PR must not stale the third-party notice bundle. CI
still regenerates and compares the complete third-party bundle with pinned
cargo-about whenever dependencies, the toolchain or license inputs change, and
installs `rust-docs` to re-verify `RUST_STDLIB_LICENSES.html` whenever the
pinned Rust toolchain, stdlib license notice, license script or full-suite CI
inputs change.

## Change-scoped CI

`ci.yml` verifies each change once, sized to what it can affect, leaving heavy
release packaging to `release.yml` unless packaging or dependency inputs change.
`scripts/ci-scope.mjs` (tested on every run) maps changed paths to jobs:
Web sources run Web checks and both browser viewports; allowlisted standalone
Agent collectors (`ip_unlock`, `return_routes`, `peer_latency`) and Agent CLI
tests run `-p senlun-agent` Clippy and tests only; Service, protocol and live
Agent reporting sources run crate-scoped Rust checks plus desktop browser flows;
dependency,
toolchain and container-input files add security and release/source image
smoke tests; documentation, deploy tooling and untested packaging notices run
only the fast repository checks. Unknown paths, workflow or classifier edits,
and manually dispatched runs select the complete suite.

A Release Please version PR is recognised by content, not by path: every
changed file must be a version file or changelog, and Cargo/npm manifests and
lockfiles may differ only in the local packages' version fields. Such a change
(on its PR and again on main) runs only version consistency, Release Please,
Agent build-input, lockfile and dependency-security checks — about a minute —
instead of repeating code builds that its unchanged code tree already passed.
Anything else falls back to path classification. The release workflow then
compiles, smoke-tests and attests the exact artifacts it publishes. On main,
scoped checks are used only when the previous main commit's own push CI
succeeded; a failed, cancelled or missing predecessor run makes the commit run
the complete suite, so a release never rests on unverified code. A squash
merge whose tree equals its successful same-repository PR head skips repeated
code checks on main. The `rust` gate fails unless every selected job succeeded
and every unselected job was skipped.

After a push to main, the release workflow waits (at most 30 minutes) for the
exact commit's CI to succeed. It then creates the release, finishes
publication, and maintains the next version PR. Review and merge that PR rather
than bumping versions or creating tags manually.

GITHUB_TOKEN-created tags do not start another release workflow. Version PRs
are created with the `petauron-nova` App token, so their pull request checks
start natively; if a generated PR's workflows ever require approval or do not
start, dispatch ci.yml on the version PR branch (a dispatched run executes the
complete suite). Do not rerun real failures blindly.
CI and dependency review use the same repository/branch concurrency identity for
PR and dispatch events, so overlapping runs cancel the older duplicate. A later
dispatch after completion is still a deliberate rerun. CI checks out the exact
PR head (or dispatch/push SHA); a dispatched run executes the complete suite.
The aggregate gate does not run in cancelled workflows, but still fails on any
failed selected job or any skipped selected job in an ordinary run. Release
preparation requires a complete successful CI workflow; cancellation or a
skipped gate cannot publish.

Publication runs in the same release.yml, gated by Release Please's output tags
and immutable source SHA. For new releases the source must equal the workflow
commit. A changed Agent gets a separate `senlun-agent-v...` draft; after the
combined native artifacts pass all checks and the Service release is published,
the Agent job verifies and copies those same attested archives and SBOMs into its
draft before publication. An unchanged Agent gets no new component release.
Tags are not a second release entry point. A stale push skips preparation when
main moved.

## Recover an unpublished draft

On a transient build failure, use GitHub's Re-run failed jobs for that run. If
the workflow itself needs a fix, merge the fix through ordinary PR checks first.
Then dispatch the existing Release workflow **on main**, with `release_tag` set
to the exact unfinished tag, for example:

```sh
gh workflow run release.yml --ref main -f release_tag=v0.1.0-alpha.15
```

If the Service release is already public but its Agent draft needs recovery,
dispatch the same workflow with the exact `senlun-agent-v...` draft tag instead.
The recovery verifies protected-main ancestry, exact-source and workflow CI,
the published Service release, both archive checksums and attested source before
copying into the original Agent draft. A public Agent release is never overwritten.

Recovery does not invoke Release Please to recreate the release. It requires an
existing bot-created draft whose target is a full commit SHA. It preserves that
source, checks main ancestry, any existing tag, successful source CI and all
version files, then runs the same native binary verification, multi-arch image
packaging and publication jobs. The workflow revision must itself be current
checked main. Published releases and mutable targets are rejected. Never
delete/recreate the draft, move its tag or overwrite a published release to work
around a failure.

Push-triggered preparation preserves pending drafts and does not create the next
version PR while one remains unfinished. Draft readers need job-scoped
`contents: write`: GitHub hides drafts from tokens without push-level access,
even when the job only checks metadata or downloads assets. Ordinary CI stays
read-only. No personal token, new secret or protection bypass is needed.

## Release provenance

The pinned `actions/attest` action signs a SLSA v1 predicate using the standard
`https://actions.github.io/buildtypes/workflow/v1` build type accepted by GitHub.
The workflow dependency resolves `refs/heads/main` to its checked SHA; an
additional `release-source` dependency identifies the immutable product source,
which can differ during recovery. Recovery records `release_tag` in dispatch
inputs. The workflow path, trigger and run attempt identify the builder
invocation, following the GitHub Actions workflow schema. It makes no additional
SLSA level claim. The publishing job binds the invocation to its actual run
attempt, including failed-job retries that reuse preparation outputs.
Checksums and SBOM generation remain mandatory. CI exercises
draft rejection, immutable identity, provenance and workflow safeguards without
GitHub writes or publication.


Local workflow edits alone do not activate releases. Push/review the changes,
then verify the first version PR CI and release run before calling the migration
complete. No local builds or tests are implied by this configuration change.

Ordinary CI keeps no downloadable artifacts. Release archives and SBOMs are
staged directly in the GitHub draft release, which downstream jobs read using
GITHUB_TOKEN; there is no duplicate Actions artifact copy. Docker build record
upload is disabled. Dependency/build caches remain available for CI speed.
Manual maintenance workflows follow the same no-artifact policy. The redundant
source-patch download workflow has been removed; ordinary CI retains its format,
generated-icon and frontend checks. Coverage reports are the only permitted
downloadable CI artifact, and require explicit approval and short retention.

## CI dependency caches and compilation

The pinned Swatinem/rust-cache action retains Cargo downloads and compiled
dependencies, prunes unused crates and artifacts older than one week, and excludes
workspace binaries and incremental outputs. Ubuntu checks and browser tests have
separate job keys; security caches only dependency sources. Native Bookworm builds
share a key between CI and release jobs, scoped by CPU architecture, Rust toolchain
and the pinned builder script. Ubuntu outputs never enter this cache. The Docker
builder mounts the restored Cargo home and runs with the runner's UID/GID.
Dependency changes retain the action's partial restore behavior. Product source
and embedded Web assets are compiled again for the checked source SHA.

Within CI, the container job compiles the Debian 12 binaries once, verifies both
binaries in the restricted non-root Debian runtime, then packages those same
binaries with Dockerfile.release and checks the final container's identity and
health. The source Dockerfile is built and smoke-tested in its own parallel job.
Clippy, Rust tests, Web checks, security, browser flows and image checks are
selected per change (see Change-scoped CI); the `rust` aggregate gate is always
required. Release builds still compile their own
immutable source for both supported architectures; only dependencies are reused.

Caches stay within GitHub's repository cache quota and normal eviction policy;
there are no per-commit cache keys or downloadable CI artifacts. PR caches cannot
seed main, so the first run on a new scope is cold. Compare actual job/step times
and cache hit logs separately from runner queue time when measuring improvements.

## Alpine probe runtime

The Linux Agent is built as static musl for each existing architecture. The
Service retains its GNU/Debian 12 baseline. `scripts/Dockerfile.native-builder`
adds only the approved musl C compiler and the pinned Rust musl standard library
target to the existing reviewed Rust base. No new application crate is added.
`build-linux-release.sh` rejects an Agent with an ELF interpreter or shared
library dependency, then retains the existing GNU runtime smoke checks.

For managed Alpine/OpenRC deployments, Vastora owns service/account creation,
private runtime credential copies and reboot recovery. The probe keeps its
existing persistent credential file across package upgrades; it does not enroll
again. This does not claim Alpine support for the Senlun Service or enable a
remote administration channel. Track the combined isolated acceptance in the
[Vastora Alpine/OpenRC issue](https://redirect.github.com/petauron/vastora/issues/1182); do not deploy from a build-only result.
