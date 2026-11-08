# Shared language catalog

`languages.json` is the single source of truth for language metadata used by
`front/public-app` and `front/admin-app`. `languages.ts` is the typed access
layer over that file; each app re-exports it from its own
`src/services/languages.ts` facade, and `front/public-app/src/utils.ts` keeps
re-exporting the same helpers for its existing `@/utils` imports.

## Entry shape

Each `languages[]` entry contains:

| Field                 | Description                                            |
| --------------------- | ------------------------------------------------------ |
| `code`                | ISO 639-1 two-letter code                              |
| `alpha3`              | ISO 639-2 terminology three-letter code                |
| `alpha3Bibliographic` | ISO 639-2 bibliographic code when it differs           |
| `name`                | English display name                                   |
| `nativeName`          | Name in the language itself                            |
| `flag`                | Flag emoji representing the language                   |
| `direction`           | Writing direction, `ltr` or `rtl`                      |

Entries are sorted by English name. Flags use a representative CLDR likely
region for the language; international or constructed languages without one
(`eo`, `ia`, `io`, `vo`) use the neutral `🌐`, and `en` keeps the `🇬🇧` flag
already used by the locale manifests. The top-level `defaults` block holds the
fallback interface language (`uiLanguage`), the ordered subtitle language
priority (`subtitleLanguagePriority`) and the initially configured subtitle
languages (`subtitleLanguages`). Change these values here instead of adding
language-code literals to a component or service.

## Helpers

- `LANGUAGES` — the sorted catalog.
- `DEFAULT_LANGUAGE_CODE`, `DEFAULT_SUBTITLE_LANGUAGE_PRIORITY`,
  `DEFAULT_SUBTITLE_LANGUAGES` and `DEFAULT_SUBTITLE_LANGUAGE` — catalog
  defaults (the last one is the language-input placeholder).
- `getLanguage()` — lookup by ISO 639-1, ISO 639-2 terminology or
  bibliographic code.
- `isKnownLanguageCode()` — membership check.
- `getLanguageEnglishName()` / `getLanguageNativeName()` — display names.
- `getLanguageFlag()` — flag emoji.
- `formatLanguageLabel()` — `<flag> <English name> - <native name>` label, or the
  raw code when unknown.
- `normalizeLanguageCode()` — lowercases codes, converts known ISO 639-2
  variants to ISO 639-1 and keeps unknown but well-formed codes for
  source-specific tracks.
- `getBaseLanguageCode()` — base subtag of a normalized BCP-47 code.

## Locale manifests

`public/locales/index.json` in each app still lists the translation
dictionaries that are actually shipped, and `defaultLanguage` there names the
default dictionary of that build. Adding a language to the catalog therefore
does not by itself make it selectable as an interface language: the matching
locale dictionary must also be declared in the app's `index.json`.
