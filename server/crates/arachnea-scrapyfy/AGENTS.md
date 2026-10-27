# Arachnea Scrapyfy Agent Notes

- This crate owns the generic scraper engine and YAML-driven query execution.
- Prefer generic actions, post-processors, and YAML configuration over source-specific Rust logic.
- Keep scraper response shaping out of stream service facades.
- If parsing rules, placeholders, actions, or expected fields change, update the relevant files under `server/services/` or `server/data-test/` in the same change.
- Follow the shared Rust source, constant, and test-layout rules in `../../AGENTS.md`. Preserve the existing `test-support` feature and helper import paths consumed by `arachnea-stream` tests, including `scrapyfy::scraper_manager::tests::test_query`; when migrating helpers to `tests/support/`, retain their conditional loading for both local tests and dependent crates enabling `test-support`.
