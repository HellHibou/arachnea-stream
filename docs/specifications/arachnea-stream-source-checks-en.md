# Stream service test YAML: authoring and execution

This is the complete authoring guide for **schema version 1**. It describes the
implemented loader and runner. It is intended for humans and AI agents.
Renaming this guide does not change `schema_version: 1`.

## 1. Commands: run the tests

Run every command in this section **from the repository root**. Cargo must be
installed and on `PATH`. The existing Rust tests perform discovery and execution;
there is no JavaScript launcher, additional binary or Cargo alias to install.

### One Rust entry point; selection through the environment

Use `stream_scraper::tests::test_all_services` for a full integration scenario.
`ARACHNEA_TEST_QUERY_SOURCE` controls its scope:

| Environment value | Source selection |
| --- | --- |
| Variable not defined | All production YAML files discovered recursively |
| Exactly `*` | All production YAML files discovered recursively |
| Production service ID, such as `anime-sama` | Only that service |
| Services-relative path, such as `arachnea-stream/dark-stream/anime-sama.yaml` | Only that configuration |
| Empty, whitespace-only, non-Unicode, unknown or ambiguous selection | Error; never silently expands to all sources |

For **one service**, execute:

```sh
ARACHNEA_TEST_QUERY_SOURCE=anime-sama \
  cargo test --manifest-path server/Cargo.toml -p arachnea-stream --lib \
  stream_scraper::tests::test_all_services -- --exact --ignored --nocapture
```

Replace `anime-sama` with the production YAML's `id` or its services-relative path.
This executes all contract steps for that source, including declared navigation.

For **all sources**, leave `ARACHNEA_TEST_QUERY_SOURCE` undefined and execute:

```sh
cargo test --manifest-path server/Cargo.toml -p arachnea-stream --lib \
  stream_scraper::tests::test_all_services -- --exact --ignored --nocapture
```

To explicitly select all sources even if a selector was previously set, use the
literal wildcard. Quote `*` as shown:

```sh
ARACHNEA_TEST_QUERY_SOURCE='*' \
  cargo test --manifest-path server/Cargo.toml -p arachnea-stream --lib \
  stream_scraper::tests::test_all_services -- --exact --ignored --nocapture
```

These are live integration commands. The test is ignored by ordinary `cargo test`;
"all by default" applies only when this integration test is explicitly executed.
From `server/`, the same commands work after omitting
`--manifest-path server/Cargo.toml`.

Rust recursively scans `server/services/arachnea-stream/` for `.yaml` files.
This includes disabled sources and files absent from `services.json`. No list of
provider paths is maintained. The scan rejects symbolic links.

Each source is prepared independently before provider requests. A missing test
contract is reported as `not_tested` with `coverage_missing` as its reason; an
invalid contract is reported as a source failure. **Other valid services still
run.** Do not silently skip missing contracts or derive expectations from production.
The final summary lists every selected service and its reason. Cargo reports an
aggregate failure only after the remaining services finish if any source failed
or was not tested, preserving the incomplete-coverage signal.
On 2026-10-30, the inventory was 21 configurations and nine contracts: the other
12 contracts were intentionally deferred. That is an inventory snapshot, not a
fixed limit. Select one covered service while authoring one missing contract.

### Check one contract through its metadata operation

```sh
ARACHNEA_TEST_QUERY_SOURCE=anime-sama \
  cargo test --manifest-path server/Cargo.toml -p arachnea-stream --lib \
  stream_scraper::tests::test_query_service_stream_metadata -- --exact --nocapture
```

This loads and validates the **whole selected contract**, checks its production
capabilities, then executes metadata only. It does not execute the navigation,
search or entry steps. Existing production metadata queries use
`scraper_type: static`; verify this before treating a new source's metadata check
as a local, no-provider check. This is not a parser-only command.

To validate all checked-in contracts and run their metadata, leave
`ARACHNEA_TEST_QUERY_SOURCE` undefined or set it to `*`, then run the same Cargo
metadata command.
It discovers contracts recursively under `tests/fixtures/services/` in the Stream
crate. It does **not** detect production YAML files that have no contract; use
the full `test_all_services` integration command with no selector or `*` for that
coverage check.

### Run one live operation and its prerequisites

```sh
ARACHNEA_TEST_QUERY_SOURCE=anime-sama \
  cargo test --manifest-path server/Cargo.toml -p arachnea-stream --lib \
  stream_scraper::tests::test_query_search -- --exact --ignored --nocapture
```

Use exactly one of these existing test names:

| Requested check | Full Cargo test name | Requires `--ignored`? |
| --- | --- | --- |
| Metadata | `stream_scraper::tests::test_query_service_stream_metadata` | No |
| Home | `stream_scraper::tests::test_query_load_home` | Yes |
| Search | `stream_scraper::tests::test_query_search` | Yes |
| Entry/detail | `stream_scraper::tests::test_query_get_entry` | Yes |
| Full integration scenario, one source or all according to the selector | `stream_scraper::tests::test_all_services` | Yes |
| Compatibility full-scenario entry point, same selector rules | `stream_scraper::tests::test_service` | Yes |

