# Agent Instructions for `arachnea-proxy`

- Follow the repository-level rules in `../../AGENTS.md`.
- Treat this crate's `README.md` and the root `docs/TODO.md` as the migrated source of truth from the retired proxy specification.
- Keep routing, chain selection, privacy policy, and outbound transport decisions in `src/core/`.
- Keep server code in `src/server/` focused on protocol parsing, listener setup, ACLs, authentication checks, and adaptation to core calls.
- Keep connector code in `src/connectors/` free of independent routing logic.
- Preserve hostnames when the incoming and upstream protocols can carry them; resolve locally only when required.
- Preserve the default safety posture: loopback by default, no unsafe public open proxy unless configuration explicitly allows it.
- Do not add claims of anonymity without a documented threat model and explicit limitations.
- When changing public proxy behavior, update this crate README, the root `docs/TODO.md` when planned work changes, and append to the root `CHANGELOG.md`.
