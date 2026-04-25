# Changelog

## [0.2.0](https://github.com/metaneutrons/knx-rs/compare/knx-ip-v0.1.0...knx-ip-v0.2.0) (2026-04-25)


### ⚠ BREAKING CHANGES

* DptValue typed enum — eliminate f64 DPT API
* DPT API completely rewritten.

### Features

* convenience APIs for SnapDog integration ([aa081dc](https://github.com/metaneutrons/knx-rs/commit/aa081dc9f27b5d8bb5a4ad71bfb1960f4966aa11))
* demo client example, publish all 4 crates, remove manual CHANGELOGs ([283693a](https://github.com/metaneutrons/knx-rs/commit/283693a9509f691b9c17b05e4554a489a08a7e72))
* initial release — knx-core and knx-ip crates ([898facf](https://github.com/metaneutrons/knx-rs/commit/898facfbf12f965ca34ef5dc9f75c18777d98615))
* **knx-ip:** GroupOps extension trait for high-level group operations ([1f9f300](https://github.com/metaneutrons/knx-rs/commit/1f9f300c407135711bef06c171899aa4d15397a8))
* **knx-ip:** tunnel server — accept incoming ETS connections ([87c868e](https://github.com/metaneutrons/knx-rs/commit/87c868ef3670777c4c7dd44ca470dd3a1d26091c))


### Bug Fixes

* CI — doc link discover→discovery, audit-check v2 with Node24 ([c123f53](https://github.com/metaneutrons/knx-rs/commit/c123f5360c7b41a81809323450c8bef19471e036))
* **knx-ip:** resolve TUN-1, TUN-4, LINT-1 audit findings ([c5dd1ca](https://github.com/metaneutrons/knx-rs/commit/c5dd1ca005bd76d3cfb55b17c6db156a9e53282d))
* **knx-ip:** resolve TUN-2, close DRY-2/3/SPLIT-1 — audit complete ([15bc66a](https://github.com/metaneutrons/knx-rs/commit/15bc66aa6faf6a1adfd14b29359291893fe925e7))
* **knx-ip:** resolve TUN-3 and TUN-7 audit findings ([da6f69e](https://github.com/metaneutrons/knx-rs/commit/da6f69e66a2038eab9ce39a414bad57bf120c0ad))
* **knx-ip:** resolve TUN-5 and TUN-6 audit findings ([d9aa3e2](https://github.com/metaneutrons/knx-rs/commit/d9aa3e2d10b0d8c33b70d07f5214d7da99779b62))
* **knx-ip:** TUN-2 proper ack-based server tunneling retry ([2c2798a](https://github.com/metaneutrons/knx-rs/commit/2c2798aa9e2394bb697c5b51cc971f74e29eec6c))
* **knx-ip:** update tunnel_integration tests for pub(crate) BAU fields ([b4478fd](https://github.com/metaneutrons/knx-rs/commit/b4478fd4c61e6bd7e2ed979daea1888e04cccbda))
* resolve broken tests from API changes ([8bc418c](https://github.com/metaneutrons/knx-rs/commit/8bc418c36e8af654622660e009822d7440e80d2a))


### Code Refactoring

* DptValue typed enum — eliminate f64 DPT API ([56d335f](https://github.com/metaneutrons/knx-rs/commit/56d335f9b8923344c7199440ff220b7d876f8ba4))
* DptValue typed enum — single API, zero bare casts ([817dd8d](https://github.com/metaneutrons/knx-rs/commit/817dd8d76cc2e06580f92a85eca25955daf923f1))
