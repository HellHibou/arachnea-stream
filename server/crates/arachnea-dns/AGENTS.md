# Agent Instructions for `arachnea-dns`

- Follow the repository-level rules in `../../AGENTS.md`.
- Treat this crate's `README.md` and the root `TODO.md` as the migrated source of truth from the retired DNS specification.
- Keep DNS core logic in `src/core/`; keep listener, file-configuration, and CLI adaptation in `src/server/`.
- Do not duplicate resolver decisions in server code. Server paths should adapt incoming protocol traffic and call the core.
- Preserve the decision order `blocklist > local_records > smart_dns > cache > upstream`.
- Keep cache behavior per-core, memory-only for v1, and disabled by default unless configuration or API enables it.
- Keep `config-sample/` synchronized with accepted configuration fields and supported feature gates.
- When changing public DNS behavior, update this crate README, the root `TODO.md` when planned work changes, and append to the root `CHANGELOG.md`.
