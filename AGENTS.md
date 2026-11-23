# Senlun Agent Rules

- Keep Senlun focused on server health monitoring. Do not add remote shells, file management, arbitrary command execution, or unrelated administration features.
- Optimize for predictable, low resident memory. Every persistent cache, background task, queue, and database buffer must have an explicit bound.
- Agents initiate outbound connections only. The Service must never gain a general-purpose command channel to an Agent.
- Do not add telemetry, crash uploads, analytics, or calls to third-party services unless an administrator explicitly configures them. The sole default-enabled exception is Agent country discovery through GeoJS: disclose it in installation/upgrade instructions, retain an explicit disable option and manual-region priority, and never send Senlun credentials or host metrics. Other third-party integrations still require explicit configuration.
- Released database schemas use tested, forward-only migrations. Back up before migration and fail closed on migration errors.
- Keep protocol changes versioned and explicit. Remove superseded pre-release implementations instead of maintaining hidden compatibility paths.
- Prefer memory-safe Rust and mature crates. Any `unsafe` block requires a documented invariant and focused tests.
- Build in small end-to-end slices and keep Service, Agent, protocol, storage, and Web concerns separate.
- Never commit credentials, enrollment tokens, private keys, production data, or generated runtime state.

- Do not run local builds. Generate Web assets and compile release artifacts in GitHub Actions; never commit `web/dist`.

## GitHub releases and Actions storage

- Use Release Please v5, pinned to a reviewed full commit SHA, with the built-in
  `GITHUB_TOKEN`. Do not add a PAT or a separate release-token secret. Grant only
  the job permissions needed; never weaken branch protection to enable releases.
- Release through the generated version PR and the repository's Release Please
  config/manifest. Use Conventional Commits, including the final squash title;
  do not hide a releasable fix under a `ci:` or `chore:` title. Do not manually
  bump versions, move tags, or add a second tag-triggered release path.
- Required checks must represent real checks on the exact head SHA. CI selects
  jobs from the change scope with the tested, fail-closed classifier
  `scripts/ci-scope.mjs`: unknown paths, workflow or classifier edits, and
  scheduled or manual runs execute the complete suite, and the `rust` gate
  requires every selected job to succeed and every unselected job to be
  skipped. A Release Please change proven by content to touch only version
  fields and changelogs runs the version and dependency-security checks instead
  of repeating code builds; the release workflow then builds, smoke-tests and
  attests the exact published artifacts. When token-created PRs do not trigger
  checks, explicitly dispatch the existing workflows. Never manufacture
  successful required-check results or weaken the classifier to skip a job a
  change can affect.
- Publish only from an immutable, checked commit on protected `main`. Use the
  Release Please output SHA/tag throughout checkout, build, provenance and
  publication; fail closed on identity/version mismatches or failed checks.
  Do not assume a tag created with `GITHUB_TOKEN` triggers another workflow.
- Keep releases as drafts until all required builds, integrity/provenance checks
  and uploads succeed. Retain only the distribution files and metadata required
  by users or verified consumers. Retry failed jobs or an explicitly documented
  recovery flow; never overwrite an already published release or move its tag.
- CI and manually dispatched maintenance workflows do not retain downloadable
  Actions artifacts: no binaries, UI bundles, browser evidence, source patches,
  workspace copies or build records. Do not add upload/download-artifact steps
  or retention-based exceptions for these files. Only explicitly requested test
  coverage reports may be uploaded, with narrowly scoped contents and short,
  documented retention. Keep the actual tests and required CI gates.
- Set `DOCKER_BUILD_RECORD_UPLOAD=false` for Docker build actions. Keep bounded
  dependency/build caches for CI speed; caches are not release artifacts.
  Stage necessary cross-job release files directly in the draft Release rather
  than keeping duplicate Actions artifacts.
- Artifact cleanup must enumerate exact targets, skip active runs and artifacts
  required to recover failed releases, and preserve published Release assets and
  caches unless separately authorized. Never include credentials, production
  data, private host details or full subscription URLs in files or logs.
- Distinguish workflow edits, successful CI, successful publication and production
  deployment in completion reports. A green preparation job with skipped publish
  jobs is not proof of a release. Documentation edits do not authorize a push,
  merge, release, signing operation or production deployment.

- Read `RELEASING.md` before changing release workflows. Keep the Service,
  workspace, Web and Service manifest versions aligned; keep the Agent crate,
  lockfile entry and Agent manifest on their independently released version.
  Refresh the checked Agent dependency marker when its dependency closure or
  build inputs change; unrelated Service-only changes must not bump the Agent.
  Verify both supported Linux architectures and preserve required licenses,
  checksums, SBOMs and provenance in distribution assets.

## Petauron 团队：Issue 跟踪与完成标准（MVP）

