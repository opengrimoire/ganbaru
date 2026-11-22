import { PALETTE_SIZE } from "$lib/calendar/types";
import {
  APP_TOKEN_KEYS,
  BASE_APP_TOKENS,
  BASE_CALENDAR_TOKENS,
  CALENDAR_TOKEN_KEYS,
  DEFAULT_CALENDAR_DEFAULT_CUSTOM,
  DERIVATION_ENGINE_VERSION,
  SOURCE_KEY_ORDER,
  normalizeSemanticSignalAppIsolated,
  syncSemanticSignalAppTokens,
  type CalendarColorDefaultMode,
  type EventPaletteHexes,
  type Theme,
  type ThemeId,
  type ThemeSources,
  type UserTheme,
} from "./definitions";
import { defaultIconLabelFromCanvas } from "./derivation";

const APP_TOKEN_KEY_SET: ReadonlySet<string> = new Set<string>(APP_TOKEN_KEYS);
const CALENDAR_TOKEN_KEY_SET: ReadonlySet<string> = new Set<string>(
  CALENDAR_TOKEN_KEYS,
);

const CALENDAR_COLOR_DEFAULT_MODE_LIST: readonly CalendarColorDefaultMode[] = [
  "light",
  "dark",
  "app-canvas",
  "custom",
];
const CALENDAR_COLOR_DEFAULT_MODES: ReadonlySet<string> = new Set(
  CALENDAR_COLOR_DEFAULT_MODE_LIST,
);

const HEX_COLOR_RE = /^#([0-9a-fA-F]{6}|[0-9a-fA-F]{8})$/;
const SLUG_RE = /^[a-z0-9][a-z0-9-]*$/;
const MAX_DISPLAY_NAME_LENGTH = 60;

