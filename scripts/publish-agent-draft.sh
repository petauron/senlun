#!/bin/sh
# Complete only the Release Please Agent draft for an already published,
# attested Service release at the same protected source commit.
set -eu
: "${GH_TOKEN:?}" "${GITHUB_REPOSITORY:?}" "${GITHUB_SHA:?}" "${GITHUB_REF:?}"
: "${RELEASE_TAG:?}" "${RELEASE_SHA:?}" "${AGENT_TAG:?}"
test "$GITHUB_REF" = refs/heads/main
printf '%s' "$RELEASE_SHA" | grep -Eq '^[a-f0-9]{40}$'
printf '%s' "$GITHUB_SHA" | grep -Eq '^[a-f0-9]{40}$'
printf '%s' "$RELEASE_TAG" | grep -Eq '^v[0-9]+\.[0-9]+\.[0-9]+(-[A-Za-z0-9.-]+)?$'
printf '%s' "$AGENT_TAG" | grep -Eq '^senlun-agent-v[0-9]+\.[0-9]+\.[0-9]+(-[A-Za-z0-9.-]+)?$'
test "$(gh api "repos/$GITHUB_REPOSITORY/branches/main" --jq .protected)" = true
for source in "$RELEASE_SHA" "$GITHUB_SHA"; do
  status=$(gh api "repos/$GITHUB_REPOSITORY/compare/$source...main" --jq .status)
  case "$status" in identical|ahead) ;; *) echo 'source is not on protected main' >&2; exit 1 ;; esac
  ci=$(gh api "repos/$GITHUB_REPOSITORY/actions/workflows/ci.yml/runs?event=push&head_sha=$source&per_page=100" \
    --jq '.workflow_runs | sort_by(.run_number) | last')
  printf '%s' "$ci" | jq -e --arg sha "$source" '.head_sha == $sha and .head_branch == "main" and .status == "completed" and .conclusion == "success"' >/dev/null
  run_id=$(printf '%s' "$ci" | jq -r .id)
  gate=$(gh api "repos/$GITHUB_REPOSITORY/actions/runs/$run_id/jobs?filter=latest&per_page=100" \
    --jq '[.jobs[] | select(.name == "rust" and .status == "completed" and .conclusion == "success")] | length')
  test "$gate" -eq 1
done
service_version=$(gh api "repos/$GITHUB_REPOSITORY/contents/version.txt?ref=$RELEASE_SHA" --jq .content | base64 --decode)
agent_version=$(gh api "repos/$GITHUB_REPOSITORY/contents/crates/senlun-agent/version.txt?ref=$RELEASE_SHA" --jq .content | base64 --decode)
test "$RELEASE_TAG" = "v$service_version"
test "$AGENT_TAG" = "senlun-agent-v$agent_version"
test "$(gh release view "$RELEASE_TAG" --json isDraft --jq .isDraft)" = false
test "$(gh release view "$RELEASE_TAG" --json targetCommitish --jq .targetCommitish)" = "$RELEASE_SHA"
test "$(gh release view "$AGENT_TAG" --json targetCommitish --jq .targetCommitish)" = "$RELEASE_SHA"
test "$(gh release view "$AGENT_TAG" --json author --jq .author.login)" = 'github-actions[bot]'
agent_draft=$(gh release view "$AGENT_TAG" --json isDraft --jq .isDraft)
case "$agent_draft" in true|false) ;; *) exit 1 ;; esac

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
gh release download "$RELEASE_TAG" --dir "$work" --pattern "senlun-$RELEASE_TAG-linux-*.tar.gz" --pattern "senlun-$RELEASE_TAG-linux-*.spdx.json" --pattern SHA256SUMS
grep -F "  senlun-$RELEASE_TAG-linux-" "$work/SHA256SUMS" > "$work/agent-sums"
test "$(wc -l < "$work/agent-sums")" -eq 4
for arch in x86_64 aarch64; do
  for suffix in tar.gz spdx.json; do
    name="senlun-$RELEASE_TAG-linux-$arch.$suffix"
    test -s "$work/$name"
    test "$(grep -Fc "  $name" "$work/agent-sums")" -eq 1
  done
  file="$work/senlun-$RELEASE_TAG-linux-$arch.tar.gz"
  gh attestation verify "$file" --repo "$GITHUB_REPOSITORY" \
    --signer-workflow "$GITHUB_REPOSITORY/.github/workflows/release.yml" \
    --source-ref refs/heads/main --deny-self-hosted-runners --format json > "$work/attestation.json"
  jq -e --arg source "$RELEASE_SHA" \
    'any(.[]; any(.verificationResult.statement.predicate.buildDefinition.resolvedDependencies[]?; .name == "release-source" and .digest.gitCommit == $source))' \
    "$work/attestation.json" >/dev/null
done
(cd "$work" && sha256sum --check agent-sums)
if [ "$agent_draft" = false ]; then
  # A rerun may observe the already published result; verify its four asset
  # digests before treating it as complete. Never overwrite a public release.
  inventory=$(gh api "repos/$GITHUB_REPOSITORY/releases/tags/$AGENT_TAG")
  while IFS= read -r line; do
    digest=${line%% *}
    name=${line#*  }
    printf '%s' "$inventory" | jq -e --arg name "$name" --arg digest "sha256:$digest" \
      '[.assets[] | select(.name == $name and .state == "uploaded" and .digest == $digest)] | length == 1' >/dev/null
  done < "$work/agent-sums"
  exit 0
fi
cp "$work/agent-sums" "$work/SHA256SUMS"
gh release upload "$AGENT_TAG" "$work"/senlun-*.tar.gz "$work"/senlun-*.spdx.json "$work/SHA256SUMS" --clobber
gh release edit "$AGENT_TAG" --draft=false
