# Localization

**Partial.** All normal app chrome and main feature surfaces use a typed message catalog. English is the canonical catalog and Spanish is the first translation. Missing translations fall back to English per message key, so the app stays usable while catalogs evolve.

## Language preference

The preference is `system`, `en`, or `es`, stored in active-vault `config.json` as `preferences.language` and edited in Appearance settings. `system` resolves from the operating system or browser language list: exact supported matches first, then the base language (`es-MX` resolves to `es`), then English. Invalid stored values are normalized and written back as `system` so a bad config does not fail every boot.

## Language selectors

Selectors are designed for someone looking for their language even when they cannot read the current UI.

- Explicit languages always appear as autonyms (`English`, `Español`), never translated into the current UI language.
- Non-language options are localized. The system option shows the resolved language in parentheses, such as `System language (Español)`.
- Search matches the autonym, the name in the current UI language, locale IDs, and useful aliases, so both `Spanish` and `Español` find Spanish.

## Before a vault exists

The first-run data-folder screen has a compact searchable language selector. Because no `config.json` exists yet, the choice is applied without persisting and kept as a temporary setup preference. After the user creates or imports a folder, it is copied into `preferences.language` and cleared. Boot checks this temporary value before mounting so neither screen paints in the wrong language during the handoff. Adding the selector or an error message must not shift the main setup content.

## Runtime behavior

Platform bootstraps resolve and load the selected catalog before mounting the shell. English is always resident; every other catalog is a separate lazily loaded chunk.

Language changes are atomic: the preference, active catalog, resolved locale, document `lang` and `dir`, and persisted value change together only after the catalog loads. A failed load keeps the previous language and can be retried, and a slower obsolete load never replaces a newer choice. System language changes follow the same path. Text direction is part of locale metadata, so right-to-left support has a defined entry point even though current locales are left-to-right.

English is typed as the canonical catalog shape and other catalogs must satisfy it partially, so adding an English key defines the key space for every translation.

## Formatting

User-facing dates, times, numbers, plurals, relative minutes, and lists use the locale-aware helpers in `apps/client/src/lib/i18n/formatters.ts`. Do not hardcode `"en"` or `"en-US"` in UI formatting except for external standards, stable interchange formats, or deliberate parsers.

Storage and interoperability stay canonical and untranslated:

- Calendar instants stay UTC ISO 8601; all-day values stay floating dates.
- SQLite and config keys stay stable English identifiers.
- iCalendar import and export use standards-defined values.
- Benchmark markdown for [performance results](../performance/results.md) stays in English so recorded rows remain comparable.

## Translation scope

Settings, title bar, Calendar, Pomodoro, Music, distraction blocker, theme editor, diagnostics, and benchmark overlays all use catalog keys or feature-local helpers. Internal IDs, CSS tokens, config keys, SQL columns, generated benchmark output, and tests can stay English when they are not shown to users. If an internal English value is shown, localize it at the render boundary rather than changing the stored identity.

Shared confirmation dialogs add localized Escape and Enter hints at render time, so feature catalogs provide only action text and translations never duplicate shortcut wording. A custom confirmation surface must render the same hints and keys.

## Adding a locale

1. Add the locale to `APP_LOCALES` and `LOCALE_METADATA` in `apps/client/src/lib/i18n/locales.ts`. The language preference list and setup selector options derive from these.
2. Add the catalog under `apps/client/src/lib/i18n/messages/`.
3. Register its lazy importer in `apps/client/src/lib/i18n/catalog-loader.ts`.
4. Add or update tests for locale resolution, fallback, setup search aliases, and locale-specific formatting.
5. Review app surfaces for hardcoded user-facing text.
