/**
 * Pure helpers for the user-theme CRUD pipeline.
 *
 * The reactive store in `theme.svelte.ts` glues these together with a `$state`
 * map of user themes, but the merge / clone / naming logic itself is pure so
 * it can be tested in isolation.
 */

import {
  APP_TOKEN_KEYS,
  BASE_CALENDAR_TOKENS,
  CALENDAR_TOKEN_KEYS,
  DERIVATION_ENGINE_VERSION,
  DEFAULT_CALENDAR_DEFAULT_CUSTOM,
  normalizeSemanticSignalAppIsolated,
  resolveAppTokens,
  syncSemanticSignalAppTokens,
  type Theme,
  type ThemeId,
  type ThemeTokenKind,
  type ThemeSources,
  type UserTheme,
} from "$lib/themes";

const MAX_DISPLAY_NAME_LENGTH = 60;
const NAME_SUFFIX_CAP = 999;

/**
 * Deep-copy a theme into a new {@link UserTheme} with a fresh identity.
 *
 * The clone always carries a `sources` palette:
 * - User-theme source: copy the existing sources, isolated flags, snapshots.
 * - Built-in source: synthesize sources by sampling canvas, ink, primary,
 *   destructive, confirm, and warning from the resolved tokens, then seed
 *   the calendar default from the built-in base (`light` or `dark`).
 *   Calendar canvas is not a source; the selected calendar default writes
 *   the snapshot and `--cal-bg` remains pinnable through `calendarIsolated`.
 *
 * Identity overrides for derived tokens are NOT captured: the derivation
 * is calibrated to reproduce the dark built-in's surface hierarchy on any
 * canvas, so the clone naturally tracks its sources. Editing canvas on the
 * clone cascades through every non-isolated surface. Seeds snapshot the
 * live state at clone time for per-row reset and "Reset all".
 */
export function cloneTheme(
  source: Theme,
  id: ThemeId,
  displayName: string,
): UserTheme {
  const resolvedApp = pickTokenSnapshot(resolveAppTokens(source), APP_TOKEN_KEYS);
  const resolvedCalendarTokens =
    source.kind === "user"
      ? pickTokenSnapshot(source.calendarTokens, CALENDAR_TOKEN_KEYS)
      : { ...BASE_CALENDAR_TOKENS[source.base] };
  const palette = [...source.eventPalette];
  const sources: ThemeSources =
    source.kind === "user"
      ? { ...source.sources }
      : synthesizeSourcesFromResolved(resolvedApp);
  const appIsolated =
    source.kind === "user"
      ? normalizeSemanticSignalAppIsolated(source.appIsolated)
      : new Set<string>();
  const calendarIsolated =
    source.kind === "user"
      ? new Set(source.calendarIsolated)
      : new Set<string>();
  // Carry the source's iconLabel verbatim. Built-ins peg iconLabel to base,
  // so a clone of "Light" starts as iconLabel=light. Cloning a user theme
  // that was manually flipped preserves the flip on the clone.
  const iconLabel = source.iconLabel;
  const calendarDefaultMode =
    source.kind === "user" ? source.calendarDefaultMode : source.base;
  const calendarDefaultCustom =
    source.kind === "user"
      ? source.calendarDefaultCustom
      : (sources.canvas ?? DEFAULT_CALENDAR_DEFAULT_CUSTOM);
  return {
    kind: "user",
    id,
    displayName,
    iconLabel,
    blendCanvas: source.blendCanvas,
    eventPalette: palette,
    derivationEngineVersion:
      source.kind === "user"
        ? source.derivationEngineVersion
        : DERIVATION_ENGINE_VERSION,
    calendarDefaultMode,
    calendarDefaultCustom,
    sources,
    appTokens: syncSemanticSignalAppTokens(sources, resolvedApp),
    calendarTokens: { ...resolvedCalendarTokens },
    appIsolated,
    calendarIsolated: calendarIsolated,
    seedSources: { ...sources },
    seedAppTokens: syncSemanticSignalAppTokens(sources, resolvedApp),
    seedCalendarTokens: { ...resolvedCalendarTokens },
    seedAppIsolated: new Set(appIsolated),
    seedCalendarIsolated: new Set(calendarIsolated),
    seedEventPalette: [...palette],
    seedBlendCanvas: source.blendCanvas,
    seedCalendarDefaultMode: calendarDefaultMode,
    seedCalendarDefaultCustom: calendarDefaultCustom,
    seedIconLabel: iconLabel,
  };
}

/**
 * Return a user-theme-shaped snapshot for surfaces that need the full
 * editable schema without registering or mutating a theme. Built-ins do not
 * store sources or token snapshots, so they are projected through the same
 * clone path used by Duplicate and edit.
 */
