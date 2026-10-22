/**
 * SQLite bridge for user themes.
 *
 * Built-in light and dark themes stay code-pinned (in `themes/definitions.ts`)
 * and are never inserted here; the `themes.id` CHECK constraint also rejects
 * their IDs at the SQL level.
 *
 * Tokens live as one row per (theme_id, kind, key) in `theme_tokens`:
 *   kind='source'   keys are bare names (canvas, ink, primary, ...)
 *   kind='app'      keys are app CSS custom property names
 *   kind='calendar' keys are calendar CSS custom property names
 *
 * Reads and writes follow `THEME_TOKEN_ROW_ORDER` so raw rows stay aligned
 * with the editor's section order without adding a persisted sort column.
 *
 * `theme_seed_tokens` mirrors the same shape; per-row reset and
 * "Reset all" copy from seed back to live.
 *
 * Durable theme writes go through sqlx-backed Rust commands, and multi-row
 * mutations commit in one transaction.
 */

import { invoke } from "@tauri-apps/api/core";
import { THEME_TOKEN_ROW_ORDER } from "$lib/themes";
import { ensureDbUrl } from "$lib/api/db";
import { mapSyncDeviceRef, type SyncDeviceRef } from "$lib/api/sync";

export type TokenKind = "source" | "app" | "calendar";

export interface TokenRow {
  theme_id: string;
  kind: TokenKind;
  key: string;
  value: string;
  isolated: number;
}

export interface PaletteRow {
  theme_id: string;
  slot: number;
  value: string;
}

export interface ThemeRow {
  id: string;
  display_name: string;
  blend_canvas: string;
  seed_blend_canvas: string;
  derivation_engine_version: number;
  calendar_default_mode: string;
  calendar_default_custom: string;
  seed_calendar_default_mode: string;
  seed_calendar_default_custom: string;
  /** Decorative sun/moon tag for the theme list and editor icon. */
  icon_label: "light" | "dark";
  seed_icon_label: "light" | "dark";
  created_at_ms: number;
  updated_at_ms: number;
  /** Whether concurrent saves on linked devices left content versions to choose from. */
  has_conflict: boolean;
}

export interface DismissalRow {
  theme_id: string;
  engine_version: number;
}

/**
 * A user theme as raw row groups for `insertTheme` and
 * `replaceThemeContent`. The store builds one from its in-memory
 * `UserTheme` before writing, so this layer stays independent of the
 * store's types.
 */
export interface UserThemeWrite {
  id: string;
  displayName: string;
  iconLabel: "light" | "dark";
  seedIconLabel: "light" | "dark";
  blendCanvas: string;
  seedBlendCanvas: string;
  derivationEngineVersion: number;
  calendarDefaultMode: string;
  calendarDefaultCustom: string;
  seedCalendarDefaultMode: string;
  seedCalendarDefaultCustom: string;
  tokens: ReadonlyArray<{
    kind: TokenKind;
    key: string;
    value: string;
    isolated: boolean;
  }>;
  palette: ReadonlyArray<{ slot: number; value: string }>;
  seedTokens: ReadonlyArray<{
    kind: TokenKind;
    key: string;
    value: string;
    isolated: boolean;
  }>;
  seedPalette: ReadonlyArray<{ slot: number; value: string }>;
}

/**
 * Materialized view of a theme's rows after `loadAllUserThemes`. Caller
 * is responsible for shaping these into the in-memory `UserTheme`.
 */
export interface UserThemeRead {
  theme: ThemeRow;
  tokens: TokenRow[];
  palette: PaletteRow[];
  seedTokens: TokenRow[];
  seedPalette: PaletteRow[];
}

const TOKEN_ORDER_INDEX: ReadonlyMap<string, number> = new Map(
  THEME_TOKEN_ROW_ORDER.map((entry, index) => [`${entry.kind}:${entry.key}`, index] as const),
);

async function activeDbUrl(): Promise<string> {
  return ensureDbUrl();
}

function compareTokenRows(a: TokenRow, b: TokenRow): number {
  const aOrder = TOKEN_ORDER_INDEX.get(`${a.kind}:${a.key}`) ?? THEME_TOKEN_ROW_ORDER.length;
  const bOrder = TOKEN_ORDER_INDEX.get(`${b.kind}:${b.key}`) ?? THEME_TOKEN_ROW_ORDER.length;
  return a.theme_id.localeCompare(b.theme_id)
    || aOrder - bOrder
    || a.kind.localeCompare(b.kind)
    || a.key.localeCompare(b.key);
}