Category, section and player operations have no dedicated test function. Execute
them through the full contract or as dependencies of an entry check. Do not invent
Cargo test names: a filter matching zero tests does not validate anything.
The filtered live operations use the same source-selection table. Set a source
ID to avoid selecting all production configurations. Prefer `test_all_services`
over the compatibility `test_service` name, and use `--exact` to avoid executing
both full-scenario entry points.

### Run routine local tests

```sh
cargo test --manifest-path server/Cargo.toml -p arachnea-stream --all-features
cargo test --manifest-path server/Cargo.toml --workspace
```

The five live integration entry points are ignored by default. Routine commands
need no `--skip` list and do not qualify live source health. Keep the ignored count
in summaries. `--offline`, if added to Cargo, affects dependency access only;
it does **not** disable provider HTTP requests when a live test is selected.

Cargo reports the existing Rust test's status directly: success returns 0 and a
test failure normally returns 101. No wrapper remaps this result. Inspect the
source-selection diagnostics and final test result, not just the process status.

## 2. Authoring procedure: follow these steps in order

1. Read the intended file under `server/services/arachnea-stream/`. Record its
   exact `id`, services-relative path and query names under `queries`.
2. Inspect its public output shapes, including production `type` declarations
   and actual responses when available. Include relevant production post-processing,
   not only the original entry declarations. Do not assume that every provider has
   search, categories, sections, detail pages or the same field types.
3. Create exactly one contract at the mirrored path described in section 3.
   Do not create or modify production YAML to make the test pass as part of
   contract authoring. If production needs a fix, report it separately.
4. Choose supported operations to check. Always include a metadata step and at
   least one content operation. Describe a real useful path through the source.
5. Write steps in dependency order. Choose small useful-data minima and only
   genuinely indispensable fields. Add optional-field constraints deliberately.
6. Check every rule in the schemas below. Do not invent keys or operations.
7. Run the metadata command for that source. Fix configuration errors first.
8. Run the full `test_all_services` integration command with
   `ARACHNEA_TEST_QUERY_SOURCE` set to the intended service ID. Inspect every step's counts,
   invalid fields, errors and coverage gaps. A metadata-only pass is insufficient.
9. Report the exact command, source, input references and observed result. Keep
   access limitations and contract failures visible. Do not relax assertions
   merely to obtain a green result. Update the changelog and coverage follow-ups
   in `docs/TODO.md` when required by the repository's `AGENTS.md` instructions.

For production-format and public-output details, consult the
[Scrapyfy YAML specification](arachnea-scrapyfy-en.md) and
[Stream specification](arachnea-stream-en.md). They describe production behavior;
the allowed test-file keys are defined only in this guide.

**For AI agents:** author only the requested service contract(s), plus required
documentation/changelog/tracking updates. Keep all source-specific terms, URLs,
field paths and navigation in the YAML. Do not add provider-specific Rust branches,
a new test function, a new framework or a maintained provider list. Do not create
all remaining contracts unless requested. Never claim that an unexecuted live
step passed.

## 3. File placement and identity

These four values must agree:

| Item | Example |
| --- | --- |
| Production file | `server/services/arachnea-stream/legal-stream/m6play-fr.yaml` |
| Production `id` and contract `source` | `m6play-fr` |
| Contract `source_config` | `arachnea-stream/legal-stream/m6play-fr.yaml` |
| Test file | `server/crates/arachnea-stream/tests/fixtures/services/arachnea-stream/legal-stream/m6play-fr.yaml` |

Rules:

- `source_config` starts relative to `server/services/`, not the repository root,
  the Stream crate or the fixture directory. Include `arachnea-stream/`.
- Preserve every subdirectory below `server/services/` in the fixture path.
  Do not put contracts directly under `tests/fixtures/services/`.
- The filename is `<source>.yaml`; its stem must equal the production `id` and
  the contract's `source` exactly. IDs must be unambiguous for ID-based selection.
- Use `.yaml`, not `.yml` or `.json`. Absolute paths, `.`/`..` path components and
  configurations escaping the services root are rejected. Directory discovery
  rejects symbolic links; do not use them for production or test files.
- The historical `server/data-test/` directory is not used. Do not recreate it.

The production file defines **how to scrape**. The contract defines **what useful
output must remain available for chosen reference inputs**. Read production to
understand the available operations and shapes, but do not mechanically copy all
production field names into expectations. The two documents have different schemas. Production keys such as `queries`,
`scraper_type`, `row_selector`, `entries` and `actions` are not valid top-level
keys in a test contract. Names such as `entries` can still occur as path segments.

## 4. Complete working example

