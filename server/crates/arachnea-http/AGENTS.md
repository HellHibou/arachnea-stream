# Agent Instructions for `arachnea-http`

- Follow the repository-level rules in `../../AGENTS.md`.
- Treat this crate's `README.md` and the root `TODO.md` as the migrated source of truth from the retired HTTP specification.
- Keep engine-specific code behind the engine abstraction in `src/engine/`.
- Keep Cloudflare behavior explicit in configuration. Do not add implicit solver fallbacks to ordinary HTTP requests.
- Preserve the shared in-memory cookie-cache model unless the README and implementation are updated together.
- Keep authorization, rate-limit, and legal-safety notes visible when documenting Cloudflare-oriented behavior.
- Redact cookie values and other sensitive headers in logs, traces, and errors.
- When changing public HTTP behavior, update this crate README, the root `TODO.md` when planned work changes, and append to the root `CHANGELOG.md`.