function comparePaletteRows(a: PaletteRow, b: PaletteRow): number {
  return a.theme_id.localeCompare(b.theme_id) || a.slot - b.slot;
}

export async function loadAllUserThemes(): Promise<UserThemeRead[]> {
  const rows = await invoke<UserThemeRead[]>("themes_load_all", {
    dbUrl: await activeDbUrl(),
  });
  return rows.map(sortedRead);
}

function sortedRead(read: UserThemeRead): UserThemeRead {
  return {
    theme: read.theme,
    tokens: [...read.tokens].sort(compareTokenRows),
    palette: [...read.palette].sort(comparePaletteRows),
    seedTokens: [...read.seedTokens].sort(compareTokenRows),
    seedPalette: [...read.seedPalette].sort(comparePaletteRows),
  };
}

export async function insertTheme(write: UserThemeWrite): Promise<void> {
  await invoke<void>("themes_insert", { dbUrl: await activeDbUrl(), write });
}

export async function deleteTheme(id: string): Promise<void> {
  await invoke<void>("themes_delete", { dbUrl: await activeDbUrl(), id });
}

/**
 * Replace a theme's live content (name, icon, tokens, palette, blend
 * canvas, and Calendar defaults) in one transaction. The seed fields of
 * `write` are ignored because a theme's seed is fixed at creation, and
 * the parent row keeps its identity, creation time, and upgrade
 * dismissal. Used by the editor Save and the JSON-paste replace path.
 *
 * A save that changes nothing writes nothing, so it does not replicate
 * to linked devices or open a conflict there.
 */
export async function replaceThemeContent(write: UserThemeWrite): Promise<void> {
  await invoke<void>("themes_replace_content", {
    dbUrl: await activeDbUrl(),
    write,
  });
}

export async function renameTheme(
  id: string,
  displayName: string,
): Promise<void> {
  await invoke<void>("themes_rename", {
    dbUrl: await activeDbUrl(),
    id,
    displayName,
  });
}

/**
 * Reset a single token to its seed value and seed isolated flag. Used by
 * the per-row reset icon in the editor.
 */
export async function resetTokenToSeed(
  id: string,
  kind: TokenKind,
  key: string,
): Promise<void> {
  await invoke<void>("themes_reset_token_to_seed", {
    dbUrl: await activeDbUrl(),
    id,
    kind,
    key,
  });
}

/**
 * Restore every token, palette slot, and blend_canvas to its seed value.
 * Used by the editor footer's "Reset all" button.
 */
export async function resetThemeToSeed(id: string): Promise<void> {
  await invoke<void>("themes_reset_to_seed", { dbUrl: await activeDbUrl(), id });
}

export async function recordDismissal(
  id: string,
  engineVersion: number,
): Promise<void> {
  await invoke<void>("themes_record_dismissal", {
    dbUrl: await activeDbUrl(),
    id,
    engineVersion,
  });
}

export async function loadDismissals(): Promise<DismissalRow[]> {
  return invoke<DismissalRow[]>("themes_load_dismissals", {
    dbUrl: await activeDbUrl(),
  });
}

/** A conflict read or resolution failure; `resolved` means the conflict or version is gone. */
export class ThemeConflictError extends Error {
  constructor(readonly code: "resolved" | "failed", message: string) {
    super(message);
    this.name = "ThemeConflictError";
  }
}

/** One token of a conflicting version. */
export interface ThemeConflictToken {
  kind: TokenKind;
  key: string;
  value: string;
  isolated: boolean;
}

/** One concurrent version of a theme's content. */
export interface ThemeConflictVersion {
  /** Opaque id for choosing this version. */
  version: string;
  device: SyncDeviceRef;
  editedAtMs: number;
  /** Whether every device shows this version now. */
  displayed: boolean;
  blendCanvas: string;
  derivationEngineVersion: number;
  calendarDefaultMode: string;
  calendarDefaultCustom: string;
  /** Tokens sorted by kind and key. */
  tokens: ThemeConflictToken[];
  /** Event palette colors in slot order. */
  palette: string[];
}

