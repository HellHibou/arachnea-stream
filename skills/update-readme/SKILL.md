---
name: update-readme
description: Maintain all Arachnea README.md files by updating Cargo commands from repository-local Cargo projects and `.cargo/config.toml` aliases, the NPM commands section from repository-local `package.json` scripts, Rust crate summaries, and the list of project-related Codex skills. Keep README text in English and interact with the user in their language. Run in batch mode with a start and end status message. Use when asked to update README.md, when Cargo aliases or npm scripts change, when Rust crates or project skills are added or removed, or when the user types `#update-readme`.
---

# Update README

## Overview
Keep repository README files in English and in sync with current repository-local Cargo projects and aliases, repository-local npm scripts, Rust crate summaries, and project-related Codex skills. Ensure any updated or added text is English. Always interact with the user in their language. Treat `#update-readme` as an explicit command to run this skill. Ensure the Skills list in the root README uses the `#`-prefixed trigger format (for example, `#update-readme`). Run in batch mode: print a start message, do the work without conversational back-and-forth, then print a final status message.

## Workflow
1. Read every repository-authored `README.md`, repository-local `Cargo.toml` files, adjacent `.cargo/config.toml` files, and repository-local `package.json` files.
2. Ensure all updated or added README text is in English.
3. Ensure all interactions with the user are in the user's language. 
4. The technical sections (`Project Structure`, `Rust Crates`, `Cargo Commands`, `NPM Commands`) live in `docs/BUILDING.md`; update them there when the discovered repository state differs. The root `README.md` only keeps the `Skills` section.
5. Enumerate repository-local Cargo projects only. Exclude `target`, build output directories, and global or system paths.
6. For each Cargo project, ensure entries for `cargo build` and `cargo run`: use plain commands at the repository root, or `cd <dir> && cargo ...` for subdirectories. For virtual workspaces, include `-p <package>` and `--bin <binary>` when needed to make run commands unambiguous.
7. For each alias under a nearby `[alias]` table, add `cargo <alias>` at the repository root, or `cd <dir> && cargo <alias>` for subdirectories, with a one-line description.
8. Prefer descriptions from comments immediately above each alias in `.cargo/config.toml`. If missing, derive a concise description from the underlying command.
9. Update crate-level README files next to repository-local Cargo packages.
10. For each crate README, keep a concise human-readable summary of the crate's responsibilities, source layout, and important ownership notes.
11. For Cargo workspace members, keep the crate README aligned with the package name, package path, and dependency role in the workspace.
12. Update the `NPM Commands` section.
13. Enumerate repository-local `package.json` files only. Exclude `node_modules`, build output directories, and global or system paths.
14. For each npm project, ensure an install command entry: `npm install` at the repository root, or `cd <dir> && npm install` for subdirectories.
15. For each script under `scripts`, add `npm run <script>` at the repository root, or `cd <dir> && npm run <script>` for subdirectories, with a one-line description.
16. Prefer conventional descriptions for common npm scripts such as `dev`, `build`, `preview`, `lint`, `test`, and `type-check`. Otherwise derive a concise description from the script command.
17. Update the `Skills` section.
18. Enumerate skills under `skills/` in this repository only. Do not include global or system skills from `$CODEX_HOME`.
19. For each skill, read `SKILL.md` frontmatter `name` and `description`.
20. List skills as `- #<name> - <short description>`, using the first sentence of the description and keeping it concise.
21. Preserve other README sections and formatting; avoid unrelated edits.
22. End by printing localized messages in the user's language: `✓ <Success message>` on success, `✗ <Error message>: <DETAIL>` on error, and `✓ <Nothing to update message>` when there is nothing to change.
