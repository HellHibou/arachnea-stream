# Arachnea build-release Agent Profile

Scope: every file under `build-release/` — release tooling (`*.mjs`),
configuration (`release-config.json`) and the cross-build image
(`build-release/docker/Dockerfile`). This file extends the repository-root
`AGENTS.md`; its baseline rules apply here too.

## Docker image versioning

- Any change that impacts the Docker image (`build-release/docker/Dockerfile`
  or anything copied into the image context) MUST be followed by a version tag
  bump of `arachnea-cross-builder`: increment the last part of the number
  (`1.0.0` -> `1.0.1`), and keep `CROSS_IMAGE` in `build-release/docker.mjs`
  in sync.

## Multi-platform constraint

- Every change must account for the fact that the project stays multi-platform.
  Never assume a single host OS or CPU architecture: macOS (arm64/x86_64),
  Windows and Linux hosts must all remain able to run this tooling, and the
  cross image must keep working for both of its architecture ports
  (`linux/amd64` and `linux/arm64`).

## Software installation

- If a change requires installing a piece of software (host tool or image
  package), it MUST be installed automatically when it is not already present —
  do not leave a broken state behind.
- Every installation of software MUST go through explicit user confirmation
  before anything is installed (see the existing confirmation prompts in
  `build-release/install-tools.mjs` for the established pattern).

## Language

- All texts (comments, doc comments, log/help messages, commit-facing strings)
  MUST be written in English.

## External Docker resources

- You may NOT delete, modify or execute Docker components that are not part of
  this project without asking the user for confirmation first. This includes,
  but is not limited to: pre-existing images from other projects, containers,
  volumes, networks and builders not created by/for `build-release`.
