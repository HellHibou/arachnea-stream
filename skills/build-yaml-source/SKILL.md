---
name: build-yaml-source
description: Create or update an Arachnea service YAML from source URLs using the maintained YAML specifications. Use for streaming catalogues, external video hosters, proxy-list providers, IP-country providers, YAML feasibility reviews, or when the user types `#build-yaml-source`.
---

# Build YAML Source

Create or update a service under `server/services/<group>/` using the current YAML capabilities. Match the requested scope and the site's observed responses; keep source-specific extraction and response shaping in YAML.

## Read the specifications first

Resolve repository paths from the Arachnea checkout root. Read `AGENTS.md`, `server/AGENTS.md`, and any nearer instructions applying to the files being changed.

Always read [the common YAML specification](../../docs/specifications/arachnea-scrapyfy-en.md), then choose the specification matching the service's responsibility:

| Service group | Read | Covers |
|---|---|---|
| `arachnea-stream` | [Stream](../../docs/specifications/arachnea-stream-en.md) | Catalogue operations, exact runtime inputs, navigation, output payloads and player descriptors |
| `arachnea-stream-hoster` | [Stream hoster](../../docs/specifications/arachnea-stream-hoster-en.md) | Recognition, stream extraction, HTML reuse, headers, metadata, terminal errors and iframe fallback |
| `arachnea-proxies` | [Proxies](../../docs/specifications/arachnea-proxies-en.md) | `list_proxies_for_country`, country evidence, output rows and protocol selection |
| `arachnea-ip-countries` | [IP countries](../../docs/specifications/arachnea-ip-countries-en.md) | `resolve_ip_country`, `resolve_current_country` and country output requirements |

For a catalogue handing playback to external hosters, also read the hoster guide. For a hoster, consult the stream guide's player descriptor and public playback sections when needed. Do not assume catalogue queries or inputs apply to provider or hoster groups.

Use English specifications as the working reference; the adjacent `-fr.md` documents are their French translations. Follow their sections on authoring, complete examples, defaults, execution order and validation. Do not maintain a separate action catalogue or schema in this skill.

The specifications are the authoring contract. Ordinary YAML work must not require reading Rust to discover fields, query names or supported capabilities. Existing service YAML demonstrates provider-specific patterns; it does not override the contract. Do not use development-tracking documents as a YAML reference.

If observed behavior contradicts a specification, or a required capability is undocumented, identify the precise gap. Inspect only the relevant implementation or existing checks to establish the behavior, then correct the owning specification in English and French within the authorized scope. Do not silently invent syntax or make a source-code lookup a prerequisite for future authors.

## Analyze and implement

1. Follow the common specification's [extraction strategy](../../docs/specifications/arachnea-scrapyfy-en.md#select-an-extraction-strategy) and [authoring procedure](../../docs/specifications/arachnea-scrapyfy-en.md#authoring-procedure), then the selected group's procedure. Inspect the supplied URLs and representative responses before editing.
2. Use the existing-service examples linked from the group guide only when relevant to the target. Follow the specifications for technical decisions and capability limits; do not reconstruct a separate checklist in this skill.
3. Implement the requested scope. Complete work already authorized by the user. Report useful additions outside that scope with their supporting URLs or payloads and whether they are expressible in YAML; do not turn every optional field into an approval stop.
4. Keep code identifiers, comments and developer-facing strings in English. Update relevant specifications, tracked follow-ups and the changelog when required by repository instructions. Preserve unrelated files and manifest activation decisions.

## Validate and report

Follow [service validation](../../docs/specifications/arachnea-scrapyfy-en.md#validate-a-service) in the common specification and the selected group's checks. For stream catalogue commands or contract edits, read [the source-check guide](../../docs/specifications/arachnea-stream-source-checks-en.md).

Run relevant existing checks. Add new tests or test infrastructure only when explicitly requested, in accordance with repository instructions. Report the changed files, implemented capabilities, manifest/activation decisions, checks actually run and remaining verification limits, using the validation levels defined in the common specification.

If satisfying the request needs an unsupported extraction primitive, new public contract or structural backend change, present the current state, gap, impact and a scoped approach before implementing that code, following `AGENTS.md`. Reuse approval already given for that scope. When code changes remain outside authorization, report `Supported in YAML now`, `Optional additions`, and `Blocked or needs code`, stating which items need code and why.
