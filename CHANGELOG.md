# Changelog

All notable changes to the server workspace are recorded here.
## Unreleased

### <u>build-release</u>

#### Added

#### Changed

- Release tooling versions are now centralized in `build-release/build-config.json` (minimum rustc, cross image tag, in-image Tauri CLI) instead of constants scattered across `lib.mjs`/`docker.mjs`/`Dockerfile`; the Docker image build forwards the Tauri CLI version as a build arg.

#### Fixed

- Release tooling now checks the active rustc version before building: when it is older than the `minRustcVersion` floor in `build-config.json` (1.91.0, required by the locked `foyer@0.22.4+` dependency), it offers to run `rustup update` with confirmation instead of failing late inside `cargo tauri build`.

