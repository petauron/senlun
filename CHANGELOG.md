# Changelog

All notable changes to Pulse will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html) after its first stable release.

## [0.1.0-alpha.41](https://github.com/petauron/senlun/compare/v0.1.0-alpha.40...v0.1.0-alpha.41) (2026-10-30)


### Bug Fixes

* **ui:** 统一表单控件 ([2b9f0a1](https://github.com/petauron/senlun/commit/2b9f0a171121da085c36361e9ebe91394fa02f9d))

## [0.1.0-alpha.40](https://github.com/petauron/senlun/compare/v0.1.0-alpha.39...v0.1.0-alpha.40) (2026-10-30)


### Bug Fixes

* match Emerald traffic tooltip ([1be7085](https://github.com/petauron/senlun/commit/1be7085b7c12cf3c759cfe64c83c9efaf67547aa))

## [0.1.0-alpha.39](https://github.com/petauron/senlun/compare/v0.1.0-alpha.38...v0.1.0-alpha.39) (2026-10-30)


### Bug Fixes

* restore traffic directions on node cards ([37beef9](https://github.com/petauron/senlun/commit/37beef96ff97d1712305459ec775be626a314f4d)), refs [#70](https://github.com/petauron/senlun/issues/70)

## [0.1.0-alpha.38](https://github.com/petauron/senlun/compare/v0.1.0-alpha.37...v0.1.0-alpha.38) (2026-10-30)


### Bug Fixes

* align probe panels when IPv6 switch is hidden ([3a755cd](https://github.com/petauron/senlun/commit/3a755cd0dfc2bb9642ec18e353ed873e5cdd02a9)), closes [#138](https://github.com/petauron/senlun/issues/138)

## [0.1.0-alpha.37](https://github.com/petauron/senlun/compare/v0.1.0-alpha.36...v0.1.0-alpha.37) (2026-10-29)


### Bug Fixes

* **deps-dev:** bump vite from 8.3.3 to 8.3.5 ([4a38185](https://github.com/petauron/senlun/commit/4a38185677000e036faf441ed635573ca9a27753))
* **deps-dev:** update eslint config to 10.0.0 ([5c4a476](https://github.com/petauron/senlun/commit/5c4a4764840d8504288345b2b5afd6536943cb85))
* **deps-dev:** update vite-plugin-static-copy to 4.1.1 ([717e735](https://github.com/petauron/senlun/commit/717e7352822d32b443bbd18768a071fbaa07e41b))
* **deps:** bump actions/setup-node from 7.0.0 to 7.1.0 ([195c10d](https://github.com/petauron/senlun/commit/195c10d8a345a1ce3d1f3ee35ae09199d2ab1585))
* **deps:** bump reka-ui from 2.10.4 to 2.11.0 ([1e7f655](https://github.com/petauron/senlun/commit/1e7f65586971de3173ca30e18e79cde2fdf123f5))
* **deps:** bump tailwind-merge from 3.6.0 to 3.7.0 ([e72103d](https://github.com/petauron/senlun/commit/e72103dfa1456a511e0f84ceb198f5a43afa6dd2))
* **deps:** update rust-embed to 8.13.0 ([b07e6ec](https://github.com/petauron/senlun/commit/b07e6ec8bb7dfcae715222fc990c6490d30bb014))
* **deps:** update rustix to 1.1.6 ([ca81cd5](https://github.com/petauron/senlun/commit/ca81cd55be0f660319540bd72e87b053ce77558a))
* **deps:** update serde_json to 1.0.154 ([cbbbaf2](https://github.com/petauron/senlun/commit/cbbbaf29bccffd81362f0d9431a6faca54184764))
* **deps:** update sha2 to 0.11.1 ([e115c6f](https://github.com/petauron/senlun/commit/e115c6f57a25165b1e85f18597431f6058cbee68))
* **deps:** update zip to 9.1.3 ([e3567f4](https://github.com/petauron/senlun/commit/e3567f4f8c9004f3b593f19bdf77fe1be8180c03))

## [0.1.0-alpha.36](https://github.com/petauron/senlun/compare/v0.1.0-alpha.35...v0.1.0-alpha.36) (2026-10-29)


### Bug Fixes

* **security:** update zip to patched release ([1ebbaaf](https://github.com/petauron/senlun/commit/1ebbaaf3845daddfeacc186c9b5d02eed09cc7d9))

## [0.1.0-alpha.35](https://github.com/petauron/senlun/compare/v0.1.0-alpha.34...v0.1.0-alpha.35) (2026-10-29)


### Bug Fixes

* hide unavailable IPv6 probe switch ([2ff6b29](https://github.com/petauron/senlun/commit/2ff6b296ac30792a877e00ab304a528fa63c2f98))

## [0.1.0-alpha.34](https://github.com/petauron/senlun/compare/v0.1.0-alpha.33...v0.1.0-alpha.34) (2026-10-29)


### Performance Improvements

* reduce monitoring overhead and protect probe scheduling ([b6d0130](https://github.com/petauron/senlun/commit/b6d0130a3a381daa4a5605b8f151367dee5ea35b)), closes [#113](https://github.com/petauron/senlun/issues/113)

## [0.1.0-alpha.33](https://github.com/petauron/senlun/compare/v0.1.0-alpha.32...v0.1.0-alpha.33) (2026-10-29)


### Bug Fixes

* apply traffic reset day immediately ([63001e4](https://github.com/petauron/senlun/commit/63001e48cdfc6e43254f3cffb6c8e1567d3b8cfe)), closes [#111](https://github.com/petauron/senlun/issues/111)

## [0.1.0-alpha.32](https://github.com/petauron/senlun/compare/v0.1.0-alpha.31...v0.1.0-alpha.32) (2026-10-29)


### Features

* support month-end traffic reset days ([5b1a4ba](https://github.com/petauron/senlun/commit/5b1a4ba536ef004e11961f23a68a81ebfa4cf671)), closes [#111](https://github.com/petauron/senlun/issues/111)

## [0.1.0-alpha.31](https://github.com/petauron/senlun/compare/v0.1.0-alpha.30...v0.1.0-alpha.31) (2026-10-28)


### Features

* rename Senulun to Senlun with retained storage migration ([#108](https://github.com/petauron/senlun/issues/108)) ([60c7eea](https://github.com/petauron/senlun/commit/60c7eeabc7ea8a9b1e0e496171a7e10fe5d94e56))


### Bug Fixes

* retain installed themes during Senlun storage rename ([#109](https://github.com/petauron/senlun/issues/109)) ([5b54bf5](https://github.com/petauron/senlun/commit/5b54bf53a6edd73f4300d1c963fd775d0ba8d51c))

## [0.1.0-alpha.30](https://github.com/petauron/senulun/compare/v0.1.0-alpha.29...v0.1.0-alpha.30) (2026-10-27)


### Features

* **traffic:** select per-node quota source for display and alerts ([#106](https://github.com/petauron/senulun/issues/106)) ([df5ab82](https://github.com/petauron/senulun/commit/df5ab82062f4443b8d2e89d28287ac9a7d89e57f))


### Bug Fixes

* **traffic:** bound authentication recovery attempts ([#104](https://github.com/petauron/senulun/issues/104)) ([1d016ca](https://github.com/petauron/senulun/commit/1d016caf101703e59b39f1f01fe057b0c6404445))

## [0.1.0-alpha.29](https://github.com/petauron/senulun/compare/v0.1.0-alpha.28...v0.1.0-alpha.29) (2026-10-24)


### Features

* **ui:** switch IPv4 and IPv6 probes per node ([#101](https://github.com/petauron/senulun/issues/101)) ([51878e9](https://github.com/petauron/senulun/commit/51878e957b10edf0fab603ed2ee42b7a9047ab05))

## [0.1.0-alpha.28](https://github.com/petauron/senulun/compare/v0.1.0-alpha.27...v0.1.0-alpha.28) (2026-10-24)


### Bug Fixes

* include IPv6 probes in dashboard summaries ([#99](https://github.com/petauron/senulun/issues/99)) ([560a0c8](https://github.com/petauron/senulun/commit/560a0c849b236b7d2fea43d5a7b12febd06cdb54))

## [0.1.0-alpha.27](https://github.com/petauron/senulun/compare/v0.1.0-alpha.26...v0.1.0-alpha.27) (2026-10-24)


### Bug Fixes

* **ui:** clarify local traffic alert scope ([0a97512](https://github.com/petauron/senulun/commit/0a97512553df63119a5c04e7f481aee194e6f48f))
* **ui:** show six home probe rows for IPv6 nodes ([#98](https://github.com/petauron/senulun/issues/98)) ([daabc7c](https://github.com/petauron/senulun/commit/daabc7c4f0959892b931c648aadd8746e3f756af))

## [0.1.0-alpha.26](https://github.com/petauron/senulun/compare/v0.1.0-alpha.25...v0.1.0-alpha.26) (2026-10-19)


### Features

* **traffic:** correct current-cycle local usage from node management ([c8695aa](https://github.com/petauron/senulun/commit/c8695aa61d895a5bbbf41c13ebeb877404bba9bb))


### Bug Fixes

* **traffic:** confirm unsaved drafts before leaving traffic settings ([2faf01f](https://github.com/petauron/senulun/commit/2faf01f563faefd660ec5a274faaf321662c0533))
* **traffic:** preserve unsaved source drafts when refreshing status ([80c9606](https://github.com/petauron/senulun/commit/80c9606a4784e48c66727f55becc84ec7828c666))


### Performance Improvements

* **agent:** avoid idle polling and duplicate metric collection ([8b6dbc3](https://github.com/petauron/senulun/commit/8b6dbc3399bae76f4c3b10b8cbb9e7bedbab7110))
* **storage:** group agent ingestion into durable SQLite batches ([eabacfd](https://github.com/petauron/senulun/commit/eabacfd54835546c3baa81fbb1feca53f597ca02))
* **storage:** persist minute rollups with a bounded hot window ([f3b420b](https://github.com/petauron/senulun/commit/f3b420b16a50c04aec193a4d85499a66500a2917))

## [0.1.0-alpha.25](https://github.com/petauron/senulun/compare/v0.1.0-alpha.24...v0.1.0-alpha.25) (2026-10-18)


### Bug Fixes

* **auth:** keep administrator logins valid for thirty days ([0086706](https://github.com/petauron/senulun/commit/00867061eff71777e9e053e8281485dfdf21cf5d))
* **deploy:** support documented standalone installation ([dfec921](https://github.com/petauron/senulun/commit/dfec92155873c992858248288630487febb71ee5))

## [0.1.0-alpha.24](https://github.com/petauron/senulun/compare/v0.1.0-alpha.23...v0.1.0-alpha.24) (2026-10-18)


### Bug Fixes

* **service:** accept visitor IP from an explicit trusted proxy ([b826a34](https://github.com/petauron/senulun/commit/b826a34fa57702efbeea6b935cc6fd57c3260aa7))

## [0.1.0-alpha.23](https://github.com/petauron/senulun/compare/v0.1.0-alpha.22...v0.1.0-alpha.23) (2026-10-18)


### Bug Fixes

* **web:** use Senulun mascot for site avatar and favicon ([b74a29a](https://github.com/petauron/senulun/commit/b74a29a442d3a2fc86275b53cde931e437518da8))

## [0.1.0-alpha.22](https://github.com/petauron/senulun/compare/v0.1.0-alpha.21...v0.1.0-alpha.22) (2026-10-17)


### Bug Fixes

* **senulun:** enable existing tempfile dependency for storage migration ([e5c226d](https://github.com/petauron/senulun/commit/e5c226dc43ff49a578796bbd640936810d4ea68b))
* **senulun:** migrate runtime branding with identity-preserving storage conversion ([b8662ba](https://github.com/petauron/senulun/commit/b8662ba34f6f1ed3133b03ae5daa3cda7cbca7ca))
* **senulun:** migrate runtime naming while preserving retained identities ([#77](https://github.com/petauron/senulun/issues/77)) ([378b6ad](https://github.com/petauron/senulun/commit/378b6ad2f6020af801d5a668aff0bcf0bbc2393f))
* **senulun:** reject incomplete migrated stores before startup ([f20c2b5](https://github.com/petauron/senulun/commit/f20c2b556bac44aba9c4e74f84670989aa209583))

## [0.1.0-alpha.21](https://github.com/petauron/senulun/compare/v0.1.0-alpha.20...v0.1.0-alpha.21) (2026-10-17)


### Bug Fixes

* restore three probe rows and simplify history details ([c2d61c0](https://github.com/petauron/senulun/commit/c2d61c07785ed92d3e6a90d530f08d118789eab2))

## [0.1.0-alpha.20](https://github.com/petauron/senulun/compare/v0.1.0-alpha.19...v0.1.0-alpha.20) (2026-10-17)


### Bug Fixes

* align Komari probe summary and bucket tooltips ([8471ebf](https://github.com/petauron/senulun/commit/8471ebf326d49a30f48b5f0e611e1d86bb8d84fd))

## [0.1.0-alpha.19](https://github.com/petauron/senulun/compare/v0.1.0-alpha.18...v0.1.0-alpha.19) (2026-10-17)


### Features

* add configurable external traffic monitoring and colored node tags ([879640b](https://github.com/petauron/senulun/commit/879640b7f0272eb9aeb813754c2152ed2e422efa))

## [0.1.0-alpha.18](https://github.com/petauron/senulun/compare/v0.1.0-alpha.17...v0.1.0-alpha.18) (2026-10-16)


### Bug Fixes

* align Emerald monitoring and Komari TCP latency ([a136fef](https://github.com/petauron/senulun/commit/a136fef44f879cb320f34b2466a230cb593e4237))
* **deps:** align and validate the updated build toolchain ([#57](https://github.com/petauron/senulun/issues/57)) ([f61d86e](https://github.com/petauron/senulun/commit/f61d86e70811829f164eaf2047877b0329dde4bb))
* **deps:** patch brace-expansion denial of service ([f482ff3](https://github.com/petauron/senulun/commit/f482ff36d1a72a981cde0ec184823df5e161caba))
* **deps:** patch shell-quote command injection ([7a36757](https://github.com/petauron/senulun/commit/7a367570a714bf815d76a08e49ecac13d2b40a71))
* **deps:** update Argon2 and reqwest with authentication regressions ([#18](https://github.com/petauron/senulun/issues/18)) ([617a975](https://github.com/petauron/senulun/commit/617a97584bf26ff6778fed8057e521af1d8d8f0f))

## [0.1.0-alpha.17](https://github.com/petauron/senulun/compare/v0.1.0-alpha.16...v0.1.0-alpha.17) (2026-10-12)


### Bug Fixes

* migrate canonical source and release identity to senulun ([6b63e42](https://github.com/petauron/senulun/commit/6b63e424259b2d0300494c2527e5d6fc2bd0532c))

## [0.1.0-alpha.16](https://github.com/petauron/pulse/compare/v0.1.0-alpha.15...v0.1.0-alpha.16) (2026-10-12)


### Bug Fixes

* **release:** recover immutable drafts with scoped permissions ([#49](https://github.com/petauron/pulse/issues/49)) ([24f944a](https://github.com/petauron/pulse/commit/24f944a598059d3741c89342e7d1b906a5abaac8))
* **release:** use supported GitHub workflow provenance ([#50](https://github.com/petauron/pulse/issues/50)) ([f0491a4](https://github.com/petauron/pulse/commit/f0491a4a88d2fb69f497ca05da8994d686c7df15))
* **web:** remove approximation prefix from remaining value ([fa09883](https://github.com/petauron/pulse/commit/fa0988390899d3991dceb2e2e8f6bb758da0da3b))

## [0.1.0-alpha.15](https://github.com/petauron/pulse/compare/v0.1.0-alpha.14...v0.1.0-alpha.15) (2026-10-11)


### Features

* rebrand Pulse as Senulun with backed-up title migration ([#44](https://github.com/petauron/pulse/issues/44)) ([1c6bd20](https://github.com/petauron/pulse/commit/1c6bd20e958ec020df54f14937ce9f79b301872f))


### Bug Fixes

* **deps:** use patched tinypool in the formatter toolchain ([#48](https://github.com/petauron/pulse/issues/48)) ([7b85b23](https://github.com/petauron/pulse/commit/7b85b236ec485eee35aa7aeffb508355d2ee2731))
* **release:** keep workspace license notices version independent ([#47](https://github.com/petauron/pulse/issues/47)) ([363f452](https://github.com/petauron/pulse/commit/363f452dd959f3ad6176b37da69ccd8008aecb06))
* **release:** support inherited Rust workspace versions ([#45](https://github.com/petauron/pulse/issues/45)) ([76a20c6](https://github.com/petauron/pulse/commit/76a20c6bbcb2f7efabd40c9aeebab81d80783628))

## [Unreleased]

## [0.1.0-alpha.13] - 2026-10-05

### Fixed

- Restore remaining-value dashboard summary with total value, monthly cost and currency details. Estimates explicitly use bundled reference exchange rates; no external requests are made.

## [0.1.0-alpha.12] - 2026-10-05

### Fixed

- Restore pricing and expiry information on dashboard cards and lists, replacing the redundant online last-report row. Missing expiry dates are no longer labeled expired.

## [0.1.0-alpha.11] - 2026-10-05

### Changed

- Align carrier latency panels and history charts with Emerald-Cazi spacing, colors, sample bars and tooltips while retaining explicit probe failure semantics.

## [0.1.0-alpha.10] - 2026-10-05

### Added

- Bounded carrier latency previews on node cards and lists, with visible failure rates and stale-data states.
- Audited CLI correction of current-cycle traffic, retaining subsequent increments and rejecting stale or mismatched counter baselines.

### Changed

- Network latency history now opens on a one-hour chart with time-range tabs, automatic refresh and collapsible detail tables.

## [0.1.0-alpha.9] - 2026-10-05

### Fixed

- Rebaseline decreasing traffic counters without adding their remaining lifetime totals to monthly usage; preserve unaffected upload/download increments.
- Document provider-facing interface selection and explicit handling of previously inflated usage totals.

## [0.1.0-alpha.8] - 2026-10-05

### Changed

- Administrator passwords now only require a non-empty value; setup and password changes no longer impose a separate password length policy. Authentication requests retain their existing size bound.
- Remove password length restrictions and helper text from the setup, login, and account forms.

## [0.1.0-alpha.7] - 2026-10-04

### Changed

- First administrator setup now requires only a username and password. The first successful registration closes setup; concurrent or later attempts cannot replace the administrator. Origin, CSRF, password, and session protections remain in place.
- Remove the setup-token configuration and deployment secret. Complete initial setup before sharing a new public instance. Existing administrator accounts are unchanged.

## [0.1.0-alpha.6] - 2026-10-03

### Added

- Read-only enrollment inspection to recover the original managed monitoring identity without creating a duplicate node.
- Authenticated node reporting inspection using the current credential on stdin, with rotation and last accepted report timestamps for recovery verification.

### Upgrade notes

- Preserve the existing Service data and collector credentials. This release supplies recovery evidence APIs; upgrading alone does not perform node recovery or prove that a replacement collector is reporting.
- Existing collectors remain separate upgrade targets. Country discovery settings and manual regions are unchanged.

## [0.1.0-alpha.5] - 2026-09-26

### Added

- Administrator-installed Komari theme ZIP packages, explicit activation, persisted theme settings, bounded extraction, and built-in Emerald fallback for authentication and administration.
- LuminaPlus v1.3.5 compatibility for dashboards, node histories, numeric Ping task bindings, assets, theme settings and mobile layouts. A reproducible Pulse-adapted theme ZIP and its upstream MIT license are included in the release.
- Optional, default-disabled IP information: registered public IPv4/IPv6, geography, ASN/network ownership, native/broadcast/anycast classification, six-location global latency, and CSRF-protected refresh in the original LuminaPlus interface.

### Fixed

- Upgrade rustls to 0.23.45 to address TLS handshake encryption-level validation advisory RUSTSEC-2026-0285.
- JavaScript-safe numeric probe identifiers and cross-node Ping history compatibility, settings-save CSRF support, and the LuminaPlus administration link.
- Nullable optional hardware samples no longer discard valid load or traffic history in the adapted theme.
- IP information APIs require administrator authentication even under the compatibility `/public/` path, validate node/address binding on every query, and never expose registered addresses to anonymous viewers.

### Upgrade notes

- Back up the entire Service state directory before upgrading. This release retains the alpha.4 database schemas and Agent protocol v2; existing Agent credentials remain valid and an Agent upgrade is not required for the theme/IP features.
- Install the bundled `LuminaPlus-v1.3.5-pulse.zip` through Admin → Themes, then activate it. Export existing theme settings before replacing an installed package; installation does not silently overwrite or activate a theme.
- IP information is disabled by default. Enable it in site settings and register node addresses explicitly. This permits Service-side requests containing the selected IP to Net.Coffee; no Pulse credentials or host metrics are sent. Mainland-China nodes are excluded. See `docs/THEMES.md` for quotas, cache bounds and third-party availability limits.
- LuminaPlus's default asset exchange-rate calculation permits browser requests to Frankfurter only while that theme is active. Custom external media/rate sources require explicit HTTPS settings.
- Theme packages are trusted administrator-installed browser code. This is not a general Komari plugin runtime, remote shell, file manager, or Komari Agent replacement.
- Publication does not deploy A1 or update Vastora's separate application catalog.

### Validation

- 106 Rust workspace tests, 24 management frontend tests and 379 LuminaPlus tests passed before release preparation, with strict Clippy and frontend lint checks.
- Real isolated Service/Agent browser verification covered desktop/mobile settings, registered addresses, real IP lookup, six-location latency, CSRF refresh, anonymous isolation and disabling the feature. No production nodes were used.

## [0.1.0-alpha.4] - 2026-09-12

### Changed

- Split the Service into paired control (`pulse.db`, schema v4) and metrics (`pulse.metrics.db`, schema v1) databases. Snapshots, probe history, traffic counters and last-seen state no longer share credential/configuration pages.
- Update node metadata only when it changes, cache ingestion statements, remove snapshot AUTOINCREMENT writes, and schedule bounded ingestion-path retention cleanup at most once per minute.
- Align snapshot and probe indexes with actual node/time/ID ordering; use WITHOUT ROWID for single-row traffic and last-seen state.
- Preserve raw sample precision and commit-before-acknowledgment durability with FULL synchronization on both files. No lossy sample buffer is enabled.
- Backups now return a private directory containing both databases and a manifest, with standalone copies that require no WAL sidecars.

### Fixed

- Preserve original credentials, sessions, TOTP state and metric fields during a backed-up, verified, forward-only split migration; fail closed on incomplete, mismatched or newer database pairs.
- Recover interrupted node/probe history cleanup and reserve both writers for consistent paired backups.
- Finalize backup copies for portable read-only access and never remove another pre-existing temporary backup on failure.

### Validation

- 90 workspace tests, strict Clippy and formatting checks passed before release preparation. Real temporary-Service upgrade checks retained original sessions, Agent credentials and WAL-backed history.
- A local macOS six-minute synthetic-HTTP comparison measured 10.53% fewer process-accounted write bytes at one-second intervals and 12.20% fewer at three-second intervals. All acknowledged samples survived test-process SIGKILL. This is not a Linux production benchmark or a power-loss simulation; see `docs/STORAGE-BENCHMARK.md`.

### Upgrade notes

- Back up and stop the old Service before upgrading. Allow additional disk space for the pre-upgrade single-file backup, metrics database, rollback journals and verification work.
- Persist the entire state directory. New backups must be restored as a matching pair; never create an empty replacement metrics file. The CLI `backup` output is now a directory, not a single file.
- `PULSE_MAX_DATABASE_BYTES` now caps each database independently: its default permits 2 GiB per file, up to 4 GiB across the pair, excluding WAL files/backups/migration scratch space.
- Do not downgrade a migrated database. Restore the single-file pre-upgrade backup with the matching older binary for rollback; retain the split pair separately.
- Existing Agent credentials and protocol v2 remain valid; an Agent upgrade is not required for this storage change. Publishing does not deploy Service instances or update Vastora's separate application catalog.

## [0.1.0-alpha.3] - 2026-09-12

### Added

- Built-in administrator setup/login, private monitoring by default, session revocation, TOTP, and explicitly configured GitHub OAuth.
- Emerald administration for node metadata, visibility, enrollment, credentials, billing-period traffic, asset fields, and site settings.
- Bounded ICMP/TCP/HTTP probes with real latency/loss history, alert rules, incident state, and durable notifications through administrator-enabled HTTPS webhooks.
- Optional process/socket/GPU metrics and automatic country discovery with manual-region priority and an explicit disable option.
- Authentication and monitoring operator guides, a private-bootstrap Compose example, and regression coverage for desktop/mobile administration and real Agent probes.

### Changed

- The default reporting interval is three seconds; administrators can configure 1–300 seconds centrally.
- SQLite schema v3 uses a backed-up, forward-only migration; the Agent snapshot/enrollment protocol remains v2.
- Webhook delivery stays disabled unless the administrator explicitly configures and enables a destination. Payloads exclude IP addresses, credentials and raw monitoring snapshots.

### Fixed

- Keep OAuth network requests outside the session-change lock while rechecking revoked credentials before session issuance.
- Apply the same strict database byte ceiling during growth and restart, including non-page-aligned limits.
- Prevent stale administrator refreshes during logout and preserve real history across supported monitoring views.

### Upgrade notes

- Back up the Service database before upgrading. Configure the exact HTTPS `PULSE_PUBLIC_URL` and a private `PULSE_SETUP_TOKEN_FILE`, then create the first administrator at `/login`. An upgraded dashboard is private until an administrator deliberately enables public reads.
- Do not downgrade a migrated schema v3 database. Restore the pre-upgrade backup with the matching older binary if rollback is necessary.
- Existing enrolled Agents retain their credentials. Upgrade Agents to use automatic country discovery, extended metrics, centrally configured reporting, and network probes.
- When no manual country is set, GeoJS lookup is enabled by default and reveals public egress IP and Agent version, but no Pulse credentials or metrics. Set `PULSE_GEOIP_PROVIDER=disabled` before starting/upgrading to opt out.
- Publishing Pulse does not update or deploy Vastora applications. The matching Vastora executor integration and signed catalog publication are separate steps.

## [0.1.0-alpha.2] - 2026-09-09

The Alpha.1 release was stopped before publication after runtime verification found an Agent glibc incompatibility. Its existing source tag is retained without rewriting history.

### Added

- Initial Rust workspace and open-source project foundation.
- End-to-end node enrollment, authenticated snapshot ingestion, and retained SQLite history.
- Outbound-only Agent loop for CPU, load, memory, swap, disk, network, uptime, and platform metrics.
- Embedded Komari Emerald dashboard with card, list, detail, online state, and load-history views.
- Local Iconify subsets and packaged flags so the dashboard does not require runtime asset CDNs.
- Short-lived database-managed enrollment tokens, Agent credential rotation, node revocation/deletion, and local audit records.
- Production systemd/container assets, backup and rollback procedures, release provenance, checksums, and SBOM generation.

### Changed

- Replaced Emerald's finance-oriented fields with monitoring status while retaining its interface structure and styling.
- Made Service receipt time authoritative and replaced newest-row history truncation with bounded full-window downsampling and coverage metadata.

### Fixed

- Built and smoke-tested both Linux release binaries against a pinned Debian 12 runtime baseline instead of inheriting the newer CI runner's glibc requirements.
- Resolved packaged flag filenames consistently on case-sensitive Linux filesystems across cards, lists, details, and globe overlays.
- Made offline chart gaps explicit and excluded stale offline samples from current usage totals.
- Restored automatic dashboard initialization after connection failures and keyboard access to node status summaries.
- Added safe credential recovery after incomplete enrollment and accepted IPv6 loopback Service URLs.
- Made credential lifecycle commits durable, coordinated online backups with writers, and bounded latest-status queries by node count.
- Corrected deterministic asset checks, release cleanup on failure/cancellation, and third-party license delivery for Rust binaries and containers.
