# Arachnea Agent Profile

## Project Context
- This repository is split into a Rust backend under `server/` and a Vue.js frontend under `front/`.
- Keep subdirectory-specific rules in the nearest `AGENTS.md` file and treat this root file as the shared baseline.
- Prefer small, local changes over broad rewrites.

## Language Rules
- Write code, identifiers, doc comments, and inline comments in English.
- Keep developer-facing strings in English unless an external requirement demands another language.
- Interact with the user in the user's language.

## Documentation Hygiene
- Keep existing comments and documentation accurate when behavior changes.
- Keep inline comments rare and only use them for non-obvious logic.
- Update or remove stale comments as part of the same change.
- Create analysis files under `docs/dev-tracking/` by default when requested.
- Keep `docs/TODO.md` current when work adds, completes, renames, or invalidates tracked follow-up items.
- Append relevant user-visible or structural changes to `CHANGELOG.md` instead of rewriting older entries.

## When to Stop and Ask
Before implementing, provide an analysis and wait for user confirmation if the request involves:
- a large change in scope or behavior;
- a notable refactor;
- a structural change under `server/services/*` or a notable frontend architecture change;
- a behavior contract change;
- a file layout reorganization.

The analysis should cover:
- current state;
- the problem to solve;
- expected impact;
- one or more implementation approaches with tradeoffs when useful.

Small local fixes and non-structural updates can be implemented directly.

## Tests and Validation
- Do not create new tests unless the user explicitly asks for them.
- If tests already exist and are affected, update them as needed.
- Run relevant existing checks after changes when possible.
- Do not introduce new test infrastructure by default.

## Engineering Defaults
- Preserve existing behavior unless the requested change requires otherwise.
- Keep shared backend crates independent from application crates; pass application-specific configuration as parameters instead of hardcoding crate paths.
- Keep errors contextualized.
- Keep backend and frontend contracts aligned when a change touches both sides.
- Keep scraper response shaping in YAML/actions or generic scraper post-processors, not in service facades.
- Do not refactor unrelated code while working on a focused change.