The following is the existing M6 contract. It exercises metadata, home navigation,
a category, a deferred section, search and a detail page. It is a complete file,
not a fragment. Its source identifiers, search term and paths belong to M6.

**When authoring another service, do not copy these values unchanged.** Replace
the identity and choose the operations, reference inputs and paths from that
service's actual contract. For example, a source without search must not inherit
M6's `search` step.

Production file: `server/services/arachnea-stream/legal-stream/m6play-fr.yaml`.
Test file: `server/crates/arachnea-stream/tests/fixtures/services/arachnea-stream/legal-stream/m6play-fr.yaml`.

```yaml
# Reference expectations; live qualification is recorded separately.
schema_version: 1
source: m6play-fr
source_config: arachnea-stream/legal-stream/m6play-fr.yaml

capabilities:
  expected: ["service_stream_metadata","load_home","get_category","get_section","search","get_entry"]
  not_expected: []

steps:
  - id: metadata
    operation: service_stream_metadata
    expect:
      items_path: []
      min_count: 1
      min_valid_count: 1
      required_non_empty:
        - ["id"]
        - ["title"]
        - ["logo"]

  - id: home
    operation: load_home
    expect:
      items_path: ["categories"]
      min_count: 1
      min_valid_count: 1
      required_non_empty:
        - ["label"]
        - ["request","query_url"]

  - id: category
    operation: get_category
    inputs:
      query_url:
        from_step: home
        items_path: ["categories"]
        select: first_valid
        value_path: ["request","query_url"]
    expect:
      items_path: ["sections"]
      min_count: 1
      min_valid_count: 1
      required_non_empty:
        - ["label"]
        - ["link"]

  - id: section
    operation: get_section
    inputs:
      query_url:
        from_step: category
        items_path: ["sections"]
        select: first_valid
        value_path: ["link"]
    expect:
      items_path: ["entries"]
      min_count: 1
      min_valid_count: 1
      required_non_empty:
        - ["title"]
        - ["link"]
      optional_fields:
        - path: ["web-link"]
          type: string

  - id: search
    operation: search
    inputs:
      query: "journal"
    expect:
      items_path: ["entries"]
      min_count: 1
      min_valid_count: 1
      required_non_empty:
        - ["title"]
        - ["link"]
      optional_fields:
        - path: ["description"]
          type: string
        - path: ["web-link"]
          type: string

  - id: entry
    operation: get_entry
    inputs:
      query_url:
        from_step: section
        items_path: ["entries"]
        select: first_valid
        value_path: ["link"]
    expect:
      items_path: []
      min_count: 1
      min_valid_count: 1
      required_non_empty:
        - ["title"]
      optional_fields:
        - path: ["description"]
          type: string
```

A simpler service may need only metadata plus search, or metadata plus home and
entry. A category-based service needs category navigation when home contains
only category descriptors. The exact useful route is a per-service decision.
The following sections are the complete allowed schema; smaller snippets below
replace the indicated part of a file and are not standalone contracts.

## 5. YAML and top-level schema

Use one YAML document. Use real numbers/booleans for numeric/boolean fields and
strings for IDs, operations, input text and path segments. Quote ambiguous text
such as `"true"`, `"123"` or `"*"` when a string is intended. Empty values such as
`query:` are null, not valid strings. Prefer `[]` or `{}` for explicit empty
collections when the schema allows them.

Unknown keys, duplicate mapping keys, non-string mapping keys, implicit scalar
coercion and explicit nulls are rejected. Do not use YAML tags or rely on tags to
convert types; the strict reader rejects unsupported tagged values. Do not add
comments-as-fields such as
`description`, `enabled`, `cache`, `retries`, `skip` or `nullable` to the schema.
Ordinary YAML comments are permitted. Omit an optional key to use its default.

| Top-level key | Required? | Type and exact rule |
| --- | --- | --- |
| `schema_version` | Yes | Integer, exactly `1` |
| `source` | Yes | Nonblank string equal to production `id` and filename stem |
| `source_config` | Yes | Existing services-relative `.yaml` path, mirrored by the test file |
| `capabilities` | Yes | Mapping containing both lists described in section 6 |
| `steps` | Yes | Ordered list of 1–32 step mappings, including metadata |
| `limits` | No | Mapping from section 11; defaults to 60 seconds per step and 300 per source |
| `prerequisites` | No | List from section 11; defaults to `[]` |

No required field is inferred from production or filled from another source.

## 6. Capabilities: expected, not expected and uncovered

```yaml
capabilities:
  expected: [service_stream_metadata, load_home, get_entry]
  not_expected: [search]
```

This fragment is valid only if production declares the three expected operations
and does **not** declare `search`.

- Both `expected` and `not_expected` are required lists of distinct nonblank
  operation names. `not_expected: []` is valid; omitting it is not.
- Only the fourteen operation names in section 7 may occur in either list.
- The lists cannot overlap.
- `service_stream_metadata` must be in `expected` and must have a step.
- Every operation in `expected` must exist in production and have at least one
  step. Every step's operation must be in `expected`.
