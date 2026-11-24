# Changelog

## [0.2.1](https://github.com/Diaszano/netrack/compare/netrack-v0.2.0...netrack-v0.2.1) (2026-11-24)


### Bug Fixes

* **auth:** migrate to argon2 0.6 password hashing api ([#20](https://github.com/Diaszano/netrack/issues/20)) ([88eb095](https://github.com/Diaszano/netrack/commit/88eb095f005b58399767e87a08e8303d4e605930))

## [0.2.0](https://github.com/Diaszano/netrack/compare/netrack-v0.1.0...netrack-v0.2.0) (2026-11-23)


### Features

* **api:** json api, sse events and boot wiring ([f2689f1](https://github.com/Diaszano/netrack/commit/f2689f1215b956fbac21606e341ed54cfe25c1d9))
* audit follow-ups (recovered event, backups, stats by server, node tokens) ([#2](https://github.com/Diaszano/netrack/issues/2)) ([fcada6e](https://github.com/Diaszano/netrack/commit/fcada6e936eb11871828ac1976202bc8cff77a9e))
* **auth:** password protection levels, sessions and middleware ([1da9fad](https://github.com/Diaszano/netrack/commit/1da9fadcedbbdba8e3bc95e14ae6dc48b3ba8103))
* **data:** export, import with myspeed compatibility and database reset ([6e6ee6e](https://github.com/Diaszano/netrack/commit/6e6ee6e065b9be97a905aed8489fff62f6d5e6b4))
* **db:** speedtest repository and retention ([bf39408](https://github.com/Diaszano/netrack/commit/bf39408714257d12b46847c07d1235dcc69a2bc4))
* **db:** sqlite schema, migrations and validated config ([836482f](https://github.com/Diaszano/netrack/commit/836482f50d36e52f8bca1b7fd9069558baab60b9))
* **i18n:** locale resolution, pt-BR/en catalogs and server-tz formatters ([cf90d47](https://github.com/Diaszano/netrack/commit/cf90d473dc96dff21281bee152d8211ed1cb6e68))
* **integrations:** notification dispatcher and channel handlers ([399148d](https://github.com/Diaszano/netrack/commit/399148dfda8353fb3a7fef84799b932b0119b3a2))
* **measure:** parallel throughput engine and latency stats ([a40ef9f](https://github.com/Diaszano/netrack/commit/a40ef9f7ea3a31c1a555fe97d6dca01613a133bd))
* new features ([ee8833b](https://github.com/Diaszano/netrack/commit/ee8833b972489e8da822336a2bb6f37ffef4a084))
* **nodes:** multi-instance node management and reverse proxy ([a722acf](https://github.com/Diaszano/netrack/commit/a722acfdf78c696ea4cec903cd2d6f4ddfa56122))
* **prometheus:** prometheus metrics endpoint with basic auth ([e45cd67](https://github.com/Diaszano/netrack/commit/e45cd675b6a16fb8b2a717f83b3876d92f4294c2))
* **providers:** native cloudflare and librespeed ([550e7ab](https://github.com/Diaszano/netrack/commit/550e7ab3149b0bd57a949e6f7425cc3b5bac302a))
* **providers:** on-demand ookla cli download and execution ([1553030](https://github.com/Diaszano/netrack/commit/15530304eaa8bc6efc4a9859a8b32632b5df779d))
* **runner:** single-flight test runner with retry and events ([5f72b50](https://github.com/Diaszano/netrack/commit/5f72b50f64263f054dbf02ba56c025afb07b0da9))
* **scheduler:** cron scheduling with offset, pause and retention ([ebde126](https://github.com/Diaszano/netrack/commit/ebde12632ddfce954794ebf0f472f5c76be79c88))
* **stats:** sql statistics with hourly, heatmap and downsampled series ([98937fe](https://github.com/Diaszano/netrack/commit/98937fe4ea8f5861f16308957190416047f263ca))
* **system:** opengraph image, preview mode and version check ([c70cb11](https://github.com/Diaszano/netrack/commit/c70cb119301a4c3a5268fe66218cfcd957bb3be1))
* **telemetry:** export logs, traces and metrics over OTLP ([#4](https://github.com/Diaszano/netrack/issues/4)) ([128b8f4](https://github.com/Diaszano/netrack/commit/128b8f4f29bc62ce4e5e56785fc08e999f6528fd))
* **web:** api client, sse state reducer and dense panel layout ([77e0f53](https://github.com/Diaszano/netrack/commit/77e0f53d183eea2a01beaa9d1fe81b5694828cd3))
* **web:** embed static assets, fonts and locale index ([68a9f2e](https://github.com/Diaszano/netrack/commit/68a9f2e5a9ff22f1586ab2138a1a45086006e633))
* **web:** overview, statistics and history views ([7cb7820](https://github.com/Diaszano/netrack/commit/7cb7820921312ed0a2dc75f53b53bb0dca8ec15b))
* **web:** servers, settings, sidebar seams and live app wiring ([0412cc2](https://github.com/Diaszano/netrack/commit/0412cc23217cc0742f6e67392992451509a29b9d))
* **web:** svg gauge, speed area chart, heatmap and hourly chart ([7fb3476](https://github.com/Diaszano/netrack/commit/7fb3476954897eeafcc76de22faa594cdaf2c75a))
* **web:** wire auth, ookla consent, integrations, nodes and data views ([59fd642](https://github.com/Diaszano/netrack/commit/59fd642137425a5a7a55d05e0d4a9b4a6d229a02))


### Bug Fixes

* **api:** ipv6-only bind, sse shutdown and json error bodies ([751f73b](https://github.com/Diaszano/netrack/commit/751f73b505e5f8760099f38415eab5fd3ff17140))
* **auth:** block direct password_mode mutation and fast-path assets ([79b1e3f](https://github.com/Diaszano/netrack/commit/79b1e3f985401ffecda45099b6b47ca0fcaefdb9))
* **db:** propagate meta read errors and test cron persistence ([9cb5d65](https://github.com/Diaszano/netrack/commit/9cb5d65253192e5c20123986f37a569ceb50dce8))
* **measure:** treat empty download bodies as failed transfers ([cf2d4f0](https://github.com/Diaszano/netrack/commit/cf2d4f078cba0780683fc3724ac9e705af696cac))
* **parity:** address final review findings for proxy paths, node export and data_dir ([22ae7df](https://github.com/Diaszano/netrack/commit/22ae7dfe5d17f6d1e2834a141be74fc57ee66b61))
* **providers:** compute server time like the official cloudflare test ([e034d91](https://github.com/Diaszano/netrack/commit/e034d917c8c59a4155ff49575ab5fd8f590f1218))
* **providers:** network-only ping, cloudflare colo, full libre pick ([2cdacaa](https://github.com/Diaszano/netrack/commit/2cdacaa22ebe6e1ac27cce7421e022b9de545944))
* **runner:** atomic run lock, terminal event after started, date bounds ([3ae6f0b](https://github.com/Diaszano/netrack/commit/3ae6f0b4e278e00ed3bc7c74ed7814242a756a14))
* **scheduler:** tolerate wall-clock steps and transient config errors ([d9da2f5](https://github.com/Diaszano/netrack/commit/d9da2f5236eaf2d7039c00705eb9abcd97d0ef81))
* **scheduler:** wait for the clock to pass stored history at boot ([c1c5e3a](https://github.com/Diaszano/netrack/commit/c1c5e3adc4b475329cf8520775d63c3e2e2c79ff))
* **settings:** treat an empty TZ as UTC ([f4c4378](https://github.com/Diaszano/netrack/commit/f4c437888a3ed9bccbb9c2b8dcffff0c68449076))
* **web:** add missing css grid styles for download heatmap cells and rows ([9e2d427](https://github.com/Diaszano/netrack/commit/9e2d4276546b1897380194f71e9156141f321e71))
* **web:** preserve form state on error and keep libre config ([1ca4c17](https://github.com/Diaszano/netrack/commit/1ca4c17f5aca4d386171c934eaa974ddb488e354))
* **web:** reconnect sse event source after login and node switch ([35ef2cf](https://github.com/Diaszano/netrack/commit/35ef2cf0fb85eeb9567b6053d06fa0ab17e61298))
* **web:** sync status on settings change & handle disconnected iface ([a0bbbce](https://github.com/Diaszano/netrack/commit/a0bbbceae1c2e9a4b171396f097414abeaa1e36d))
