/**
 * Conversion between a {@link UserTheme} and the SQLite row groups the themes API reads and writes: ordered token
 * rows, palette slots, and the seed copies of both.
 */

import type { UserThemeRead, UserThemeWrite } from "$lib/api/themes";
import { PALETTE_SIZE } from "$lib/calendar/types";
import {
  APP_TOKEN_KEYS,
  BASE_APP_TOKENS,
  BASE_CALENDAR_TOKENS,
  CALENDAR_TOKEN_KEYS,
  DEFAULT_CALENDAR_DEFAULT_CUSTOM,
  THEME_TOKEN_ROW_ORDER,
  darkTheme,
  lightTheme,
  normalizeSemanticSignalAppIsolated,
  syncSemanticSignalAppTokens,
  type CalendarColorDefaultMode,
  type ThemeSources,
  type UserTheme,
} from "./definitions";
import { defaultIconLabelFromCanvas } from "./derivation";

type TokenWrite = UserThemeWrite["tokens"][number];

function orderedTokenRows(
  sources: ThemeSources,
  appTokens: Readonly<Record<string, string>>,
  calendarTokens: Readonly<Record<string, string>>,
  appIsolated: ReadonlySet<string>,
  calendarIsolated: ReadonlySet<string>,
): TokenWrite[] {
  const rows: TokenWrite[] = [];
  for (const entry of THEME_TOKEN_ROW_ORDER) {
    if (entry.kind === "source") {
      rows.push({
        kind: "source",
        key: entry.key,
        value: sources[entry.key],
        isolated: false,
      });
      continue;
    }
    if (entry.kind === "app") {
      rows.push({
        kind: "app",
        key: entry.key,
        value: appTokens[entry.key],
        isolated: appIsolated.has(entry.key),
      });
      continue;
    }
    rows.push({
      kind: "calendar",
      key: entry.key,
      value: calendarTokens[entry.key],
      isolated: calendarIsolated.has(entry.key),
    });
  }
  return rows;
}

/**
 * Shape a {@link UserTheme} into the row groups the DB layer expects.
 */
export function userThemeToWrite(theme: UserTheme): UserThemeWrite {
  const appTokens = syncSemanticSignalAppTokens(theme.sources, theme.appTokens);
  const seedAppTokens = syncSemanticSignalAppTokens(
    theme.seedSources,
    theme.seedAppTokens,
  );
  const tokens = orderedTokenRows(
    theme.sources,
    appTokens,
    theme.calendarTokens,
    normalizeSemanticSignalAppIsolated(theme.appIsolated),
    theme.calendarIsolated,
  );
  const palette = theme.eventPalette.map((value, slot) => ({ slot, value }));
  const seedTokens = orderedTokenRows(
    theme.seedSources,
    seedAppTokens,
    theme.seedCalendarTokens,
    normalizeSemanticSignalAppIsolated(theme.seedAppIsolated),
    theme.seedCalendarIsolated,
  );
  const seedPalette = theme.seedEventPalette.map((value, slot) => ({
    slot,
    value,
  }));
  return {
    id: theme.id,
    displayName: theme.displayName,
    iconLabel: theme.iconLabel,
    seedIconLabel: theme.seedIconLabel,
    blendCanvas: theme.blendCanvas,
    seedBlendCanvas: theme.seedBlendCanvas,
    derivationEngineVersion: theme.derivationEngineVersion,
    seedDerivationEngineVersion: theme.seedDerivationEngineVersion,
    calendarDefaultMode: theme.calendarDefaultMode,
    calendarDefaultCustom: theme.calendarDefaultCustom,
    seedCalendarDefaultMode: theme.seedCalendarDefaultMode,
    seedCalendarDefaultCustom: theme.seedCalendarDefaultCustom,
    tokens,
    palette,
    seedTokens,
    seedPalette,
  };
}

/**
 * Build a {@link UserTheme} from row groups returned by `loadAllUserThemes`.
 * Missing tokens are backfilled from BASE so a partial DB write or a
 * malformed import never crashes the UI. The fallback BASE table is picked
 * from the canvas-row luminance, so a user theme recovers consistent defaults
 * regardless of which built-in family it originally came from.
 */