- `not_expected` means **absent in production**, not "present but untested" or
  "temporarily broken". If production declares it, validation fails.
  These operations are excluded from the result table and severity totals.
- Production operations in neither list are reported as `coverage_missing`.
  The result table displays `Skip` status with `Warning` severity.
  Do not put operation names absent from section 7 in either list.
- An additional uncovered operation does not invalidate passing declared steps,
  but prevents a complete-capability coverage claim.
- A missing source contract is reported as `not_tested`. Other valid sources run;
  the aggregate remains unsuccessful for incomplete coverage after the final summary.

To finish a full source run successfully, at least one executed **content** step
must pass: any supported operation other than metadata or home. Metadata and
home alone cannot produce a green full source result.

## 7. Steps and supported operations

Each step accepts exactly these keys:

| Step key | Required? | Rule |
| --- | --- | --- |
| `id` | Yes | Unique nonblank string within this file; references use this ID |
| `operation` | Yes | One exact supported name from the table below |
| `inputs` | No | Mapping, default `{}`; only this operation's allowed keys |
| `expect` | Yes | Mapping described in section 10 |

Multiple steps may use the same operation, for example a discovered entry and a
separate reference entry. They must have different step IDs. Each step runs once;
there are no runner retries, loops or automatic pagination.

| Operation | Allowed `inputs` | Public payload roots supplied to assertions | Cache mode |
| --- | --- | --- | --- |
| `service_stream_metadata` | No inputs | Individual metadata objects | `NoCache` |
| `load_home` | No inputs | Individual home objects | `FullCache` |
| `search` | Required `query`: nonblank string or reference | Individual public search group objects, usually containing `entries` | `ClientCache` |
| `get_category` | Required `query_url`: nonblank string or reference; optional `page`: positive integer, default `1` | Individual category objects | `FullCache` |
| `get_section` | Required `query_url`: nonblank string or reference; optional `page`: positive integer, default `1` | One section object | `FullCache` |
| `get_entry` | Required `query_url`: nonblank string or reference | One entry/detail object | `ClientCache` |
| `get_players` | Required `query_url`: nonblank string or reference | One player-result object | `ClientCache` |
| `get_season` | Required `query_url`; optional positive integer `page`, default `1` | One season object (usually `episodes`; Anime-Sama returns resolver/language metadata) | `ClientCache` |
| `list_lives` | No inputs | Individual channel objects, without an `entries` wrapper | `FullCache` |
| `get_live` | Required `query_url`, passed as the facade's channel identifier or URL | One object containing `players` | `ClientCache` |
| `get_banners` | Required `query_url` | One object containing `banners` | `ClientCache` |
| `get_recommendations` | Required `query_url`; optional positive integer `page`, default `1` | One object containing `entries` | `FullCache` |
| `resolve_stream` | Required HTTP(S) `query_url` | One normalized stream object containing `stream_url` | `NoCache` |
| `get_episode` | Required `query_url` | Individual legacy episode query objects, usually containing `players` | `ClientCache` |

`page` must be a literal integer, not a string or a dynamic reference. Do not put
`page` on search: search always runs page 1 with empty media-type/theme filters.
URLs and internal query identifiers are passed as source-defined strings; the
test format does not require all `query_url` inputs to be HTTP URLs.

The additional operations preserve the existing `inputs` structure and literal/
reference forms. `get_live` deliberately uses `query_url` for the typed facade's
`channel` argument; do not add an input named `channel`. `page` is allowed only on
`get_category`, `get_section`, `get_season` and `get_recommendations`.
`list_lives` remains isolated because the prepared facade contains one source.
`get_episode` has no dedicated Stream facade: its adapter invokes only the
selected source's legacy query through the aggregator, with runtime parameters
and the same envelope checks. All other operations use typed facade methods.
`resolve_stream` uses the `scraper-query` resolver for the selected source, with
no country hints and manifest rewriting disabled. It validates the normalized
stream descriptor, not playback, DRM acquisition or segment availability.
Source-specific resolver parameters and `source_params` remain unsupported.


The adapter chooses the source from the contract. For `get_category`, it builds
the typed descriptor `{name: source, query_url: resolved_value}` and passes empty
extra source parameters. Section, detail and player calls also target the same
source. Do not add `source`, `name`, `channel`, `source_params`, request headers,
credentials, media filters or arbitrary parameters to `inputs`. If the production
operation requires an unsupported input, report the limitation instead of
inventing a key or adding a provider exception to Rust.

Assertions examine the public serialized `data`, **not** the envelope, internal
`ScraperDataNode.values/children/items` or a reconstructed response. Do not start
paths with `data`. Vector responses are separated into individual root objects;
singleton responses stay as one root. Search keeps its public group object:
use `items_path: [entries]`, not `[]` to count search results.

