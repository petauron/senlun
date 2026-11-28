# Changelog

All notable changes to Pulse will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html) after its first stable release.

## [0.2.2](https://github.com/petauron/senlun/compare/v0.2.1...v0.2.2) (2026-11-28)


### Bug Fixes

* **admin:** handle revoked node targets, stale channels, and multiline remarks ([#316](https://github.com/petauron/senlun/issues/316)) ([a2af28c](https://github.com/petauron/senlun/commit/a2af28c1932d95300441e217873376b74d8b3087)), closes [#312](https://github.com/petauron/senlun/issues/312)
* **service:** decouple ip_info cache reads from external provider I/O ([#309](https://github.com/petauron/senlun/issues/309)) ([2392df0](https://github.com/petauron/senlun/commit/2392df0f35c6f089fab8b4db7d6f1174c82262f9))
* **service:** harden auth rate limiting and narrow SessionGate locking ([#305](https://github.com/petauron/senlun/issues/305)) ([150ba84](https://github.com/petauron/senlun/commit/150ba84db9bea2023c057edf223b75d220eabbbf))
* **service:** isolate alert recovery eligibility and cooldown state ([#315](https://github.com/petauron/senlun/issues/315)) ([e8faa9f](https://github.com/petauron/senlun/commit/e8faa9f5521bcfee959caaeb7024e15029052c81)), closes [#311](https://github.com/petauron/senlun/issues/311)
* **service:** isolate external traffic per-source sync failures and harden retries ([#314](https://github.com/petauron/senlun/issues/314)) ([f79f535](https://github.com/petauron/senlun/commit/f79f5352909561a08fe3f42451060c3e6480abee)), closes [#300](https://github.com/petauron/senlun/issues/300)
* **service:** use IMMEDIATE ingestion transactions and isolate out-of-range snapshots ([#313](https://github.com/petauron/senlun/issues/313)) ([245fd00](https://github.com/petauron/senlun/commit/245fd006cddb3be08bc460144a6d961adeac0fc6)), closes [#310](https://github.com/petauron/senlun/issues/310)
* **web:** allow logout/401 past traffic draft guard and harden auth lifecycle ([#307](https://github.com/petauron/senlun/issues/307)) ([d4eb976](https://github.com/petauron/senlun/commit/d4eb976994ed53a774c4aad3dca46382e7e72a04))
* **web:** include public remarks in homepage node search ([#319](https://github.com/petauron/senlun/issues/319)) ([b243584](https://github.com/petauron/senlun/commit/b2435845989c566a5e06223db23555706af74ff1)), closes [#317](https://github.com/petauron/senlun/issues/317)
* **web:** make DataTooltip accessible and keep it within the viewport ([#321](https://github.com/petauron/senlun/issues/321)) ([a6e5f26](https://github.com/petauron/senlun/commit/a6e5f2645d16f12df0c5f0df84c1dfed2c193815)), closes [#318](https://github.com/petauron/senlun/issues/318)
* **web:** make homepage cards and list keyboard accessible ([#320](https://github.com/petauron/senlun/issues/320)) ([d444505](https://github.com/petauron/senlun/commit/d444505071b60288435001417df65ec5abb83cd3)), closes [#304](https://github.com/petauron/senlun/issues/304)
* **web:** stabilize NetworkQuality polling and fix sub-byte formatBytes scaling ([#308](https://github.com/petauron/senlun/issues/308)) ([a6801af](https://github.com/petauron/senlun/commit/a6801afff26cb767d6c54df88c4685c5ecea2772))

## [0.2.1](https://github.com/petauron/senlun/compare/v0.2.0...v0.2.1) (2026-11-24)


### Bug Fixes

* **auth:** move password hashing out of SQLite write locks and optimize CI/release builds ([#294](https://github.com/petauron/senlun/issues/294)) ([e6c584c](https://github.com/petauron/senlun/commit/e6c584cf3495e309ee0eb7d983a290b239a18376))

## [0.2.0](https://github.com/petauron/senlun/compare/v0.1.1...v0.2.0) (2026-11-22)


### Features

* add directional node-to-node latency monitoring ([#262](https://github.com/petauron/senlun/issues/262)) ([a7147d6](https://github.com/petauron/senlun/commit/a7147d6669223539efa428399ae0dd513add104d))


### Bug Fixes

* **deps-dev:** bump @iconify/json from 2.2.558 to 2.2.559 in /web in the web-minor-patch group ([#285](https://github.com/petauron/senlun/issues/285)) ([cfc0506](https://github.com/petauron/senlun/commit/cfc0506589293eea23f190128f06082c94db9539))
* **deps:** bump @iconify/vue from 4.3.0 to 5.0.5 in /web ([#259](https://github.com/petauron/senlun/issues/259)) ([6152beb](https://github.com/petauron/senlun/commit/6152bebb0fc1e98a4edaa6574f98f432b2922143))
* **deps:** bump actions/dependency-review-action ([#277](https://github.com/petauron/senlun/issues/277)) ([1f0005b](https://github.com/petauron/senlun/commit/1f0005b2914234b2ea5c79eda7349952458d6e0e))
* **deps:** bump pinia from 3.0.4 to 4.1.0 in /web ([#280](https://github.com/petauron/senlun/issues/280)) ([37a820c](https://github.com/petauron/senlun/commit/37a820cc55b9a910b2c9582806f994d0a46d7b47))
* **deps:** bump rust from 1.99-bookworm to 1.100-bookworm ([#251](https://github.com/petauron/senlun/issues/251)) ([0526c8f](https://github.com/petauron/senlun/commit/0526c8fabf3a826af24984936ac00ed26a9a7afc))
* **deps:** bump the cargo-minor-patch group with 4 updates ([#278](https://github.com/petauron/senlun/issues/278)) ([0ba22d6](https://github.com/petauron/senlun/commit/0ba22d67f35b343e245d14d4c901190bfd3011b4))
* **deps:** bump the web-minor-patch group in /web with 11 updates ([#279](https://github.com/petauron/senlun/issues/279)) ([c05121f](https://github.com/petauron/senlun/commit/c05121fde83388fd1edd84d8ad84b79e6857536a))
* **deps:** update url to 2.5.9 for RUSTSEC-2026-0592 ([#289](https://github.com/petauron/senlun/issues/289)) ([1c9e19d](https://github.com/petauron/senlun/commit/1c9e19d177e79ccac6b62ac680a65a937a87d5f0))
* remove display expiry, skip IPv6 TikTok unlock probe, and trim normal Google hint ([#287](https://github.com/petauron/senlun/issues/287)) ([496a67e](https://github.com/petauron/senlun/commit/496a67ee7e1dbe16a02984353720890b4eb96650))

## [0.1.1](https://github.com/petauron/senlun/compare/v0.1.0...v0.1.1) (2026-11-22)


### Bug Fixes

* **agent:** improve IP unlock and Google China-redirect probes ([#272](https://github.com/petauron/senlun/issues/272)) ([933bfeb](https://github.com/petauron/senlun/commit/933bfebc99e86e3233d312402afc8ebe4cebd76e))
* **deps-dev:** bump @tsconfig/node24 from 24.0.5 to 24.0.6 in /web ([#255](https://github.com/petauron/senlun/issues/255)) ([5e8e74d](https://github.com/petauron/senlun/commit/5e8e74da9f509939260aad9b8223bb8f573a82d1))
* **deps:** bump vue-echarts from 8.2.0 to 8.3.2 in /web ([#260](https://github.com/petauron/senlun/issues/260)) ([0ec0b9c](https://github.com/petauron/senlun/commit/0ec0b9c14a0080df9ed1ba9868d413baecd5921e))

## [0.1.0](https://github.com/petauron/senlun/compare/v0.1.0-alpha.70...v0.1.0) (2026-11-21)


### Features

* add on-demand IP unlock and Google region checks ([#265](https://github.com/petauron/senlun/issues/265)) ([40b8c7d](https://github.com/petauron/senlun/commit/40b8c7db1bcec8fed3a261c77f245d37076d4a8c))


### Miscellaneous Chores

* **release:** graduate Senlun from alpha to 0.1.0 ([#268](https://github.com/petauron/senlun/issues/268)) ([a2dec0d](https://github.com/petauron/senlun/commit/a2dec0d023178996fcc6789152ce9e0df69de16d)), closes [#267](https://github.com/petauron/senlun/issues/267)

## [0.1.0-alpha.70](https://github.com/petauron/senlun/compare/v0.1.0-alpha.69...v0.1.0-alpha.70) (2026-11-16)


### Bug Fixes

* exclude probe-only tests from agent releases ([bbd893f](https://github.com/petauron/senlun/commit/bbd893f6a228e20921120bedc55c9cd8ab316e05)), closes [#244](https://github.com/petauron/senlun/issues/244)
* narrow agent build dependency tracking ([62573ea](https://github.com/petauron/senlun/commit/62573ea89902a61860d0b98a689d2e44483a6f99)), closes [#244](https://github.com/petauron/senlun/issues/244)

## [0.1.0-alpha.69](https://github.com/petauron/senlun/compare/v0.1.0-alpha.68...v0.1.0-alpha.69) (2026-11-16)


### Features

* version Senlun Agent independently ([1e23680](https://github.com/petauron/senlun/commit/1e236801595a4f02a46e2d8c74f84f884215ddd3))

## [0.1.0-alpha.68](https://github.com/petauron/senlun/compare/v0.1.0-alpha.67...v0.1.0-alpha.68) (2026-11-16)


### Bug Fixes

* **web:** keep traffic tooltip readable on narrow screens ([4762993](https://github.com/petauron/senlun/commit/47629937a69c7bd1d73d12544601760a1a8bfbe4))

## [0.1.0-alpha.67](https://github.com/petauron/senlun/compare/v0.1.0-alpha.66...v0.1.0-alpha.67) (2026-11-16)


### Bug Fixes

* **deps:** repair Agent and accessibility compatibility for dependency updates ([#235](https://github.com/petauron/senlun/issues/235)) ([7ce5ae4](https://github.com/petauron/senlun/commit/7ce5ae4381ab06b19b22719ccf31e894419035fb))
* **security:** resolve vulnerable lint toolchain dependencies ([#238](https://github.com/petauron/senlun/issues/238)) ([179fe81](https://github.com/petauron/senlun/commit/179fe81d55c4d112bc59d90bb0b293223750f1c3))
* **web:** use short Hong Kong region name ([6887706](https://github.com/petauron/senlun/commit/6887706b57879ef44e652851bdb8c5e6a6f0dcd2))

## [0.1.0-alpha.66](https://github.com/petauron/senlun/compare/v0.1.0-alpha.65...v0.1.0-alpha.66) (2026-11-15)


### Bug Fixes

* **deps-dev:** bump @playwright/test from 1.63.0 to 1.65.0 in /web ([437681f](https://github.com/petauron/senlun/commit/437681fcdba7c3aead3ab2266ce1ed461f48d4f4))
* **deps-dev:** bump @vitejs/plugin-vue to 6.0.11 ([#172](https://github.com/petauron/senlun/issues/172)) ([13c0d7f](https://github.com/petauron/senlun/commit/13c0d7f268ed67092e2527f7a957753376ba7a2e))
* **deps-dev:** bump TypeScript to 6.0.3 ([#168](https://github.com/petauron/senlun/issues/168)) ([7fb5d4d](https://github.com/petauron/senlun/commit/7fb5d4deb7db00a3ed6e0f94d53a3e11e0a19f2a))
* **deps-dev:** bump Vitest to 5.1.0 ([#174](https://github.com/petauron/senlun/issues/174)) ([4ae11c4](https://github.com/petauron/senlun/commit/4ae11c4c31d6c4926c99f4630a5d5af82422d6d6))
* **deps:** bump docker/login-action from 4.7.0 to 4.8.0 ([15b0ba6](https://github.com/petauron/senlun/commit/15b0ba696ca71cef1dfb697855c210cb172be947))

## [0.1.0-alpha.65](https://github.com/petauron/senlun/compare/v0.1.0-alpha.64...v0.1.0-alpha.65) (2026-11-15)


### Bug Fixes

* **web:** preserve node detail clicks outside card controls ([#225](https://github.com/petauron/senlun/issues/225)) ([6743e4f](https://github.com/petauron/senlun/commit/6743e4f21f28e21d94485ac257569af9368a20f1))
* **web:** share current return route labels with node details ([#231](https://github.com/petauron/senlun/issues/231)) ([4b654ad](https://github.com/petauron/senlun/commit/4b654adee17387fe2f7c08f5d61939615ba1c86b)), closes [#229](https://github.com/petauron/senlun/issues/229)

## [0.1.0-alpha.64](https://github.com/petauron/senlun/compare/v0.1.0-alpha.63...v0.1.0-alpha.64) (2026-11-15)


### Bug Fixes

* **web:** keep only configured node group filters ([#227](https://github.com/petauron/senlun/issues/227)) ([959133d](https://github.com/petauron/senlun/commit/959133de3ac53f03f51e71948e44773d557c0f70)), closes [#226](https://github.com/petauron/senlun/issues/226)

## [0.1.0-alpha.63](https://github.com/petauron/senlun/compare/v0.1.0-alpha.62...v0.1.0-alpha.63) (2026-11-15)


### Bug Fixes

* keep saved IP quality scores without display expiry ([#222](https://github.com/petauron/senlun/issues/222)) ([09634e1](https://github.com/petauron/senlun/commit/09634e1712e85a5a07c6b03a6f95627e5e2e4629)), closes [#221](https://github.com/petauron/senlun/issues/221)

## [0.1.0-alpha.62](https://github.com/petauron/senlun/compare/v0.1.0-alpha.61...v0.1.0-alpha.62) (2026-11-15)


### Features

* **web:** integrate regional overview, flat map and compact carrier history ([#219](https://github.com/petauron/senlun/issues/219)) ([a5e26bb](https://github.com/petauron/senlun/commit/a5e26bbb5de3c2dd26b2d286cf7ad5e73f2d21f4)), closes [#218](https://github.com/petauron/senlun/issues/218)

## [0.1.0-alpha.61](https://github.com/petauron/senlun/compare/v0.1.0-alpha.60...v0.1.0-alpha.61) (2026-11-14)


### Features

* unify node card network quality and inline controls ([#216](https://github.com/petauron/senlun/issues/216)) ([db02d50](https://github.com/petauron/senlun/commit/db02d50f14047c36cde79ec96c9e70a2166d36aa))

## [0.1.0-alpha.60](https://github.com/petauron/senlun/compare/v0.1.0-alpha.59...v0.1.0-alpha.60) (2026-11-14)


### Features

* **web:** color ordinary home routes by hourly probe loss ([#213](https://github.com/petauron/senlun/issues/213)) ([2a88ea7](https://github.com/petauron/senlun/commit/2a88ea733b84c4efbe57273983b6bd4712aca5b8))

## [0.1.0-alpha.59](https://github.com/petauron/senlun/compare/v0.1.0-alpha.58...v0.1.0-alpha.59) (2026-11-14)


### Bug Fixes

* **web:** compact home card return routes into a single line ([#210](https://github.com/petauron/senlun/issues/210)) ([fa6b7d6](https://github.com/petauron/senlun/commit/fa6b7d678f4064ab37dbe85cfe51406a8c9983ab))

## [0.1.0-alpha.58](https://github.com/petauron/senlun/compare/v0.1.0-alpha.57...v0.1.0-alpha.58) (2026-11-14)


### Features

* **web:** refine return routes on home node cards ([#207](https://github.com/petauron/senlun/issues/207)) ([dfe8827](https://github.com/petauron/senlun/commit/dfe8827eff2d3fa5dc38c6d197231b3062410c81))

## [0.1.0-alpha.57](https://github.com/petauron/senlun/compare/v0.1.0-alpha.56...v0.1.0-alpha.57) (2026-11-14)


### Bug Fixes

* **web:** simplify rate labels and premium route badges ([#204](https://github.com/petauron/senlun/issues/204)) ([a81a82b](https://github.com/petauron/senlun/commit/a81a82b309392e1f24f553f5f22f59af490d7c7f))

## [0.1.0-alpha.56](https://github.com/petauron/senlun/compare/v0.1.0-alpha.55...v0.1.0-alpha.56) (2026-11-14)


### Bug Fixes

* **web:** align return route and IP quality badge colors ([#201](https://github.com/petauron/senlun/issues/201)) ([09b9eb7](https://github.com/petauron/senlun/commit/09b9eb7605c1b5d29bc6ae8321f1faeb80713efc))

## [0.1.0-alpha.55](https://github.com/petauron/senlun/compare/v0.1.0-alpha.54...v0.1.0-alpha.55) (2026-11-14)


### Bug Fixes

* **web:** restore carrier colors and move bandwidth next to rate ([#198](https://github.com/petauron/senlun/issues/198)) ([ca53a82](https://github.com/petauron/senlun/commit/ca53a8234e7762aecf6626b08f949bb0d4d53ba8))

## [0.1.0-alpha.54](https://github.com/petauron/senlun/compare/v0.1.0-alpha.53...v0.1.0-alpha.54) (2026-11-14)


### Bug Fixes

* persist IP quality cache across service restarts ([#193](https://github.com/petauron/senlun/issues/193)) ([b739b07](https://github.com/petauron/senlun/commit/b739b074f1bb1ea0fab4da759d0c7fd480652adf))
* **web:** keep three carrier routes on one row ([#195](https://github.com/petauron/senlun/issues/195)) ([d84bde3](https://github.com/petauron/senlun/commit/d84bde36afbe002a000d0919bcf6c91225d3ce53))

## [0.1.0-alpha.53](https://github.com/petauron/senlun/compare/v0.1.0-alpha.52...v0.1.0-alpha.53) (2026-11-14)


### Bug Fixes

* **web:** clarify node quality and bandwidth badges ([#190](https://github.com/petauron/senlun/issues/190)) ([31c376b](https://github.com/petauron/senlun/commit/31c376bd3e26eae0275142795654457988348c2d))

## [0.1.0-alpha.52](https://github.com/petauron/senlun/compare/v0.1.0-alpha.51...v0.1.0-alpha.52) (2026-11-14)


### Bug Fixes

* **web:** make IP version switch respond immediately ([#188](https://github.com/petauron/senlun/issues/188)) ([67d0364](https://github.com/petauron/senlun/commit/67d0364d3bc7f4352c5ee9272e0d08d52c3ad00d))

## [0.1.0-alpha.51](https://github.com/petauron/senlun/compare/v0.1.0-alpha.50...v0.1.0-alpha.51) (2026-11-13)


### Features

* run bounded Shanghai route checks from the node panel ([#186](https://github.com/petauron/senlun/issues/186)) ([3e55d99](https://github.com/petauron/senlun/commit/3e55d991627988a6818e732c9ee0f8173b412748))

## [0.1.0-alpha.50](https://github.com/petauron/senlun/compare/v0.1.0-alpha.49...v0.1.0-alpha.50) (2026-11-13)


### Bug Fixes

* polish node quality badges and synchronize IP family selection ([870cda0](https://github.com/petauron/senlun/commit/870cda02dfbc1a0616462f34731141170edc125c))

## [0.1.0-alpha.49](https://github.com/petauron/senlun/compare/v0.1.0-alpha.48...v0.1.0-alpha.49) (2026-11-13)


### Bug Fixes

* enrich the active dashboard with route and score controls ([e901dd9](https://github.com/petauron/senlun/commit/e901dd9df265ef041bf6fa55b1227ee8a13b25d5))

## [0.1.0-alpha.48](https://github.com/petauron/senlun/compare/v0.1.0-alpha.47...v0.1.0-alpha.48) (2026-11-13)


### Features

* display Shanghai TCP return routes and Net.Coffee scores ([#179](https://github.com/petauron/senlun/issues/179)) ([e6e6722](https://github.com/petauron/senlun/commit/e6e6722533c2c30bdf6839334d8c15cd7c1c6b21))


### Bug Fixes

* run Shanghai collector with a supported terminal environment ([4311552](https://github.com/petauron/senlun/commit/4311552947aaf307d605e2be29a09dfc3719094d))

## [0.1.0-alpha.47](https://github.com/petauron/senlun/compare/v0.1.0-alpha.46...v0.1.0-alpha.47) (2026-11-13)


### Bug Fixes

* compact traffic quota labels ([#176](https://github.com/petauron/senlun/issues/176)) ([e92c29f](https://github.com/petauron/senlun/commit/e92c29f316359db19f1032c880088bd64546d7c1)), closes [#175](https://github.com/petauron/senlun/issues/175)

## [0.1.0-alpha.46](https://github.com/petauron/senlun/compare/v0.1.0-alpha.45...v0.1.0-alpha.46) (2026-11-11)


### Bug Fixes

* **web:** show live probe selection count without redundant picker ([#163](https://github.com/petauron/senlun/issues/163)) ([19269d7](https://github.com/petauron/senlun/commit/19269d74d662a246983edc3192caf6f49d90d5c2))

## [0.1.0-alpha.45](https://github.com/petauron/senlun/compare/v0.1.0-alpha.44...v0.1.0-alpha.45) (2026-11-11)


### Bug Fixes

* **web:** add quick focus and family filters to probe history ([#161](https://github.com/petauron/senlun/issues/161)) ([251d09f](https://github.com/petauron/senlun/commit/251d09fdbab59c21f3fe899f06907ff1470e40bc))

## [0.1.0-alpha.44](https://github.com/petauron/senlun/compare/v0.1.0-alpha.43...v0.1.0-alpha.44) (2026-11-10)


### Bug Fixes

* **web:** make probe chart controls and tooltips responsive ([#159](https://github.com/petauron/senlun/issues/159)) ([3b4a0ea](https://github.com/petauron/senlun/commit/3b4a0eaf631d276a302838b01cee11437efef5ed))

## [0.1.0-alpha.43](https://github.com/petauron/senlun/compare/v0.1.0-alpha.42...v0.1.0-alpha.43) (2026-11-10)


### Features

* **web:** simplify probe history with a line inspector ([#157](https://github.com/petauron/senlun/issues/157)) ([ff1a159](https://github.com/petauron/senlun/commit/ff1a159e324bd427129b4488d44caadb3c828139))

## [0.1.0-alpha.42](https://github.com/petauron/senlun/compare/v0.1.0-alpha.41...v0.1.0-alpha.42) (2026-11-08)


### Features

* **agent:** build native musl probes for Alpine ([#150](https://github.com/petauron/senlun/issues/150)) ([3d335b3](https://github.com/petauron/senlun/commit/3d335b3d5a5f17b0c85325eadad1ed1d030d7f90))


### Bug Fixes

* **deps:** consolidate compatible monitoring dependencies ([#149](https://github.com/petauron/senlun/issues/149)) ([6148a6f](https://github.com/petauron/senlun/commit/6148a6fceb2c3bef328ea34a21f573cec9a06c54))
* **installer:** refuse fresh installs beside legacy runtimes ([f8d3735](https://github.com/petauron/senlun/commit/f8d373571de401dffde5b97021a559f36c489af8))
* select the available probe family on single-stack nodes ([#155](https://github.com/petauron/senlun/issues/155)) ([edf6d39](https://github.com/petauron/senlun/commit/edf6d39e6f22672235b651eb3366daef9efbb749))

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
