# Arachnea Scrapyfy Agent Notes

- This crate owns the generic scraper engine and YAML-driven query execution.
- Prefer generic actions, post-processors, and YAML configuration over source-specific Rust logic.
- Keep scraper response shaping out of stream service facades.
- If parsing rules, placeholders, actions, or expected fields change, update the relevant files under `server/services/` or `server/data-test/` in the same change.
