# Changelog

## [0.2.0](https://github.com/metaneutrons/knx-rs/compare/knx-core-v0.1.0...knx-core-v0.2.0) (2026-04-25)


### ⚠ BREAKING CHANGES

* DptValue typed enum — eliminate f64 DPT API
* DPT API completely rewritten.

### Features

* convenience APIs for SnapDog integration ([aa081dc](https://github.com/metaneutrons/knx-rs/commit/aa081dc9f27b5d8bb5a4ad71bfb1960f4966aa11))
* demo client example, publish all 4 crates, remove manual CHANGELOGs ([283693a](https://github.com/metaneutrons/knx-rs/commit/283693a9509f691b9c17b05e4554a489a08a7e72))
* DptValue Display, From&lt;Vec&lt;u8&gt;&gt;, serde, richer errors, golden coverage ([e7f113b](https://github.com/metaneutrons/knx-rs/commit/e7f113bda8fcdfde004664e4702d90f8885f7a80))
* initial release — knx-core and knx-ip crates ([898facf](https://github.com/metaneutrons/knx-rs/commit/898facfbf12f965ca34ef5dc9f75c18777d98615))
* **knx-device:** connected-mode transport integration, configured() guard, timestamps ([2d02d3a](https://github.com/metaneutrons/knx-rs/commit/2d02d3a961cc5e8f2b235a362586ed0126f95040))


### Bug Fixes

* **knx-ip:** resolve TUN-5 and TUN-6 audit findings ([d9aa3e2](https://github.com/metaneutrons/knx-rs/commit/d9aa3e2d10b0d8c33b70d07f5214d7da99779b62))
* resolve audit findings DPT-1, SIGN-1/2/3/4, DOC-1/2 ([bf83541](https://github.com/metaneutrons/knx-rs/commit/bf835416bfa3ef20acfa4fd5931a8b3ddf366b99))
* resolve broken tests from API changes ([8bc418c](https://github.com/metaneutrons/knx-rs/commit/8bc418c36e8af654622660e009822d7440e80d2a))


### Code Refactoring

* DptValue typed enum — eliminate f64 DPT API ([56d335f](https://github.com/metaneutrons/knx-rs/commit/56d335f9b8923344c7199440ff220b7d876f8ba4))
* DptValue typed enum — single API, zero bare casts ([817dd8d](https://github.com/metaneutrons/knx-rs/commit/817dd8d76cc2e06580f92a85eca25955daf923f1))