Every facade envelope must be error-free, even if it contains useful partial
data. A returned `source` field must identify the selected source. Metadata
must identify exactly one selected source and advertise the loaded capabilities.
An unrelated source or fallback result cannot satisfy the contract.

## 8. Path syntax: keys and explicit array traversal

A path is a YAML list of strings, not JSONPath and not a slash-delimited string.

| Path | Meaning |
| --- | --- |
| `[]` | Each public root object itself |
| `[entries]` | Items of the root's `entries` array |
| `[categories]` | Items of the root's `categories` array |
| `[sections, "*", entries]` | Entries from all root sections |
| `[sections, "*", subsections, "*", entries]` | Entries from explicitly nested subsections |
| `["img/poster", link]` | Literal key `img/poster`, then key `link` |
| `[request, query_url]` | Key `query_url` in the `request` object |

Traversal rules are exact:

1. Start at each public root.
2. A literal segment reads one object key. It never implicitly traverses arrays.
3. `"*"` expands one array level. Quote it to avoid YAML alias syntax.
4. At the final `items_path` location only, expand an array once into its items.
   A final non-array value is counted as one selected item.
5. A missing key contributes zero selected values. An existing value of the wrong
   intermediate type is a shape error, even if other branches yield enough data.
6. Selected items must be objects to be valid. Scalars, nulls and nested array
   items still count toward `total`, but are invalid objects.

Example public payload:

```json
{
  "sections": [
    {"entries": [{"title": "First", "link": "entry-1"}]},
    {"entries": [{"title": "Second", "link": "entry-2"}]}
  ]
}
```

`items_path: [sections, "*", entries]` selects two objects. Their required field
paths `[title]` and `[link]` are relative to each entry, not to the root.
`[sections, entries]` is wrong because `sections` is an array.

`/`, `>`, dots and brackets inside a literal key are not separators. An empty
path is allowed; a blank/whitespace-only segment is not. Only `items_path` allows
wildcards. Required, optional and input-value paths use literal keys only.
There is no recursive wildcard, numeric array indexing, JSONPath expression or
automatic subsection descent.

## 9. Inputs: literals and dependencies

### Literal inputs

A search term must be deliberate and source-specific:

```yaml
inputs:
  query: "journal"
```

An entry can use a maintained reference instead of discovery:

```yaml
inputs:
  query_url: "https://example.invalid/reference-entry"
```

The latter URL is illustrative and must be replaced with a verified reference
for the source. Do not interpret a disappeared reference as a successful test.
Literal inputs should be concrete strings: the test runner itself does not expand
production placeholders or environment-variable syntax in the test document.
Do not assume that `{base_url}`, `${VARIABLE}` or a YAML `value` field will be
resolved as a reference input.

### Dynamic input: all four reference keys are required

```yaml
inputs:
  query_url:
    from_step: home
    items_path: [categories]
    select: first_valid
    value_path: [request, query_url]
```

| Reference key | Exact rule |
| --- | --- |
| `from_step` | ID of a step appearing earlier in this same file |
| `items_path` | Must equal that upstream step's `expect.items_path` exactly |
| `select` | Exactly `first_valid`; no other strategy exists |
| `value_path` | Literal path relative to a selected upstream object; must resolve to a nonblank JSON string at runtime |

Missing, forward, self and cyclic dependencies are invalid before network calls.
Do not use a production operation name as `from_step` unless that is also the
actual upstream step ID.

`first_valid` scans upstream selected objects in response order and chooses the
first object satisfying all upstream per-object assertions **and** possessing the
requested nonblank string. It does not simply choose the first item. It does not
try another candidate after the downstream request fails. The entire upstream
step must have passed; otherwise its dependents are blocked, even if some data
were available. A passing upstream step with no usable input value also blocks
the consumer and reports the missing value path.

### Choose a navigation recipe from the actual output

| Source shape | Steps and item paths | Next input's `value_path` |
| --- | --- | --- |
| Home has direct entries | `home: [sections, "*", entries]` → `entry: []` | `[link]` from home |
| Home has categories with content behind them | `home: [categories]` → `category: [sections, "*", entries]` → `entry: []` | `[request, query_url]` from home, then `[link]` from category |
| Category has deferred sections | `home: [categories]` → `category: [sections]` → `section: [entries]` → `entry: []` | `[request, query_url]`, then `[link]`, then `[link]` |
| Home has deferred sections | `home: [sections]` → `section: [entries]` → `entry: []` | `[link]` from home, then `[link]` from section |
| Explicit nested subsections contain entries | `home: [sections, "*", subsections, "*", entries]` → `entry: []` | `[link]` from home |

These are recipes, not universal provider guarantees. If a selected path is absent
or an internal identifier has a different meaning, use the actual source shape.
A `load_home` step whose item path starts with `categories` must have a dependent
content step, directly or transitively. Static navigation descriptors alone do
not demonstrate useful content extraction. The M6 example shows the deferred
category/section route without any Rust provider branch.

