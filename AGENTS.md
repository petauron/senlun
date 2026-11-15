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
- Required checks must represent real checks on the exact version PR head SHA.
  When token-created PRs do not trigger them, explicitly dispatch the existing
  workflows. Metadata validation is a separate check, not a substitute for CI
  or CodeQL; never manufacture successful required-check results.
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

- Read `RELEASING.md` before changing release workflows. Keep the Rust workspace,
  Cargo lockfile, Web package files and manifest on the same release version.
  Verify both supported Linux architectures and preserve required licenses,
  checksums, SBOMs and provenance in distribution assets.

## Petauron 团队：Issue 跟踪与完成标准（MVP）

- 开发每项已获授权的功能或缺陷修复前，先查找并关联对应仓库的现有 issue；没有合适的 issue 时先创建，简要写明用户目标、最小范围和完成标准。不要重复创建，也不要把创建 issue 当作扩大实现或部署授权。
- 一项完整需求或缺陷使用一个主 issue 持续跟踪；开发、修复、CI、合并、发布、部署和验收复用它，不为每个操作、重试或版本单独建 issue。跨仓库工作优先在主 issue 关联各仓库 PR，只有可独立交付的问题才拆分 issue。
- PR 必须关联主 issue，并将 PR 链接、关键 CI 结果、发布版本及链接、适用的部署和验收结果回填到同一 issue。记录简洁、真实且可核验；未执行、失败、阻塞和尚未完成的步骤明确标注，不记录凭据、完整订阅链接或私有环境敏感信息。
- 只有约定范围实际完成且有相应验收证据后才关闭 issue。CI 通过、PR 合并或源码发布不自动代表部署及用户功能验收完成；范围包含部署或实测时，完成前保持 issue 开放，PR 使用关联引用而非会导致提前关闭的自动关闭关键词。
- 按 MVP 保持最小记录，不增加形式化审批、重复报告或额外测试要求。仅涉及代码或文档的任务，按其已约定完成标准关闭，不强加无关的发布或生产验收；所有操作仍遵循用户授权和现有 CI、安全规范。

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