export function toUserThemeSnapshot(source: Theme): UserTheme {
  if (source.kind === "user") return source;
  return cloneTheme(source, source.id, source.displayName);
}

/**
 * Return the import-blocking error for a theme id that already belongs to
 * the current registry. New imports must not silently shadow or rename
 * either built-in or user-authored themes.
 */
export function themeIdCollisionError(
  id: ThemeId,
  registry: Readonly<Record<ThemeId, Theme>>,
): string | undefined {
  const existing = registry[id];
  if (!existing) return undefined;
  if (existing.kind === "builtin") return "id must not collide with a built-in theme";
  return `id "${id}" is already used by another theme`;
}

/**
 * Decide whether a single editor row has its own reset action available.
 * Linked app/calendar rows follow their source, so source-driven value drift
 * must not make a child row independently resettable.
 */
export function canResetTokenToSeed(
  theme: UserTheme,
  kind: ThemeTokenKind,
  key: string,
): boolean {
  if (kind === "source") {
    const sourceKey = key as keyof ThemeSources;
    return theme.sources[sourceKey] !== theme.seedSources[sourceKey];
  }

  const liveIsolated =
    kind === "app"
      ? theme.appIsolated.has(key)
      : theme.calendarIsolated.has(key);
  const seedIsolated =
    kind === "app"
      ? theme.seedAppIsolated.has(key)
      : theme.seedCalendarIsolated.has(key);
  if (liveIsolated !== seedIsolated) return true;
  if (!liveIsolated) return false;

  const liveValue =
    kind === "app" ? theme.appTokens[key] : theme.calendarTokens[key];
  const seedValue =
    kind === "app" ? theme.seedAppTokens[key] : theme.seedCalendarTokens[key];
  return liveValue !== seedValue;
}

function pickTokenSnapshot(
  source: Readonly<Record<string, string>>,
  keys: readonly string[],
): Record<string, string> {
  const out: Record<string, string> = {};
  for (const key of keys) {
    const value = source[key];
    if (value !== undefined) out[key] = value;
  }
  return out;
}

/**
 * Synthesize a source palette from a resolved app-token
 * snapshot. Used when cloning a built-in theme into a user theme: every
 * UserTheme requires sources, and the resolved tokens carry the right
 * starting points (canvas = --background, ink = --foreground, etc.).
 */
function synthesizeSourcesFromResolved(
  resolvedApp: Readonly<Record<string, string>>,
): ThemeSources {
  return {
    canvas: resolvedApp["--background"],
    ink: resolvedApp["--foreground"],
    primary: resolvedApp["--primary"],
    destructive: resolvedApp["--destructive"],
    destructiveText: resolvedApp["--destructive-foreground"],
    confirm: resolvedApp["--action-confirm"],
    confirmText: resolvedApp["--action-confirm-foreground"],
    warning: resolvedApp["--status-tentative"],
    warningText: resolvedApp["--status-tentative-foreground"],
  };
}

type UserThemePatch = Partial<
  Pick<UserTheme, "displayName" | "eventPalette" | "blendCanvas">
>;

/**
 * Merge a small subset of fields onto a user theme. Token and source edits
 * go through targeted store mutators (`updateSourceValue`, `isolateToken`);
 * this helper covers the fields outside those paths: display name,
 * event palette, and blend canvas.
 */
export function mergeThemePatch(
  current: UserTheme,
  patch: UserThemePatch,
): UserTheme {
  return {
    ...current,
    displayName: patch.displayName ?? current.displayName,
    eventPalette: patch.eventPalette
      ? [...patch.eventPalette]
      : current.eventPalette,
    blendCanvas: patch.blendCanvas ?? current.blendCanvas,
  };
}

/**
 * Pick a display name that does not collide with any name in `existing`.
 * Comparison is case-insensitive (matches what users perceive). Falls back
 * to the unsuffixed base after a sanity-bound number of attempts.
 */
export function nextUniqueDisplayName(
  base: string,
  existing: Iterable<string>,
): string {
  const taken = new Set<string>();
  for (const name of existing) taken.add(name.toLowerCase());
  if (!taken.has(base.toLowerCase())) return base;
  for (let i = 2; i < NAME_SUFFIX_CAP; i++) {
    const candidate = `${base} ${i}`;
    if (!taken.has(candidate.toLowerCase())) return candidate;
  }
  return base;
}

/**
 * Trim and length-cap a user-supplied display name. Returns `undefined` if
 * the name is empty after trimming so callers can reject the rename.
 */
export function normalizeDisplayName(input: string): string | undefined {
  const trimmed = input.trim();
  if (trimmed.length === 0) return undefined;
  return trimmed.slice(0, MAX_DISPLAY_NAME_LENGTH);
}
