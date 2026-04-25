# Changelog

## [0.2.0](https://github.com/metaneutrons/knx-rs/compare/knx-device-v0.1.0...knx-device-v0.2.0) (2026-04-25)


### ⚠ BREAKING CHANGES

* DptValue typed enum — eliminate f64 DPT API
* DPT API completely rewritten.

### Features

* implement PropertyExtDescriptionRead + translation splitting (zero TODOs) ([1a6065e](https://github.com/metaneutrons/knx-rs/commit/1a6065e536c81df488b55ede681e7b46b8667305))
* initial release — knx-core and knx-ip crates ([898facf](https://github.com/metaneutrons/knx-rs/commit/898facfbf12f965ca34ef5dc9f75c18777d98615))
* **knx-device:** add all missing application layer services ([20a2d5a](https://github.com/metaneutrons/knx-rs/commit/20a2d5a6aa1e91a4dec02e547082f9ed2f94c439))
* **knx-device:** add all missing BAU handlers ([65d2393](https://github.com/metaneutrons/knx-rs/commit/65d23934434dc848542cb5af5606a50fc6a96ecd))
* **knx-device:** add input validation and memory limits ([2f37670](https://github.com/metaneutrons/knx-rs/commit/2f376705cc9ee002a1502638462f69a398ae1293))
* **knx-device:** add memory_area() getter for persistence ([c523339](https://github.com/metaneutrons/knx-rs/commit/c523339a5f33ba2d5325112a641011a1b209c7b5))
* **knx-device:** add TableObject with ETS Load State Machine ([0be33cf](https://github.com/metaneutrons/knx-rs/commit/0be33cf626d497f4a7747b570a0f988ae6d25b7b))
* **knx-device:** connected-mode transport integration, configured() guard, timestamps ([2d02d3a](https://github.com/metaneutrons/knx-rs/commit/2d02d3a961cc5e8f2b235a362586ed0126f95040))
* **knx-device:** extended property services, program version persistence ([9cdbbbb](https://github.com/metaneutrons/knx-rs/commit/9cdbbbb1e911770194e5071c70a9b73ed8965d9c))
* **knx-device:** fixes 8-9 — DPT-aware group objects, C++ memory format ([8228f9f](https://github.com/metaneutrons/knx-rs/commit/8228f9f0d2de1532d9a591e116b91890c32fcf15))
* **knx-device:** full transport layer state machine with ACK/NACK/retry ([3eeb637](https://github.com/metaneutrons/knx-rs/commit/3eeb637884822a35c00fa17e90106dd2485d69b7))
* **knx-device:** group object I-flag init read, device descriptor property ([a36c9e6](https://github.com/metaneutrons/knx-rs/commit/a36c9e6b7ecc1d240c6b91897b9821b1eba2ba5f))
* **knx-device:** implement Display/Error for AppLayerError and PersistenceError ([2e0bb71](https://github.com/metaneutrons/knx-rs/commit/2e0bb718b3af73fe0449016fcd80ef5d49e9b0d1))
* **knx-device:** implement PropertyExtDescriptionRead handler ([0108ed8](https://github.com/metaneutrons/knx-rs/commit/0108ed87df1464353152308212df73c1b282c80e))
* **knx-device:** layer 5 — group objects with ComFlag state machine ([a4f83a4](https://github.com/metaneutrons/knx-rs/commit/a4f83a425e47057cdcaa16ec257c465b1c1fdfa2))
* **knx-device:** layers 1-4 — property system, interface objects, device/app objects, tables ([e57448d](https://github.com/metaneutrons/knx-rs/commit/e57448df56c6f8104c9faad52c0c109bb551811b))
* **knx-device:** layers 6-7 — memory, transport, application layers ([388bd8c](https://github.com/metaneutrons/knx-rs/commit/388bd8cd76e6739926b1caa6aca5a169c27c335d))
* **knx-device:** layers 8-9 — BAU controller and integration tests ([ed734fb](https://github.com/metaneutrons/knx-rs/commit/ed734fb021c1fe4bd830b520c780d4216b728555))
* **knx-device:** table object MCB CRC, table reference, fill byte ([6b82dd9](https://github.com/metaneutrons/knx-rs/commit/6b82dd9527734e5db006250c7c38ced082a3faf0))
* **knx-device:** wire TableObjects into BAU with save/restore ([decc671](https://github.com/metaneutrons/knx-rs/commit/decc6716b8d8a60d672e6b09e85617cc2c294727))


### Bug Fixes

* CI failures — clippy 1.95, doc links, test threading, audit action ([e507052](https://github.com/metaneutrons/knx-rs/commit/e5070520afb3736d5b82ff8167aa4f0dbd664c30))
* **knx-device:** enterprise-grade fixes — no allows, proper architecture ([992a234](https://github.com/metaneutrons/knx-rs/commit/992a23441ae2dce43d9ec2557eca66be7573bfda))
* **knx-device:** fix parse bugs — off-by-one in PropertyValueRead, ext header, AdcRead ([6d6a3a1](https://github.com/metaneutrons/knx-rs/commit/6d6a3a1382ccadd0febbc5c478b363099fe7f2f6))
* **knx-device:** GO initialization parity + all remaining audit findings ([7d87b09](https://github.com/metaneutrons/knx-rs/commit/7d87b093ca99b37ab65e8a1221f0ef230f0431e6))
* **knx-device:** resolve BAU audit findings BAU-1/2/4/5/6/8 ([4c469c9](https://github.com/metaneutrons/knx-rs/commit/4c469c904ba6da03325d2d5f26159102df03acca))
* **knx-device:** resolve BAU-3 and MEM-1 audit findings ([9ac17f2](https://github.com/metaneutrons/knx-rs/commit/9ac17f2d7acc7b611e2efc091198ae906acb94e4))
* **knx-device:** validate property_id truncation in ext services ([27d88ac](https://github.com/metaneutrons/knx-rs/commit/27d88acc44651650ae9dc93441fc709ff6901213))


### Code Refactoring

* DptValue typed enum — eliminate f64 DPT API ([56d335f](https://github.com/metaneutrons/knx-rs/commit/56d335f9b8923344c7199440ff220b7d876f8ba4))
* DptValue typed enum — single API, zero bare casts ([817dd8d](https://github.com/metaneutrons/knx-rs/commit/817dd8d76cc2e06580f92a85eca25955daf923f1))
