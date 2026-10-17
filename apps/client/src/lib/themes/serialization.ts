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

const CALENDAR_COLOR_DEFAULT_MODES: ReadonlySet<string> = new Set([
  "light",
  "dark",
  "app-canvas",
  "custom",
]);

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
 * Generate a unique slug-style theme ID. Prefix is short and human-friendly,
 * suffix is a 6-char base36 random tail. Pass the existing combined registry
 * to guarantee no collisions with current themes.
 */
export function generateThemeId(
  existing: Readonly<Record<ThemeId, Theme>>,
  prefix = "theme",
): ThemeId {
  const safePrefix = SLUG_RE.test(prefix) ? prefix : "theme";
  for (let attempt = 0; attempt < 64; attempt++) {
    const tail = Math.random().toString(36).slice(2, 8);
    const candidate = `${safePrefix}-${tail}`;
    if (!Object.hasOwn(existing, candidate)) return candidate;
  }
  return `${safePrefix}-${Date.now().toString(36)}`;
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

export type ThemeValidationResult =
  | { ok: true; theme: UserTheme }
  | { ok: false; errors: string[] };

/**
 * Validate an unknown JSON-parsed value as a Theme suitable for import.
 * Returns a {@link UserTheme} on success (built-ins are never imported);
 * on failure returns a list of all problems found so the UI can surface
 * them at once instead of one round-trip per error. Unknown token keys
 * are stripped silently because dropping a stale token name should not
 * block an otherwise valid theme.
 *
 * Theme imports must use the current `schemaVersion: 1` export shape: full
 * token snapshots, calendar defaults, source palette, isolated-flag arrays,
 * and an engine version stamp.
 */
export function validateThemeJson(input: unknown): ThemeValidationResult {
  if (!isPlainObject(input)) {
    return { ok: false, errors: ["theme must be a JSON object"] };
  }
  if (input.schemaVersion !== 1) {
    return { ok: false, errors: ["schemaVersion must be 1"] };
  }
  return validateSnapshot(input);
}

function validateIdentity(
  input: Record<string, unknown>,
  errors: string[],
): {
  cleanId: string;
  cleanDisplayName: string;
  cleanBlend: string;
  cleanPalette: string[];
} {
  const { id, displayName, blendCanvas, eventPalette } = input;

  let cleanId = "";
  if (typeof id !== "string" || id.length === 0) {
    errors.push("id must be a non-empty string");
  } else if (!SLUG_RE.test(id)) {
    errors.push(
      "id must be a slug (lowercase letters, digits, and hyphens; must start with a letter or digit)",
    );
  } else if (id === "light" || id === "dark") {
    errors.push("id must not collide with a built-in theme");
  } else {
    cleanId = id;
  }

  let cleanDisplayName = "";
  if (typeof displayName !== "string" || displayName.trim().length === 0) {
    errors.push("displayName must be a non-empty string");
  } else if (displayName.length > MAX_DISPLAY_NAME_LENGTH) {
    errors.push(`displayName must be ${MAX_DISPLAY_NAME_LENGTH} characters or fewer`);
  } else {
    cleanDisplayName = displayName;
  }

  let cleanBlend = "";
  if (!isHexColor(blendCanvas)) {
    errors.push("blendCanvas must be a hex color (#RRGGBB or #RRGGBBAA)");
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
  errors: string[],
  label: "eventPalette" | "seedEventPalette",
): string[] | undefined {
  if (!Array.isArray(source)) {
    errors.push(`${label} must be an array of ${PALETTE_SIZE} hex strings`);
    return undefined;
  }
  if (source.length !== PALETTE_SIZE) {
    errors.push(
      `${label} must contain exactly ${PALETTE_SIZE} entries (got ${source.length})`,
    );
    return undefined;
  }
  const out: string[] = [];
  for (let i = 0; i < source.length; i++) {
    const value = source[i];
    if (!isHexColor(value)) {
      errors.push(`${label}[${i}] must be a hex color`);
      out.push("#000000");
    } else {
      out.push(value);
    }
  }
  return out;
}

function validateSnapshot(input: Record<string, unknown>): ThemeValidationResult {
  const errors: string[] = [];
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
  if (!cleanSources && !errors.some((e) => e.startsWith("sources"))) {
    errors.push("sources is required");
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
      errors.push("derivationEngineVersion must be a non-negative integer");
    }
  } else if (input.derivationEngineVersion !== undefined) {
    errors.push("derivationEngineVersion must be a number");
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

  if (errors.length > 0) return { ok: false, errors };

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
  };
  return { ok: true, theme };
}

/** Validate the required decorative `iconLabel` field. */
function sanitizeIconLabel(
  raw: unknown,
  fallbackCanvasHex: string,
  fieldName: string,
  errors: string[],
): "light" | "dark" {
  if (raw === undefined) {
    errors.push(`${fieldName} is required`);
    return defaultIconLabelFromCanvas(fallbackCanvasHex);
  }
  if (raw === "light" || raw === "dark") return raw;
  errors.push(`${fieldName} must be "light" or "dark"`);
  return defaultIconLabelFromCanvas(fallbackCanvasHex);
}

function sanitizeCalendarDefaults(
  raw: unknown,
  fallbackCustomBasis: string,
  errors: string[],
): { mode: CalendarColorDefaultMode; customBasis: string } {
  const fallbackBasis = isHexColor(fallbackCustomBasis)
    ? fallbackCustomBasis
    : DEFAULT_CALENDAR_DEFAULT_CUSTOM;
  if (raw === undefined) {
    errors.push("calendarDefaults is required");
    return { mode: "app-canvas", customBasis: fallbackBasis };
  }
  if (!isPlainObject(raw)) {
    errors.push("calendarDefaults must be an object");
    return { mode: "app-canvas", customBasis: fallbackBasis };
  }
  const mode = raw.mode;
  let cleanMode: CalendarColorDefaultMode = "app-canvas";
  if (typeof mode !== "string" || !CALENDAR_COLOR_DEFAULT_MODES.has(mode)) {
    errors.push(
      'calendarDefaults.mode must be "light", "dark", "app-canvas", or "custom"',
    );
  } else {
    cleanMode = mode as CalendarColorDefaultMode;
  }
  const customBasis = raw.customBasis;
  if (customBasis === undefined) {
    return { mode: cleanMode, customBasis: fallbackBasis };
  }
  if (!isHexColor(customBasis)) {
    errors.push("calendarDefaults.customBasis must be a hex color");
    return { mode: cleanMode, customBasis: fallbackBasis };
  }
  return { mode: cleanMode, customBasis };
}

function sanitizeIsolatedList(
  source: unknown,
  allowed: ReadonlySet<string>,
  fieldName: string,
  errors: string[],
): Set<string> {
  if (source === undefined) {
    errors.push(`${fieldName} is required`);
    return new Set();
  }
  if (!Array.isArray(source)) {
    errors.push(`${fieldName} must be an array of token-key strings`);
    return new Set();
  }
  const out = new Set<string>();
  for (let i = 0; i < source.length; i++) {
    const value = source[i];
    if (typeof value !== "string") {
      errors.push(`${fieldName}[${i}] must be a string`);
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
  errors: string[],
): Record<string, string> {
  if (!isPlainObject(source)) {
    errors.push(`${fieldName} must be an object of token-hex pairs`);
    return { ...base };
  }
  const out: Record<string, string> = {};
  for (const key of order) {
    const value = (source as Record<string, unknown>)[key];
    if (value === undefined) {
      errors.push(`${fieldName}.${key} is required`);
      out[key] = base[key];
      continue;
    }
    if (!isHexColor(value)) {
      errors.push(`${fieldName}.${key} must be a hex color`);
      continue;
    }
    if (!allowed.has(key)) continue;
    out[key] = value;
  }
  return out;
}

function sanitizeSources(
  source: unknown,
  errors: string[],
  fieldName: string = "sources",
): ThemeSources | undefined {
  if (source === undefined) {
    errors.push(`${fieldName} is required`);
    return undefined;
  }
  if (!isPlainObject(source)) {
    errors.push(`${fieldName} must be an object`);
    return undefined;
  }
  const out: Partial<ThemeSources> = {};
  let ok = true;
  for (const key of SOURCE_KEY_ORDER) {
    const value = (source as Record<string, unknown>)[key];
    if (value === undefined) {
      errors.push(`${fieldName}.${key} is required`);
      ok = false;
      continue;
    }
    if (!isHexColor(value)) {
      errors.push(`${fieldName}.${key} must be a hex color`);
      ok = false;
      continue;
    }
    out[key] = value;
  }
  if (!ok) return undefined;
  return out as ThemeSources;
}