## 10. Assertions and useful-data counts

Every `expect` accepts exactly these fields:

| Key | Required? | Type, default and rule |
| --- | --- | --- |
| `items_path` | Yes | List of string segments; may be `[]`; explicit wildcards allowed |
| `min_count` | Yes | Positive integer; zero is invalid |
| `min_valid_count` | Yes | Positive integer, no greater than `min_count` |
| `required_non_empty` | Yes | Nonempty list of distinct literal field paths |
| `optional_fields` | No | List of mappings described below; defaults to `[]` |
| `all_valid` | No | Boolean, default `false` |

Recommended starting values are `min_count: 1`, `min_valid_count: 1` and omitted
`all_valid`. Never require an exact live catalog size, stable editorial ordering
or full title text without a specific justified need.

### Required fields

```yaml
expect:
  items_path: [entries]
  min_count: 1
  min_valid_count: 1
  required_non_empty:
    - [title]
    - [link]
```

A required value is invalid when missing, null, an empty/whitespace-only string,
an empty array or an empty object. Numeric zero and boolean false are nonempty.
This assertion checks presence/nonemptiness; it does **not** require a string.

At least one required path must be nonempty and different from `[source]` and
`[id]`. For metadata, `[title]` is useful. For an entry, `[title]` may be the minimum.
For navigation, require the actual next link or query input. Root identity alone
must never serve as proof of content extraction. `[]` may additionally check the
selected object itself, but does not satisfy this useful-field requirement.

### Optional fields and types

```yaml
optional_fields:
  - path: [description]
    type: string
  - path: ["img/poster", link]
    type: string
    non_empty: true
```

Every optional-field mapping requires `path` and `type`. `non_empty` is an optional
boolean defaulting to `false`. Allowed types are exactly `string`, `number`,
`boolean`, `object` and `array`. There is no type coercion, nullable type or union.

An absent optional key is valid. An existing intermediate value of the wrong
type is not absence: for example `img/poster: null` cannot supply a nested `link`.
When the terminal field exists, it must match the declared type, including when
its value is null:

| Public value | Optional `type: string` | Also `non_empty: true` |
| --- | --- | --- |
| Key absent | Valid | Valid |
| `null` | Invalid | Invalid |
| `""` or `"   "` | Valid | Invalid |
| `"A description"` | Valid | Valid |
| `42`, `false`, `[]`, `{}` | Invalid | Invalid |

This distinction matters: the qualified RTL Play search returned `description:
null` for two items, so its optional string constraint failed. Do not remove the
constraint, convert null into absence or weaken the schema just to hide such a
failure. Report whether the expectation, public output or supported semantics
needs a separately justified correction.

A path cannot be duplicated within required fields, within optional fields, or
across both. Do not declare the same path required and optional to add a required
type check: schema v1 has no separate required-type assertion. New undeclared
fields do not fail a check. Optional fields need not appear uniformly on rows.

### Counting and success

Each selected item contributes once to `total`. An object contributes to `valid`
only if every required and present-optional assertion passes. Everything else
contributes to `invalid`.

A step passes only when all of these hold:

1. There is no envelope or shape error.
2. `total >= min_count`.
3. `valid >= min_valid_count`.
4. If `all_valid: true`, `invalid == 0`.

With minima of one, three items containing one valid object can pass when
`all_valid` is false; the two invalid objects remain visible in diagnostics.
With zero extracted items, a step always fails. Wrong intermediate shapes remain
failures even if another branch provides enough objects.

## 11. Optional limits and prerequisites

Omit both sections when unnecessary. To set them explicitly:

```yaml
limits:
  step_timeout_seconds: 60
  source_timeout_seconds: 300
prerequisites:
  - env: EXAMPLE_ACCESS_READY
    reason: "This source requires an independently configured access prerequisite"
```

The environment name and reason above are examples, not a built-in authentication
mechanism. Replace them with a real prerequisite only if needed.

| Limit | Default | Allowed values |
| --- | --- | --- |
| Number of steps | No implicit steps added | 1–32, including metadata |
| `step_timeout_seconds` | 60 | Integer from 1 to 120 |
| `source_timeout_seconds` | 300 | Integer from the step timeout to 900 |

Every prerequisite has exactly `env` and `reason`, both nonblank strings.
Environment names cannot repeat. The runner requires a present, nonblank Unicode
value, but never logs that value. A prerequisite does not inject credentials,
configure a proxy, log in or prove that access works. Missing prerequisites fail
as `prerequisite_missing`; they are never counted as successful retrieval.

Execution is sequential. At most 32 scenario operations run per source; the
preserved metadata assertions additionally check empty/unknown home selections
without dispatching provider queries. The contract does not cap internal HTTP
fan-out, redirects or transport retries. There are no YAML retry options.
Cache shutdown has a separate five-second bound and can extend cleanup beyond a
request/source deadline. Cancelling a step does not guarantee immediate termination
of already detached transport work.

