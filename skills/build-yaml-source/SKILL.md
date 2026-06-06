---
name: build-yaml-source
description: Create or update one Arachnea scraper YAML from one or more source URLs by analyzing the target website, reusing patterns from `server/services/*.yaml`, and checking supported scraper capabilities in `server/crates/arachnea-scrapyfy/src/scrapyfy/*`. Use when adding a new source, repairing or extending an existing source YAML, auditing whether a site can be supported without Rust changes, or when the user types `#build-yaml-source`.
---

# Build YAML Source

## Overview

Create or update `server/services/<source>.yaml` with a YAML-first approach. Analyze the target site from the provided URLs, reuse the closest existing source configs, and avoid Rust changes unless the current scraper primitives cannot express the source.

## Workflow

1. Read `AGENTS.md` and `server/AGENTS.md`.
2. Read [references/query-contracts.md](references/query-contracts.md) and [references/yaml-capabilities.md](references/yaml-capabilities.md).
3. Inspect the provided URLs before touching files:
   - identify whether the source is HTML, direct JSON, Next.js `__NEXT_DATA__`, or JSON fetched through follow-up requests;
   - locate stable search, entry, home, category, season, and player URLs when they exist;
   - prefer network payloads or embedded JSON over brittle CSS scraping when both are available.
4. Reuse the closest existing examples instead of starting from scratch:
   - `server/services/papystreaming.yaml` for plain HTML pages;
   - `server/services/rtlplay-be.yaml` for Next.js `__NEXT_DATA__`;
   - `server/services/rtbf-auvio-be.yaml` for JSON APIs and `sub_queries`;
   - `server/services/anime-sama.yaml` for regex-heavy extraction or `post_process`.
5. Draft the smallest useful YAML first:
   - prefer `search` and `get_entry` as the baseline;
   - add `load_home`, `get_category`, and `get_season` only when the site exposes them cleanly;
   - keep identifiers, comments, and added text in English.
6. Before adding optional source-specific content, stop and report it. For each proposed addition, state:
   - what can be added;
   - which URLs or payloads prove it exists;
   - whether it is YAML-only or needs Rust changes.
7. If the site needs a capability not already supported by `server/crates/arachnea-scrapyfy/src/scrapyfy/*`, stop before editing Rust. Explain the missing capability, point to the closest code area, and ask for approval before modifying source code.
8. After the minimal scope is agreed, create or update the YAML and keep backend contracts aligned with the current query names and field conventions.

## Mandatory Report

Before any Rust change, and before any optional addition beyond the minimal YAML, present three short sections:

- `Supported in YAML now`
- `Optional additions`
- `Blocked or needs code`

Under `Optional additions` and `Blocked or needs code`, explicitly say whether code changes are required.

## Resource Use

Read the references first, then load only the specific config and code files that match the site being analyzed. Do not read every source YAML unless the site shape is still unclear after comparing the closest examples.