export function userThemeFromRead(read: UserThemeRead): UserTheme {
  const tokenSourceRows = read.tokens.filter((t) => t.kind === "source");
  const tokenAppRows = read.tokens.filter((t) => t.kind === "app");
  const seedSourceRows = read.seedTokens.filter((t) => t.kind === "source");
  const seedAppRows = read.seedTokens.filter((t) => t.kind === "app");
  const sources = sourcesFromRows(
    tokenSourceRows,
    tokenAppRows,
  );
  const seedSources = sourcesFromRows(
    seedSourceRows,
    seedAppRows,
  );
  const fallbackBase = defaultIconLabelFromCanvas(sources.canvas);
  const seedFallbackBase = defaultIconLabelFromCanvas(seedSources.canvas);
  const appTokens = syncSemanticSignalAppTokens(
    sources,
    snapshotFromRows(
      tokenAppRows,
      APP_TOKEN_KEYS,
      BASE_APP_TOKENS[fallbackBase],
    ),
  );
  const calendarTokens = snapshotFromRows(
    read.tokens.filter((t) => t.kind === "calendar"),
    CALENDAR_TOKEN_KEYS,
    BASE_CALENDAR_TOKENS[fallbackBase],
  );
  const seedAppTokens = syncSemanticSignalAppTokens(
    seedSources,
    snapshotFromRows(
      seedAppRows,
      APP_TOKEN_KEYS,
      BASE_APP_TOKENS[seedFallbackBase],
    ),
  );
  const seedCalendarTokens = snapshotFromRows(
    read.seedTokens.filter((t) => t.kind === "calendar"),
    CALENDAR_TOKEN_KEYS,
    BASE_CALENDAR_TOKENS[seedFallbackBase],
  );
  const appIsolated = normalizeSemanticSignalAppIsolated(
    isolatedFromRows(tokenAppRows),
  );
  const calendarIsolated = isolatedFromRows(
    read.tokens.filter((t) => t.kind === "calendar"),
  );
  const seedAppIsolated = normalizeSemanticSignalAppIsolated(
    isolatedFromRows(seedAppRows),
  );
  const seedCalendarIsolated = isolatedFromRows(
    read.seedTokens.filter((t) => t.kind === "calendar"),
  );
  const eventPalette = paletteFromRows(read.palette, fallbackBase);
  const seedEventPalette = paletteFromRows(read.seedPalette, seedFallbackBase);
  const calendarDefaultMode = calendarDefaultModeFromRow(
    read.theme.calendar_default_mode,
  );
  const seedCalendarDefaultMode = calendarDefaultModeFromRow(
    read.theme.seed_calendar_default_mode,
  );
  const calendarDefaultCustom = calendarDefaultCustomFromRow(
    read.theme.calendar_default_custom,
    sources.canvas,
  );
  const seedCalendarDefaultCustom = calendarDefaultCustomFromRow(
    read.theme.seed_calendar_default_custom,
    seedSources.canvas,
  );
  return {
    kind: "user",
    id: read.theme.id,
    displayName: read.theme.display_name,
    iconLabel: read.theme.icon_label,
    blendCanvas: read.theme.blend_canvas,
    eventPalette,
    derivationEngineVersion: read.theme.derivation_engine_version,
    calendarDefaultMode,
    calendarDefaultCustom,
    sources,
    appTokens,
    calendarTokens,
    appIsolated,
    calendarIsolated,
    seedSources,
    seedAppTokens,
    seedCalendarTokens,
    seedAppIsolated,
    seedCalendarIsolated,
    seedEventPalette,
    seedBlendCanvas: read.theme.seed_blend_canvas,
    seedCalendarDefaultMode,
    seedCalendarDefaultCustom,
    seedIconLabel: read.theme.seed_icon_label,
    seedDerivationEngineVersion: read.theme.seed_derivation_engine_version,
  };
}

function sourcesFromRows(
  rows: ReadonlyArray<{ key: string; value: string }>,
  appRows: ReadonlyArray<{ key: string; value: string }> = [],
): ThemeSources {
  const map = new Map(rows.map((r) => [r.key, r.value]));
  const appMap = new Map(appRows.map((r) => [r.key, r.value]));
  // If the canvas row is missing entirely, fall back to the dark BASE
  // canvas; otherwise pick a fallback table from the canvas luminance.
  const canvas = map.get("canvas") ?? BASE_APP_TOKENS.dark["--background"];
  const fallback = defaultIconLabelFromCanvas(canvas);
  return {
    canvas,
    ink: map.get("ink") ?? BASE_APP_TOKENS[fallback]["--foreground"],
    primary: map.get("primary") ?? BASE_APP_TOKENS[fallback]["--primary"],
    destructive:
      map.get("destructive") ?? BASE_APP_TOKENS[fallback]["--destructive"],
    destructiveText:
      map.get("destructiveText") ??
      appMap.get("--destructive-foreground") ??
      BASE_APP_TOKENS[fallback]["--destructive-foreground"],
    confirm: map.get("confirm") ?? BASE_APP_TOKENS[fallback]["--action-confirm"],
    confirmText:
      map.get("confirmText") ??
      appMap.get("--action-confirm-foreground") ??
      BASE_APP_TOKENS[fallback]["--action-confirm-foreground"],
    warning:
      map.get("warning") ?? BASE_APP_TOKENS[fallback]["--status-tentative"],
    warningText:
      map.get("warningText") ??
      appMap.get("--status-tentative-foreground") ??
      BASE_APP_TOKENS[fallback]["--status-tentative-foreground"],
  };
}

function snapshotFromRows(
  rows: ReadonlyArray<{ key: string; value: string }>,
  order: readonly string[],
  fallback: Readonly<Record<string, string>>,
): Record<string, string> {
  const map = new Map(rows.map((r) => [r.key, r.value]));
  const out: Record<string, string> = {};
  for (const key of order) out[key] = map.get(key) ?? fallback[key];
  return out;
}

function isolatedFromRows(
  rows: ReadonlyArray<{ key: string; isolated: number }>,
): Set<string> {
  const out = new Set<string>();
  for (const r of rows) if (r.isolated) out.add(r.key);
  return out;
}

function paletteFromRows(
  rows: ReadonlyArray<{ slot: number; value: string }>,
  fallbackBase: "light" | "dark",
): string[] {
  const fallbackPalette = fallbackBase === "dark"
    ? darkTheme.eventPalette
    : lightTheme.eventPalette;
  const out: string[] = [...fallbackPalette];
  for (const r of rows) {
    if (r.slot >= 0 && r.slot < PALETTE_SIZE) out[r.slot] = r.value;
  }
  return out;
}

function calendarDefaultModeFromRow(value: string): CalendarColorDefaultMode {
  if (
    value === "light" ||
    value === "dark" ||
    value === "app-canvas" ||
    value === "custom"
  ) {
    return value;
  }
  return "app-canvas";
}

function calendarDefaultCustomFromRow(value: string, fallback: string): string {
  if (/^#([0-9a-fA-F]{6}|[0-9a-fA-F]{8})$/.test(value)) return value;
  if (/^#([0-9a-fA-F]{6}|[0-9a-fA-F]{8})$/.test(fallback)) return fallback;
  return DEFAULT_CALENDAR_DEFAULT_CUSTOM;
}