## 12. Diagnostic input overrides

The Rust tests read these environment variables directly. Remove search/detail
diagnostic overrides when qualifying the unchanged reference contract.

| Variable | Effect |
| --- | --- |
| `ARACHNEA_TEST_QUERY_SOURCE` | Source selector: undefined or `*` selects all; an ID or services-relative path selects one. Metadata checks apply the all selection to existing fixture contracts only |
| `ARACHNEA_TEST_SEARCH_TERM` | Replaces `query` for executed search steps |
| `ARACHNEA_TEST_ENTRY_URL` | Replaces `query_url` for executed entry/detail steps |

Blank or non-Unicode diagnostic values are invalid. An override without a matching
executed operation is also invalid. Filtered runs assess relevance after resolving
the effective step selection. Overrides are labelled diagnostic and use masked
reference fingerprints in output.

A replaced reference removes only that input's dependency. Thus a filtered entry
check with an explicit URL can run without its discovery steps. A full service
run still executes its other declared steps. Overrides never waive envelope or
field assertions, never change the source and do not edit the contract file.

## 13. Read failures and report coverage honestly

| Outcome | Meaning and next action |
| --- | --- |
| `passed` | This step's operation and declared assertions passed; inspect remaining source steps and coverage gaps |
| `invalid_configuration` | Fix the YAML, identity, inputs, capabilities, references or limits before interpreting a source-health result |
| `coverage_missing` | A missing contract causes that service to be `not_tested`; other services continue. Extra uncovered operations do not invalidate passing declared steps |
| `not_tested` | Source-level result for an absent contract; no calls for that source, with the reason retained in the final summary |
| `prerequisite_missing` | Supply the independently required access setup; no retrieval success is claimed |
| `retrieval_failed` | Typed transport failure or timeout; preserve the contextual cause |
| `extraction_failed` | No shape error, but fewer items than `min_count`; investigate reference/access/extraction without assuming a cause |
| `contract_failed` | Envelope, shape or object assertions failed; inspect the error, invalid paths and observed JSON kinds |
| `blocked` | Step was not called because its input could not be discovered, an upstream step failed or the source deadline expired |
| `not_applicable` | Operation is explicitly absent in production and is not executed; zero calls is not a passed live control |

Independent steps continue after a failure within the source deadline. Dependents
of a failed step are blocked, even when partial upstream data exists. The source
and aggregate fail on expected-step failures, blocked steps or missing prerequisites.
A full source run without a successful content step cannot be green.

The public envelope does not retain every typed transport cause. An opaque backend
envelope error remains `contract_failed`; do not reclassify it by guessing from an
empty payload. For example, LN24's qualified entry retained a Belgian proxy-route
error despite direct fallback; the runner rejected the envelope instead of
claiming an extraction regression or a successful detail check.

Output includes source/step, operation, durations, paths, minima, counts, invalid
paths, observed JSON kinds, exclusions and uncovered capabilities. `partial_coverage`
is true for operation-filtered runs, uncovered production capabilities, or
sources that could not be prepared (including missing contracts). Filtered
metadata can pass its own assertions without qualifying full source health.
The reference fingerprint and URL origin are reported without credentials,
URL paths, query strings, fragments or response bodies. Do not invent HTTP status,
effective URLs or asserted payload counts when the facade does not expose them.

### Final per-source summary

After preparation and execution, the runner always lists the per-source results
for discovered services that reached the common runner:

| Source outcome | Meaning |
| --- | --- |
| `passed` | The selected source checks passed |
| `failed` | Invalid per-source configuration, prerequisite/execution/assertion failure, or caught Rust panic |
| `not_tested` | No independent contract exists for this source; the message explains the missing contract |
| `not_applicable` | A filtered operation is explicitly not expected; no successful live check is claimed |

`source_check_summary` reports `selected`, `attempted`, `passed`, `failed`,
`not_tested`, `not_applicable` and `partial_coverage`. `attempted` counts sources
whose nonempty operation selection reached execution, excluding preparation
failures and nonapplicable selections. `source_check_result` then lists each
service's outcome and reason. Source results and step outcomes are separate.

After the machine-readable summary, the runner prints one aligned `Source check
results` table grouping every recorded source/operation row: source, operation,
status (`Passed`, `Skip`, `Blocked`, `Failed`), severity (`Ok`, `Warning`, `Error`)
and a message for non-passing rows. `passed` maps to `Passed`/`Ok`;
`coverage_missing` and `not_tested` map to `Skip`/`Warning`: the production
operation or source exists, but its test is not implemented. This also applies to
production operations unsupported by schema v1. Blocked steps map to
`Blocked`/`Error`; other failures and configuration/preflight errors map to
`Failed`/`Error`. Operations explicitly absent through `capabilities.not_expected`
are validated as absent in production, never called, and excluded from table rows
and severity totals. Their list remains informational in diagnostics; an explicitly
requested absent operation retains its source-level `not_applicable` result.
Severity labels use ANSI colors only on an interactive terminal and are
disabled by `NO_COLOR` (or `CLICOLOR=0`). The table is followed by the total row
count and the `Ok`/`Warning`/`Error` counts, then a blank line.