/** The content versions of a theme, newest first; empty when it has no conflict. */
export interface ThemeConflict {
  id: string;
  versions: ThemeConflictVersion[];
}

/** Keep the version every device shows, or use another one. */
export type ThemeConflictChoice =
  | { kind: "displayed" }
  | { kind: "version"; version: string };

export interface ThemeConflictResolution {
  id: string;
  choice: ThemeConflictChoice;
}

function record(value: unknown, label: string): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error(`${label} must be an object`);
  }
  return value as Record<string, unknown>;
}

function string(value: unknown, label: string): string {
  if (typeof value !== "string") throw new Error(`${label} must be a string`);
  return value;
}

function boolean(value: unknown, label: string): boolean {
  if (typeof value !== "boolean") throw new Error(`${label} must be a boolean`);
  return value;
}

function integer(value: unknown, label: string): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value)) {
    throw new Error(`${label} must be an integer`);
  }
  return value;
}

function array(value: unknown, label: string): unknown[] {
  if (!Array.isArray(value)) throw new Error(`${label} must be an array`);
  return value;
}

function tokenKind(value: unknown, label: string): TokenKind {
  if (value !== "source" && value !== "app" && value !== "calendar") throw new Error(`${label} is invalid`);
  return value;
}

function mapConflictToken(value: unknown, label: string): ThemeConflictToken {
  const row = record(value, label);
  return {
    kind: tokenKind(row.kind, `${label}.kind`),
    key: string(row.key, `${label}.key`),
    value: string(row.value, `${label}.value`),
    isolated: boolean(row.isolated, `${label}.isolated`),
  };
}

function mapConflictVersion(value: unknown, label: string): ThemeConflictVersion {
  const row = record(value, label);
  const editedAtMs = integer(row.editedAtMs, `${label}.editedAtMs`);
  if (editedAtMs < 0) throw new Error(`${label}.editedAtMs is invalid`);
  return {
    version: string(row.version, `${label}.version`),
    device: mapSyncDeviceRef(row.device, `${label}.device`),
    editedAtMs,
    displayed: boolean(row.displayed, `${label}.displayed`),
    blendCanvas: string(row.blendCanvas, `${label}.blendCanvas`),
    derivationEngineVersion: integer(row.derivationEngineVersion, `${label}.derivationEngineVersion`),
    calendarDefaultMode: string(row.calendarDefaultMode, `${label}.calendarDefaultMode`),
    calendarDefaultCustom: string(row.calendarDefaultCustom, `${label}.calendarDefaultCustom`),
    tokens: array(row.tokens, `${label}.tokens`).map((token, index) =>
      mapConflictToken(token, `${label}.tokens[${index}]`)),
    palette: array(row.palette, `${label}.palette`).map((color, index) =>
      string(color, `${label}.palette[${index}]`)),
  };
}

/** Validates the content conflict of a theme from the native side. */
export function mapThemeConflict(value: unknown): ThemeConflict {
  const row = record(value, "theme conflict");
  return {
    id: string(row.id, "theme conflict.id"),
    versions: array(row.versions, "theme conflict.versions").map((version, index) =>
      mapConflictVersion(version, `theme conflict.versions[${index}]`)),
  };
}

async function invokeConflict<T>(command: string, args: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, { dbUrl: await activeDbUrl(), ...args });
  } catch (error: unknown) {
    if (typeof error === "object" && error !== null && "code" in error && "message" in error
      && (error.code === "resolved" || error.code === "failed") && typeof error.message === "string") {
      throw new ThemeConflictError(error.code, error.message);
    }
    throw error;
  }
}

/** Reads the content versions that concurrent saves on linked devices left on a theme. */
export async function getThemeConflict(id: string): Promise<ThemeConflict> {
  return mapThemeConflict(await invokeConflict<unknown>("themes_conflict", { id }));
}

/** Resolves a theme's content conflict on every linked device and returns the theme as it now stands. */
export async function resolveThemeConflict(resolution: ThemeConflictResolution): Promise<UserThemeRead> {
  return sortedRead(await invokeConflict<UserThemeRead>("themes_resolve_conflict", { resolution }));
}
