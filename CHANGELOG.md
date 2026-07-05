# Changelog

## [0.5.0](https://github.com/metaneutrons/knx-rs/compare/knx-rs-v0.4.0...knx-rs-v0.5.0) (2026-07-05)


### Features

* **prod:** user-supplied-key .knxprod RSA signing ([#17](https://github.com/metaneutrons/knx-rs/issues/17)) ([5a1d84b](https://github.com/metaneutrons/knx-rs/commit/5a1d84b6721ddbffb4d2ed65a83f0ca3fe93824d))


### Bug Fixes

* **prod:** patch fingerprint across Catalog/Hardware, not just the app ([#19](https://github.com/metaneutrons/knx-rs/issues/19)) ([28f39ce](https://github.com/metaneutrons/knx-rs/commit/28f39ce6a47cdd58e065082dc9ddb95e3b9f38b3))
* **prod:** soften unsigned warning to "maybe not importable" ([#23](https://github.com/metaneutrons/knx-rs/issues/23)) ([2bbc19d](https://github.com/metaneutrons/knx-rs/commit/2bbc19dd951b5412d2ba1cc30d9019aad00d3b87))

## [0.4.0](https://github.com/metaneutrons/knx-rs/compare/knx-rs-v0.3.2...knx-rs-v0.4.0) (2026-07-05)


### Features

* **device:** System-B conformance (unsecured) — decode fix + Tier-1/Tier-2 ([1e66ebe](https://github.com/metaneutrons/knx-rs/commit/1e66ebec35e53c6baff2b463f41c1f7898bb5ee9))

## [0.3.2](https://github.com/metaneutrons/knx-rs/compare/knx-rs-v0.3.1...knx-rs-v0.3.2) (2026-07-05)


### Bug Fixes

* **deps:** bump quick-xml to 0.41 to clear RUSTSEC-2026-0194/0195 ([#14](https://github.com/metaneutrons/knx-rs/issues/14)) ([7946825](https://github.com/metaneutrons/knx-rs/commit/7946825628cb00e6b544f725da69cf4091c85da6))

## [0.3.1](https://github.com/metaneutrons/knx-rs/compare/knx-rs-v0.3.0...knx-rs-v0.3.1) (2026-06-28)


### Bug Fixes

* **api:** restore 0.2 back-compat via deprecated shims ([2eb8cfc](https://github.com/metaneutrons/knx-rs/commit/2eb8cfc155d1f7c14b453a465c65e00cdc6930a9))
* **docs:** drop removed doc_auto_cfg feature gate (breaks docs.rs) ([2894346](https://github.com/metaneutrons/knx-rs/commit/2894346727bee9b24d44f448dd7cf1535845a9e2))
* **prod:** keep pipeline modules public ([4d01589](https://github.com/metaneutrons/knx-rs/commit/4d01589cac10ff363c0de2e6c7977f3f2ccd389f))

## [0.3.0](https://github.com/metaneutrons/knx-rs/compare/knx-rs-v0.2.0...knx-rs-v0.3.0) (2026-06-28)


### ⚠ BREAKING CHANGES

* **core:** rename KnxIpError, add KNXnet/IP constants and builders

### Features

* **ip:** structured error variants and a Result alias ([4cded38](https://github.com/metaneutrons/knx-rs/commit/4cded3830eb8403e7f9af7f03bd5652db5282fec))


### Bug Fixes

* **ci:** correct publish order to respect dependency graph ([c7bc168](https://github.com/metaneutrons/knx-rs/commit/c7bc168ad5a8e24a95498477b42367e6fd51dd5a))
* **core:** only short-encode group values that fit in 6 bits ([f303214](https://github.com/metaneutrons/knx-rs/commit/f303214a5d9fc8924bf41b0ad0f7e85c8338deb5))
* **core:** unify APCI masking and short-form APDU encoding ([dd79a05](https://github.com/metaneutrons/knx-rs/commit/dd79a05a468f6c45cbead6b418d4c6aaf940a1d6))
* **device:** correct app-layer parse offsets and dedup encoders ([5ef896b](https://github.com/metaneutrons/knx-rs/commit/5ef896bf05caea8dc5f57368132c3c78f37520c0))
* **device:** stop BAU poll-loop hang and check memory bounds ([fb49610](https://github.com/metaneutrons/knx-rs/commit/fb496103277b5a01aaed738aba704cc1313781d9))
* **device:** validate restored table state; single-source object indices ([e6bf712](https://github.com/metaneutrons/knx-rs/commit/e6bf7129c7331f83061362e4199d7e3de2c62254))
* **ip:** SO_REUSEADDR, exact RoutingBusy pause, structured router errors ([1e8f75d](https://github.com/metaneutrons/knx-rs/commit/1e8f75d1f0af40c2c4b4a402f9fd18ccb8c5259c))
* **prod:** fail loud on malformed hash input ([67fd35b](https://github.com/metaneutrons/knx-rs/commit/67fd35b92efb88785a2b875a6a55b1bafde6be3b))
* **tp:** correct extended-frame encoding and harden the receive path ([1e45370](https://github.com/metaneutrons/knx-rs/commit/1e453701b3241cbe38c04cffe732120c5f9f2274))


### Code Refactoring

* **core:** rename KnxIpError, add KNXnet/IP constants and builders ([517c90b](https://github.com/metaneutrons/knx-rs/commit/517c90bbdcb0c2b30e53da1eb35a6e3eff5b320e))

## [0.2.0](https://github.com/metaneutrons/knx-rs/compare/knx-rs-v0.1.1...knx-rs-v0.2.0) (2026-05-18)


### Features

* **ip:** object-safe KnxConnection, IPv6 support, source validation ([c57fa3c](https://github.com/metaneutrons/knx-rs/commit/c57fa3cd344ad65910efd0f81241515da04b369a))


### Bug Fixes

* **device:** enforce write_enable on Property::write, minor cleanups ([585015e](https://github.com/metaneutrons/knx-rs/commit/585015ef18e3b116eb273b9888238fbf1e55dc84))

## [0.1.1](https://github.com/metaneutrons/knx-rs/compare/knx-rs-v0.1.0...knx-rs-v0.1.1) (2026-05-01)


### Bug Fixes

* **ci:** add retry logic and longer wait for crates.io index propagation ([dac60fa](https://github.com/metaneutrons/knx-rs/commit/dac60faa9135d984057057da2ef09563597f1bda))

## 0.1.0 (2026-04-27)


### Features

* **examples:** demo client and ETS-programmable device ([85f59d5](https://github.com/metaneutrons/knx-rs/commit/85f59d50dec269ea8913fc11dd1025001c8deef7))
* **knx-core:** KNX protocol types, addresses, CEMI frames, and DPT conversions ([6a92231](https://github.com/metaneutrons/knx-rs/commit/6a92231e82c8cfc370477844f7a9f27e914b43e4))
* **knx-device:** KNX device stack with ETS programming support ([1241833](https://github.com/metaneutrons/knx-rs/commit/1241833835c64ca1ee7c5cc5c070ed51dce64251))
* **knx-ip:** async KNXnet/IP tunnel, router, discovery, and device server ([3f21b7f](https://github.com/metaneutrons/knx-rs/commit/3f21b7f54f8cd613456f8cb0be4fd43ad6b17ccc))
* **knx-prod:** cross-platform .knxprod generator with ETS hash verification ([aaa4fce](https://github.com/metaneutrons/knx-rs/commit/aaa4fced2903f4a3b6c6bd63e4c1dfef16e0a600))
* **knx-tp:** TP-UART data link layer for embedded targets ([de8cc73](https://github.com/metaneutrons/knx-rs/commit/de8cc7390090c783bba6fb4d73eaef170a496eb1))


### Bug Fixes

* add version to inter-crate path dependencies for crates.io ([893405e](https://github.com/metaneutrons/knx-rs/commit/893405e11f78f789829e94290e5a5a2dd1e1fdcf))
* **ci:** handle already-published crates in publish workflow ([7b54ec5](https://github.com/metaneutrons/knx-rs/commit/7b54ec56f27c6c9abe2941fc7a47894807b6e6ed))
* **ci:** release-please config for workspace-inherited versioning ([d20aa47](https://github.com/metaneutrons/knx-rs/commit/d20aa471a84941541525991adba97656fd4088b6))
* **ci:** set initial-version 0.1.0 for first release ([95923cc](https://github.com/metaneutrons/knx-rs/commit/95923ccffdf7cf7098eeaba0c2b44d3202b974c3))
* **ci:** update workflow crate names to knx-rs-* namespace ([db65590](https://github.com/metaneutrons/knx-rs/commit/db6559057c30c51421850dd70f8a2e8556c5fe2a))
* **ci:** use simple release type for workspace version updates ([b25b455](https://github.com/metaneutrons/knx-rs/commit/b25b4550727825b9a49fd54c5a2a2d3dfb6e4009))
* **knx-prod:** flaky zip test — write output outside source dir ([51b9c21](https://github.com/metaneutrons/knx-rs/commit/51b9c2129c05229fd139c0155c87e513162e6ed7))

## 0.1.0 (2026-04-26)


### Features

* **examples:** demo client and ETS-programmable device ([85f59d5](https://github.com/metaneutrons/knx-rs/commit/85f59d50dec269ea8913fc11dd1025001c8deef7))
* **knx-core:** KNX protocol types, addresses, CEMI frames, and DPT conversions ([6a92231](https://github.com/metaneutrons/knx-rs/commit/6a92231e82c8cfc370477844f7a9f27e914b43e4))
* **knx-device:** KNX device stack with ETS programming support ([1241833](https://github.com/metaneutrons/knx-rs/commit/1241833835c64ca1ee7c5cc5c070ed51dce64251))
* **knx-ip:** async KNXnet/IP tunnel, router, discovery, and device server ([3f21b7f](https://github.com/metaneutrons/knx-rs/commit/3f21b7f54f8cd613456f8cb0be4fd43ad6b17ccc))
* **knx-prod:** cross-platform .knxprod generator with ETS hash verification ([aaa4fce](https://github.com/metaneutrons/knx-rs/commit/aaa4fced2903f4a3b6c6bd63e4c1dfef16e0a600))
* **knx-tp:** TP-UART data link layer for embedded targets ([de8cc73](https://github.com/metaneutrons/knx-rs/commit/de8cc7390090c783bba6fb4d73eaef170a496eb1))


### Bug Fixes

* **ci:** release-please config for workspace-inherited versioning ([d20aa47](https://github.com/metaneutrons/knx-rs/commit/d20aa471a84941541525991adba97656fd4088b6))
* **ci:** set initial-version 0.1.0 for first release ([95923cc](https://github.com/metaneutrons/knx-rs/commit/95923ccffdf7cf7098eeaba0c2b44d3202b974c3))
* **ci:** use simple release type for workspace version updates ([b25b455](https://github.com/metaneutrons/knx-rs/commit/b25b4550727825b9a49fd54c5a2a2d3dfb6e4009))