A per-source error does not stop later services. Recoverable Rust panics during
source preparation/execution are recorded as failures and their source state is
discarded. Missing/invalid contracts never cause fabricated requests for that
source. The aggregate remains unsuccessful if any source failed or was not tested;
its final assertion occurs **after** the summary and the remaining source runs.
Shared setup errors, such as an unreadable top-level manifest or invalid global
override, can still prevent the run before per-source execution starts.

## 14. Cache and scope guarantees

The facade contains exactly the selected Stream source, with isolated in-memory
activation/session/application stores and a new temporary credentials file.
Manifest parameter overrides apply when registered; an unregistered scanned YAML
uses its own defaults. The test does not import user credentials or preferences.

Every scenario operation receives a fresh request context without client
`If-None-Match` and a fresh empty data-cache directory. Server-cache handles close
before cleanup. Session/cookie reuse within a source run is distinct from reusing
extracted data. Diagnostics show distinct cache IDs, zero initial memory usage,
final memory usage and facade validation status. The 2026-10-29 qualification
observed fresh, separate caches across repeated runs; see the linked report below.

There is no test YAML cache toggle. Do not delete application caches or sessions,
set a fictional `no_cache` flag or change production facade APIs to author a
contract. These checks do not prove CDN cache bypass, complete video playback,
DRM/license validity or every segment's future availability.

Unsupported schema-v1 features include arbitrary scripts/selectors/transforms,
conditionals, loops, retries, cross-source dependencies, alternate-reference
fallbacks, numeric path indexing, JSONPath, constant-value/regex assertions,
nullable/union types, arbitrary operation parameters and end-to-end playback
(including DRM acquisition and segment validation). Report a real need for a generic extension separately;
do not add unsupported YAML or a source-specific Rust shortcut.

## 15. Final checklist for the author or AI agent

- [ ] Only the requested service contracts and required documentation/tracking
  were created/changed; production and Rust were left alone unless authorized.
- [ ] The path mirrors `source_config`; production `id`, `source` and filename
  stem match exactly.
- [ ] `schema_version` is integer `1`; all required mappings/lists are present.
- [ ] Every named operation is supported by schema v1. Expected operations exist
  in production; nonexpected operations are actually absent.
- [ ] Every expected operation has a step, metadata is included and a real content
  operation can establish a successful full run.
- [ ] Step IDs are unique; references point backward and copy the upstream
  `expect.items_path` exactly.
- [ ] Paths use literal string segments and explicit quoted wildcards. Value and
  field paths have no wildcard and do not include the envelope's `data` key.
- [ ] Each expectation has positive minima and a useful required field beyond
  `source`/`id`; counts do not enforce a volatile catalog size.
- [ ] Optional fields use verified public JSON types; absent and null values are
  handled according to the documented rules. No field path is declared twice.
- [ ] Inputs are deliberate reference values or bounded discovery references;
  irrelevant environment overrides are removed for qualification.
- [ ] The source-specific metadata command actually selected and executed its test.
- [ ] The one-service live command was run if access was available; otherwise the
  unexecuted steps and reason are explicitly reported.
- [ ] Failures, invalid objects, access limits and uncovered operations remain
  visible; no success or complete coverage is invented.

## 16. Code references and historical qualification

When documentation and behavior appear inconsistent, inspect these implementations
before proposing a change. Do not silently change the format while writing a file.

| Responsibility | File |
| --- | --- |
| Existing Rust test entry points | [stream_scraper.rs](../../server/crates/arachnea-stream/tests/stream_scraper.rs) |
| Discovery, selection, mirrored fixture loading and isolated setup | [stream_scraper.rs](../../server/crates/arachnea-stream/tests/support/stream_scraper.rs) |
| Strict contract schema and dependency validation | [source_checks.rs](../../server/crates/arachnea-stream/tests/support/source_checks.rs) |
| JSON traversal and assertions | [assertions.rs](../../server/crates/arachnea-scrapyfy/tests/support/scraper_manager/assertions.rs) |
| Strict YAML reader | [yaml.rs](../../server/crates/arachnea-scrapyfy/tests/support/scraper_manager/yaml.rs) |
| Operation selection, inputs, timeouts and reporting | [runner.rs](../../server/crates/arachnea-stream/tests/support/source_checks/runner.rs) |
| Typed facade calls and envelope checks | [execution.rs](../../server/crates/arachnea-stream/tests/support/source_checks/execution.rs) |
| Schema version and runner bounds | [lib.rs](../../server/crates/arachnea-stream/src/lib.rs) |