function isHexColor(value: unknown): value is string {
  return typeof value === "string" && HEX_COLOR_RE.test(value);
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/**
 * Generate an id for a new user theme. Themes are created on every linked
 * device, so ids are random UUIDs rather than names checked against the
 * themes this device knows. A UUID is a valid theme id slug.
 *
 * @param randomUuid Source of version 4 UUIDs; tests pass a fixed one.
 */
export function generateThemeId(
  randomUuid: () => string = () => crypto.randomUUID(),
): ThemeId {
  const id = randomUuid().toLowerCase();
  if (!SLUG_RE.test(id)) throw new Error("generated theme id is not a slug");
  return id;
}

/**
 * Serialize a theme to a stable, pretty-printed JSON string suitable for
 * clipboard or file export. Keys are emitted in a deterministic order so
 * exported files diff cleanly across saves.
 *
 * Built-ins emit a minimal read-only payload (id, name, base, palette,
 * blendCanvas) used by the editor's "View JSON" affordance; built-ins are
 * never round-tripped through import. User themes emit `schemaVersion: 1`
 * with the full token snapshot, sources, isolated-flag arrays, engine
 * version stamp, and palette. Seeds are install-local reset state and are
 * intentionally omitted from the export.
 */
export function serializeTheme(theme: Theme): string {
  if (theme.kind === "builtin") {
    const ordered: Record<string, unknown> = {
      id: theme.id,
      displayName: theme.displayName,
      base: theme.base,
      iconLabel: theme.iconLabel,
      eventPalette: orderedPalette(theme.eventPalette),
      blendCanvas: theme.blendCanvas,
    };
    return JSON.stringify(ordered, null, 2);
  }
  const ordered: Record<string, unknown> = {
    schemaVersion: 1,
    id: theme.id,
    displayName: theme.displayName,
    iconLabel: theme.iconLabel,
    derivationEngineVersion: theme.derivationEngineVersion,
    calendarDefaults: {
      mode: theme.calendarDefaultMode,
      customBasis: theme.calendarDefaultCustom,
    },
    sources: orderedSources(theme.sources),
    appTokens: orderedTokens(
      syncSemanticSignalAppTokens(theme.sources, theme.appTokens),
      APP_TOKEN_KEYS,
    ),
    calendarTokens: orderedTokens(theme.calendarTokens, CALENDAR_TOKEN_KEYS),
    eventPalette: orderedPalette(theme.eventPalette),
    blendCanvas: theme.blendCanvas,
    appIsolated: orderedIsolated(
      normalizeSemanticSignalAppIsolated(theme.appIsolated),
      APP_TOKEN_KEYS,
    ),
    calendarIsolated: orderedIsolated(theme.calendarIsolated, CALENDAR_TOKEN_KEYS),
  };
  return JSON.stringify(ordered, null, 2);
}

function orderedIsolated(
  set: ReadonlySet<string>,
  order: readonly string[],
): string[] {
  const out: string[] = [];
  for (const key of order) {
    if (set.has(key)) out.push(key);
  }
  return out;
}

function orderedPalette(palette: EventPaletteHexes): string[] {
  return [...palette];
}

function orderedSources(sources: ThemeSources): Record<string, string> {
  const out: Record<string, string> = {};
  for (const key of SOURCE_KEY_ORDER) out[key] = sources[key];
  return out;
}

function orderedTokens(
  source: Readonly<Record<string, string>>,
  order: readonly string[],
): Record<string, string> {
  const out: Record<string, string> = {};
  for (const key of order) {
    if (Object.hasOwn(source, key)) out[key] = source[key];
  }
  return out;
}

/** The theme JSON schema version imports accept. */
export const THEME_SCHEMA_VERSION = 1;

/**
 * One problem theme validation found. `path` names the field from the theme
 * JSON root (`appTokens.--primary`, `eventPalette[3]`), empty for the root
 * itself. Paths and parameters come only from known field names, token keys,
 * indexes, and limits, never from values the JSON supplied, so they are safe
 * to show.
 */
export type ThemeValidationIssue =
  | { code: "not_object"; path: "" }
  | { code: "schema_version"; path: string; expected: number }
  | { code: "required"; path: string }
  | { code: "non_empty_string"; path: string }
  | { code: "slug"; path: string }
  | { code: "built_in_id"; path: string }
  | { code: "max_length"; path: string; limit: number }
  | { code: "hex_color"; path: string }
  | { code: "hex_color_list"; path: string; count: number }
  | { code: "entry_count"; path: string; expected: number; actual: number }
  | { code: "object"; path: string }
  | { code: "token_color_map"; path: string }
  | { code: "token_name_list"; path: string }
  | { code: "string"; path: string }
  | { code: "number"; path: string }
  | { code: "non_negative_integer"; path: string }
  | { code: "one_of"; path: string; allowed: readonly string[] };

export type ThemeValidationIssueCode = ThemeValidationIssue["code"];

export type ThemeValidationResult =
  | { ok: true; theme: UserTheme }
  | { ok: false; issues: ThemeValidationIssue[] };

const ICON_LABELS: readonly string[] = ["light", "dark"];

/**
 * Validate an unknown JSON-parsed value as a Theme suitable for import.
 * Returns a {@link UserTheme} on success (built-ins are never imported);
 * on failure returns every {@link ThemeValidationIssue} found so the UI can
 * surface them at once instead of one round-trip per error. Unknown token
 * keys are stripped silently because dropping a stale token name should not
 * block an otherwise valid theme.
 *
 * Theme imports must use the current `schemaVersion: 1` export shape: full
 * token snapshots, calendar defaults, source palette, isolated-flag arrays,
 * and an engine version stamp.
 */
export function validateThemeJson(input: unknown): ThemeValidationResult {
  if (!isPlainObject(input)) {
    return { ok: false, issues: [{ code: "not_object", path: "" }] };
  }
  if (input.schemaVersion !== THEME_SCHEMA_VERSION) {
    return {
      ok: false,
      issues: [{ code: "schema_version", path: "schemaVersion", expected: THEME_SCHEMA_VERSION }],
    };
  }
  return validateSnapshot(input);
}

function validateIdentity(
  input: Record<string, unknown>,
  errors: ThemeValidationIssue[],
): {
  cleanId: string;
  cleanDisplayName: string;
  cleanBlend: string;
  cleanPalette: string[];
} {
  const { id, displayName, blendCanvas, eventPalette } = input;

  let cleanId = "";
  if (typeof id !== "string" || id.length === 0) {
    errors.push({ code: "non_empty_string", path: "id" });
  } else if (!SLUG_RE.test(id)) {
    errors.push({ code: "slug", path: "id" });
  } else if (id === "light" || id === "dark") {
    errors.push({ code: "built_in_id", path: "id" });
  } else {
    cleanId = id;
  }

  let cleanDisplayName = "";
  if (typeof displayName !== "string" || displayName.trim().length === 0) {
    errors.push({ code: "non_empty_string", path: "displayName" });
  } else if (displayName.length > MAX_DISPLAY_NAME_LENGTH) {
    errors.push({ code: "max_length", path: "displayName", limit: MAX_DISPLAY_NAME_LENGTH });
  } else {
    cleanDisplayName = displayName;
  }

  let cleanBlend = "";
  if (!isHexColor(blendCanvas)) {
    errors.push({ code: "hex_color", path: "blendCanvas" });
  } else {
    cleanBlend = blendCanvas;
  }

  const cleanPalette = sanitizeEventPalette(
    eventPalette,
    errors,
    "eventPalette",
  ) ?? [];
  return { cleanId, cleanDisplayName, cleanBlend, cleanPalette };
}

function sanitizeEventPalette(
  source: unknown,
  errors: ThemeValidationIssue[],
  label: "eventPalette" | "seedEventPalette",
): string[] | undefined {
  if (!Array.isArray(source)) {
    errors.push({ code: "hex_color_list", path: label, count: PALETTE_SIZE });
    return undefined;
  }
  if (source.length !== PALETTE_SIZE) {
    errors.push({
      code: "entry_count",
      path: label,
      expected: PALETTE_SIZE,
      actual: source.length,
    });
    return undefined;
  }
  const out: string[] = [];
  for (let i = 0; i < source.length; i++) {
    const value = source[i];
    if (!isHexColor(value)) {
      errors.push({ code: "hex_color", path: `${label}[${i}]` });
      out.push("#000000");
    } else {
      out.push(value);
    }
  }
  return out;
}

function validateSnapshot(input: Record<string, unknown>): ThemeValidationResult {
  const errors: ThemeValidationIssue[] = [];
  const { cleanId, cleanDisplayName, cleanBlend, cleanPalette } =
    validateIdentity(input, errors);

  const rawCanvas =
    isPlainObject(input.sources) &&
    typeof (input.sources as Record<string, unknown>).canvas === "string"
      ? ((input.sources as Record<string, unknown>).canvas as string)
      : cleanBlend;
  const fallbackBase: "light" | "dark" = isHexColor(rawCanvas)
    ? defaultIconLabelFromCanvas(rawCanvas)
    : "dark";

  const cleanAppTokensRaw = sanitizeFullTokenSnapshot(
    input.appTokens,
    APP_TOKEN_KEYS,
    APP_TOKEN_KEY_SET,
    BASE_APP_TOKENS[fallbackBase],
    "appTokens",
    errors,
  );
  const cleanCalendarTokens = sanitizeFullTokenSnapshot(
    input.calendarTokens,
    CALENDAR_TOKEN_KEYS,
    CALENDAR_TOKEN_KEY_SET,
    BASE_CALENDAR_TOKENS[fallbackBase],
    "calendarTokens",
    errors,
  );

  const cleanSources = sanitizeSources(
    input.sources,
    errors,
    "sources",
  );
  if (!cleanSources && !errors.some((e) => e.path.startsWith("sources"))) {
    errors.push({ code: "required", path: "sources" });
  }

  const cleanAppTokens = cleanSources
    ? syncSemanticSignalAppTokens(cleanSources, cleanAppTokensRaw)
    : cleanAppTokensRaw;

  const cleanAppIsolated = normalizeSemanticSignalAppIsolated(
    sanitizeIsolatedList(
      input.appIsolated,
      APP_TOKEN_KEY_SET,
      "appIsolated",
      errors,
    ),
  );
  const cleanCalendarIsolated = sanitizeIsolatedList(
    input.calendarIsolated,
    CALENDAR_TOKEN_KEY_SET,
    "calendarIsolated",
    errors,
  );

  let cleanEngineVersion = DERIVATION_ENGINE_VERSION;
  if (typeof input.derivationEngineVersion === "number") {
    if (
      Number.isInteger(input.derivationEngineVersion) &&
      input.derivationEngineVersion >= 0
    ) {
      cleanEngineVersion = input.derivationEngineVersion;
    } else {
      errors.push({ code: "non_negative_integer", path: "derivationEngineVersion" });
    }
  } else if (input.derivationEngineVersion !== undefined) {
    errors.push({ code: "number", path: "derivationEngineVersion" });
  }

  const cleanIconLabel = sanitizeIconLabel(
    input.iconLabel,
    cleanSources?.canvas ?? cleanBlend,
    "iconLabel",
    errors,
  );
  const calendarDefaults = sanitizeCalendarDefaults(
    input.calendarDefaults,
    cleanSources?.canvas ?? cleanBlend,
    errors,
  );

  if (errors.length > 0) return { ok: false, issues: errors };

  const theme: UserTheme = {
    kind: "user",
    id: cleanId,
    displayName: cleanDisplayName,
    iconLabel: cleanIconLabel,
    blendCanvas: cleanBlend,
    eventPalette: cleanPalette,
    derivationEngineVersion: cleanEngineVersion,
    calendarDefaultMode: calendarDefaults.mode,
    calendarDefaultCustom: calendarDefaults.customBasis,
    sources: cleanSources as ThemeSources,
    appTokens: cleanAppTokens,
    calendarTokens: cleanCalendarTokens,
    appIsolated: cleanAppIsolated,
    calendarIsolated: cleanCalendarIsolated,
    seedSources: { ...(cleanSources as ThemeSources) },
    seedAppTokens: { ...cleanAppTokens },
    seedCalendarTokens: { ...cleanCalendarTokens },
    seedAppIsolated: new Set(cleanAppIsolated),
    seedCalendarIsolated: new Set(cleanCalendarIsolated),
    seedEventPalette: [...cleanPalette],
    seedBlendCanvas: cleanBlend,
    seedCalendarDefaultMode: calendarDefaults.mode,
    seedCalendarDefaultCustom: calendarDefaults.customBasis,
    seedIconLabel: cleanIconLabel,
    seedDerivationEngineVersion: cleanEngineVersion,
  };
  return { ok: true, theme };
}

/** Validate the required decorative `iconLabel` field. */
function sanitizeIconLabel(
  raw: unknown,
  fallbackCanvasHex: string,
  fieldName: string,
  errors: ThemeValidationIssue[],
): "light" | "dark" {
  if (raw === undefined) {
    errors.push({ code: "required", path: fieldName });
    return defaultIconLabelFromCanvas(fallbackCanvasHex);
  }
  if (raw === "light" || raw === "dark") return raw;
  errors.push({ code: "one_of", path: fieldName, allowed: ICON_LABELS });
  return defaultIconLabelFromCanvas(fallbackCanvasHex);
}

function sanitizeCalendarDefaults(
  raw: unknown,
  fallbackCustomBasis: string,
  errors: ThemeValidationIssue[],
): { mode: CalendarColorDefaultMode; customBasis: string } {
  const fallbackBasis = isHexColor(fallbackCustomBasis)
    ? fallbackCustomBasis
    : DEFAULT_CALENDAR_DEFAULT_CUSTOM;
  if (raw === undefined) {
    errors.push({ code: "required", path: "calendarDefaults" });
    return { mode: "app-canvas", customBasis: fallbackBasis };
  }
  if (!isPlainObject(raw)) {
    errors.push({ code: "object", path: "calendarDefaults" });
    return { mode: "app-canvas", customBasis: fallbackBasis };
  }
  const mode = raw.mode;
  let cleanMode: CalendarColorDefaultMode = "app-canvas";
  if (typeof mode !== "string" || !CALENDAR_COLOR_DEFAULT_MODES.has(mode)) {
    errors.push({
      code: "one_of",
      path: "calendarDefaults.mode",
      allowed: CALENDAR_COLOR_DEFAULT_MODE_LIST,
    });
  } else {
    cleanMode = mode as CalendarColorDefaultMode;
  }
  const customBasis = raw.customBasis;
  if (customBasis === undefined) {
    return { mode: cleanMode, customBasis: fallbackBasis };
  }
  if (!isHexColor(customBasis)) {
    errors.push({ code: "hex_color", path: "calendarDefaults.customBasis" });
    return { mode: cleanMode, customBasis: fallbackBasis };
  }
  return { mode: cleanMode, customBasis };
}

function sanitizeIsolatedList(
  source: unknown,
  allowed: ReadonlySet<string>,
  fieldName: string,
  errors: ThemeValidationIssue[],
): Set<string> {
  if (source === undefined) {
    errors.push({ code: "required", path: fieldName });
    return new Set();
  }
  if (!Array.isArray(source)) {
    errors.push({ code: "token_name_list", path: fieldName });
    return new Set();
  }
  const out = new Set<string>();
  for (let i = 0; i < source.length; i++) {
    const value = source[i];
    if (typeof value !== "string") {
      errors.push({ code: "string", path: `${fieldName}[${i}]` });
      continue;
    }
    if (!allowed.has(value)) continue;
    out.add(value);
  }
  return out;
}

/** Validate a full current-shape token snapshot. */
function sanitizeFullTokenSnapshot(
  source: unknown,
  order: readonly string[],
  allowed: ReadonlySet<string>,
  base: Readonly<Record<string, string>>,
  fieldName: string,
  errors: ThemeValidationIssue[],
): Record<string, string> {
  if (!isPlainObject(source)) {
    errors.push({ code: "token_color_map", path: fieldName });
    return { ...base };
  }
  const out: Record<string, string> = {};
  for (const key of order) {
    const value = (source as Record<string, unknown>)[key];
    if (value === undefined) {
      errors.push({ code: "required", path: `${fieldName}.${key}` });
      out[key] = base[key];
      continue;
    }
    if (!isHexColor(value)) {
      errors.push({ code: "hex_color", path: `${fieldName}.${key}` });
      continue;
    }
    if (!allowed.has(key)) continue;
    out[key] = value;
  }
  return out;
}

function sanitizeSources(
  source: unknown,
  errors: ThemeValidationIssue[],
  fieldName: string = "sources",
): ThemeSources | undefined {
  if (source === undefined) {
    errors.push({ code: "required", path: fieldName });
    return undefined;
  }
  if (!isPlainObject(source)) {
    errors.push({ code: "object", path: fieldName });
    return undefined;
  }
  const out: Partial<ThemeSources> = {};
  let ok = true;
  for (const key of SOURCE_KEY_ORDER) {
    const value = (source as Record<string, unknown>)[key];
    if (value === undefined) {
      errors.push({ code: "required", path: `${fieldName}.${key}` });
      ok = false;
      continue;
    }
    if (!isHexColor(value)) {
      errors.push({ code: "hex_color", path: `${fieldName}.${key}` });
      ok = false;
      continue;
    }
    out[key] = value;
  }
  if (!ok) return undefined;
  return out as ThemeSources;
}