- 开发每项已获授权的功能或缺陷修复前，先查找并关联对应仓库的现有 issue；没有合适的 issue 时先创建，简要写明用户目标、最小范围和完成标准（仅覆盖代码/文档实现与自动化测试或 CI 验证，不写入线上部署或生产环境验收要求）。不要重复创建，也不要把创建 issue 当作扩大实现或部署授权。
- 一项完整需求或缺陷使用一个主 issue 持续跟踪；开发、修复、CI 与合并复用它，不为每个操作、重试或补丁单独建 issue。跨仓库工作可在主 issue 关联各仓库 PR，只有可独立交付的问题才拆分 issue。
- PR 必须关联主 issue，使用 `Closes #<编号>`（跨仓库使用 `Closes <owner>/<repo>#<编号>`）在合并时自动关闭对应 issue，并将关键测试与 CI 结果简明留痕；不记录凭据、完整订阅链接或私有环境敏感信息。
- Issue 以代码/文档改动完成且测试与必需 CI 验证通过为完成标准，PR 合并即自动关闭；后续生产发布、线上部署与私有环境实测验收不绑定公开 GitHub Issue，另由本地运维/任务记录承接。
- 按 MVP 保持最小记录，不增加形式化审批、重复报告或额外测试要求；所有操作仍遵循用户授权和现有 CI、安全规范。

## 节点标签维护（运维）

- 新接入、重装或维护节点标签时，沿用已有短标签风格。`tags` 保存顺序为「地区 → 线路或上游 → 带宽」，通常 2～3 项，使用分号分隔的 `文字<颜色>` 格式；信息不足时可以只保留地区，不猜测或凑满标签。
- 静态标签颜色按含义固定使用：地区 `gray`，已核实的优化线路／回程 `blue`，明确的上游运营商 `green`，套餐标称带宽 `violet`。这不是自动检测回程的等级配色；不要通过维护标签修改现有线路等级或 IP 评分样式。
- 地区使用与节点地区字段一致的大写两位代码，如 `JP`、`HK`、`TW`、`SG`、`US`。城市和供应商由已有名称、分组承载，不在标签重复。
- 线路或上游只选一项简短且有依据的信息，如 `CN2/9929`、`三网4837`、`Akari`；不使用泛称 `BGP`、`国际线路`、广告语或未知信息。新增或改变线路内容前，核对用户确认、套餐资料或对应协议族的回程记录；不得从名称、位置或一次延迟推断线路，「三网」必须有三网依据。
- 优化仅适用于一个 IP 协议族时，在同一标签写明范围，如 `V4 CN2/9929`、`AT&T家宽V6`，不能暗示另一协议族同样优化。不重复堆叠协议标签，也不把静态声明冒充自动检测结果。
- 带宽统一保存为 `100M`、`200M`、`300M`、`500M`、`1G`、`2G`、`4G` 等；采用已确认的套餐端口值，未知留空，不用一次测速结果或流量额度替代。当前界面会从 `tags` 提取带宽单独展示并按数值着色，普通标签区域不重复展示带宽；保留存储约定，不强制把它塞回标签行。
- 保持简短单行，不增加重复商家名、CPU／内存／磁盘规格、价格、周期流量额度或「实测」「待确认」等字样；这些由已有名称、资源指标、费用及流量区域承载。
- 存储格式示例：`US<gray>;V4 CN2/9929<blue>;500M<violet>`；`TW<gray>;Akari<green>;2G<violet>`；未知线路但已知带宽时为 `US<gray>;300M<violet>`。
- 标签整理只修改指定节点的 `tags`；保留身份、名称、地区、分组、排序、费用、流量额度／重置日／来源、计数、探测配置及历史。保存后读回标签并确认其他选项未变；不顺带重装或重启探针、触发检测或全网测速。

## Petauron 团队：任务工作区与失效引用收尾（MVP）

- 开发完成或交接时，按以下三步清理本任务不再需要的工作区、分支和失效远程跟踪引用；发布与验收继续按原任务跟踪，无需为等待后续阶段保留已无用途的工作区。
- 工作区：确认没有待保留的未提交内容或其他任务使用后，使用 `git worktree remove` 移除 worktree；Codex 受管工作区使用其归档工具。独立检出先确认没有需保留的独有提交、stash、忽略文件及关联工作区，再删除。可重建的私有依赖和缓存可一起清理，不删除其他工作区引用的共享源。
- 任务分支：确认 PR 已合并到目标分支、本地分支没有超出 PR 合并时最终 head 的提交，且没有其他工作区或任务使用后，删除本地任务分支。Squash 合并以 PR 最终 head 为准；`git branch -d` 不适用时，核实上述条件后可用 `git branch -D`。已合并的远端任务分支按仓库自动删除设置清理；需手动删除时核对当前远端提交、使用情况及保护规则。
- 失效远程跟踪引用：核对远端和 refspec，先用 `git remote prune origin --dry-run` 预览；全部确认安全时执行 prune，混有不明或需保留项时仅定向清理已确认项，避免特殊 refspec 或 prune-tags 误删标签。普通已完成任务不逐条备份 reflog，也不为其新建 `refs/archive`。
- 未提交内容、独有资料、仍在使用的工作区、共享 Git 数据和受保护分支继续保留；只在有异常或依赖线索时进一步核查。需要保全独有历史时，可保留相应引用或使用可恢复的 Git 备份；仅导出引用文本不能替代备份。
- 存量 `refs/archive` 和 `refs/codex` 快照暂不清理，发布标签保留。本规则不自动授权批量整理历史工作区、修改生产、发布或创建定时任务。
