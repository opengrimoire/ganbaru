/**
 * Stable ID identifying a theme. Built-in IDs are "light" and "dark".
 * User themes use generated slug IDs (see `generateThemeId`) to avoid
 * collisions.
 */
export type ThemeId = string;

/**
 * Source colors that drive most of the shell palette through the
 * derivation formulas in {@link deriveAppTokens} and, when the calendar
 * default mode is app-canvas, {@link deriveCalendarColorDefaultBundle}.
 *
 * - **canvas:** app background. The color visible in framing gaps and the
 *   Settings modal; also the reference the other app surfaces lift toward
 *   ink from. Calendar header follows this color by default. The internal
 *   calendar surface follows it only when the calendar default mode is
 *   app-canvas; users can still isolate `--cal-bg` when they want a fully
 *   independent value.
 * - **ink:** text base. Default text color and the color every "lifted"
 *   surface mixes a small fraction of to tint it. Also drives secondary
 *   text tokens (form indicator and event panel text).
 * - **primary:** brand/action accent used on highlighted buttons and links.
 * - **destructive / destructiveText:** danger background and text. These
 *   drive delete actions, armed-delete state, and declined attendance
 *   status as one semantic pair.
 * - **confirm / confirmText:** positive background and text. These drive
 *   the confirm button (save, active scope pill) and accepted attendance
 *   status as one semantic pair.
 * - **warning / warningText:** caution background and text. These drive
 *   the tentative attendance status and other warning surfaces as one
 *   semantic pair.
 *
 * Themes without `sources` fall back to the base CSS tokens unchanged;
 * sources exist purely to let a small number of color choices drive a
 * consistent palette across the shell.
 */
export interface ThemeSources {
  canvas: string;
  ink: string;
  primary: string;
  destructive: string;
  destructiveText: string;
  confirm: string;
  confirmText: string;
  warning: string;
  warningText: string;
}

export const SOURCE_KEY_ORDER = Object.freeze([
  "canvas",
  "ink",
  "primary",
  "destructive",
  "destructiveText",
  "confirm",
  "confirmText",
  "warning",
  "warningText",
] as const satisfies readonly (keyof ThemeSources)[]);

/**
 * Full palette for event color slots within a theme. Always exactly
 * PALETTE_SIZE entries (currently 32); each entry is a hex color the slot
 * resolves to. Events store the slot index, not the hex, so two themes can
 * assign the same slot index completely different colors and stored events
 * pick up the active theme's hex automatically when the user switches.
 *
 * Stored as an array (not an object) so order is intrinsic to the data
 * shape: a slot's position is its identity. JSON serialization preserves
 * the order without depending on object-key insertion semantics, and the
 * on-disk integer color (0..31) is tighter than any string key.
 */
export type EventPaletteHexes = readonly string[];

export type CalendarColorDefaultMode =
  | "light"
  | "dark"
  | "app-canvas"
  | "custom";

/**
 * Engine version stamp written onto every user theme at create, clone, or
 * import time. Bump whenever the derivation engine (APP_DERIVATION,
 * APP_FRACTIONS, CALENDAR_DERIVATION, CALENDAR_FRACTIONS, or any token-key list)
 * changes such that derived output shifts. The editor uses the gap between
 * a stored theme's stamp and this constant to render an opt-in "rebake"
 * banner so non-pinned colors do not silently drift across app updates.
 */
export const DERIVATION_ENGINE_VERSION = 6;

/**
 * A built-in theme ships with the app, never persists to SQLite, and paints
 * nothing onto the DOM beyond the base CSS rules. Built-ins carry the
 * minimum a theme needs: an id, display name, base (light or dark, used for
 * the base CSS lookup), a complete event palette, and the blend canvas the
 * dimmed event variants reference.
 */
export interface BuiltinTheme {
  kind: "builtin";
  id: ThemeId;
  displayName: string;
  base: "light" | "dark";
  /**
   * Decorative sun/moon tag surfaced as the icon next to the theme name.
   * Built-in themes pin this to their base; user themes carry it as a
   * separate editable label so the user can mark a theme for "day" or
   * "night" use independently of canvas luminance. The runtime `.dark`
   * class and event contrast bucket still come from the actual canvas
   * through `isThemeDark` / `isThemeCalendarDark`.
   */
  iconLabel: "light" | "dark";
  eventPalette: EventPaletteHexes;
  /** Reference bg dimmed event variants blend toward. Usually canvas bg. */
  blendCanvas: string;
}

/**
 * A user-authored theme persists in SQLite as a normalized snapshot. Every
 * shell token is stored as a resolved hex; the per-token `isolated` flag
 * controls whether each token participates in the next source-edit cascade
 * or is treated as user-pinned. Sources drive multi-token derivation only
 * at write time (source edits, rebake, clone); at runtime the
 * snapshot is the source of truth, which makes saved themes stable across
 * derivation-engine changes.
 *
 * Seeds capture the live state at clone time so per-row and "Reset all"
 * restore correctly. Seed isolated flags mirror the live flags so a clone
 * of a user theme keeps its pins through resets.
 */
export interface UserTheme {
  kind: "user";
  id: ThemeId;
  displayName: string;
  /**
   * Decorative sun/moon tag; flips the icon in the editor header and the
   * theme list. Has no effect on the runtime `.dark` class or the calendar
   * event contrast bucket, which still derive from canvas luminance.
   */
  iconLabel: "light" | "dark";
  eventPalette: EventPaletteHexes;
  /** Reference bg dimmed event variants blend toward. Usually canvas bg. */
  blendCanvas: string;
  /** Engine version stamp; drives the rebake banner. */
  derivationEngineVersion: number;
  /**
   * Default bundle used for the internal calendar area: calendar surface,
   * event palette, and calendar details. The header follows app canvas
   * separately through `--cal-header-bg`.
   */
  calendarDefaultMode: CalendarColorDefaultMode;
  /** Custom basis used when `calendarDefaultMode` is "custom". */
  calendarDefaultCustom: string;
  /** Source palette that powers source-edit derivation. */
  sources: ThemeSources;
  /** Full snapshot of every app-shell CSS token. */
  appTokens: Readonly<Record<string, string>>;
  /** Full snapshot of every calendar-shell CSS token. */
  calendarTokens: Readonly<Record<string, string>>;
  /** Keys of `appTokens` the user has pinned against future derivations. */
  appIsolated: ReadonlySet<string>;
  /** Keys of `calendarTokens` the user has pinned against future derivations. */
  calendarIsolated: ReadonlySet<string>;
  /** Clone-time snapshots used for per-row reset and "Reset all". */
  seedSources: ThemeSources;
  seedAppTokens: Readonly<Record<string, string>>;
  seedCalendarTokens: Readonly<Record<string, string>>;
  seedAppIsolated: ReadonlySet<string>;
  seedCalendarIsolated: ReadonlySet<string>;
  seedEventPalette: EventPaletteHexes;
  seedBlendCanvas: string;
  seedCalendarDefaultMode: CalendarColorDefaultMode;
  seedCalendarDefaultCustom: string;
  /** Clone-time iconLabel; "Reset all" restores `iconLabel` to this. */
  seedIconLabel: "light" | "dark";
}

/**
 * A theme is either a code-pinned built-in or a user-authored entry. The
 * `kind` discriminator picks which shape applies; built-ins are
 * synchronously available, user themes hydrate from SQLite at boot.
 */
export type Theme = BuiltinTheme | UserTheme;

// Built-in event palettes. Stored as positional arrays so the slot index
// (the value events save on disk) maps directly into the array. Additional
// themes can freely deviate from these hexes. Slot FALLBACK_COLOR_INDEX is
// the render-layer fallback; every theme must keep all PALETTE_SIZE
// positions filled.

const LIGHT_EVENT_PALETTE: EventPaletteHexes = Object.freeze([
  "#AD1457",
  "#D81B60",
  "#C62828",
  "#D50000",
  "#E67C73",
  "#D06A45",
  "#F4511E",
  "#F09300",
  "#F6BF26",
  "#E4C441",
  "#C0CA33",
  "#7CB342",
  "#33B679",
  "#0B8043",
  "#009688",
  "#4DB6AC",
  "#00ACC1",
  "#1E88E5",
  "#4285F4",
  "#3F51B5",
  "#7986CB",
  "#B39DDB",
  "#7E57C2",
  "#8E24AA",
  "#9E69AF",
  "#A65A4A",
  "#795548",
  "#5D4037",
  "#8D7B73",
  "#A79B8E",
  "#616161",
  "#546E7A",
]);

const DARK_EVENT_PALETTE: EventPaletteHexes = Object.freeze([
  "#C05476",
  "#D85675",
  "#CF4F58",
  "#E5484D",
  "#D6837A",
  "#C97959",
  "#E3683E",
  "#E0963C",
  "#E6B951",
  "#D8BE5E",
  "#BCC256",
  "#85AD59",
  "#55B080",
  "#489160",
  "#429A8E",
  "#64B7AB",
  "#4FB3C2",
  "#55A2DF",
  "#668BE1",
  "#6E72C3",
  "#828BC2",
  "#AE9CCE",
  "#9271C8",
  "#A75ABA",
  "#A479B1",
  "#B06B5E",
  "#957367",
  "#80665A",
  "#93847D",
  "#A5998C",
  "#7C7C7C",
  "#7B8FA0",
]);

export const lightTheme: BuiltinTheme = Object.freeze({
  kind: "builtin",
  id: "light",
  displayName: "Light default",
  base: "light",
  iconLabel: "light",
  eventPalette: LIGHT_EVENT_PALETTE,
  blendCanvas: "#ffffff",
});

export const darkTheme: BuiltinTheme = Object.freeze({
  kind: "builtin",
  id: "dark",
  displayName: "Dark default",
  base: "dark",
  iconLabel: "dark",
  eventPalette: DARK_EVENT_PALETTE,
  blendCanvas: "#131314",
});

/**
 * Registry of the themes that ship with the app. Frozen so it cannot be
 * mutated at runtime; user-authored themes live in the store layer and are
 * merged with this registry when the active theme is resolved.
 */
export const BUILTIN_THEME_REGISTRY: Readonly<Record<ThemeId, BuiltinTheme>> =
  Object.freeze({
    [lightTheme.id]: lightTheme,
    [darkTheme.id]: darkTheme,
  });

/** Theme ID used on first launch and when the stored ID is unknown. */
export const DEFAULT_THEME_ID: ThemeId = lightTheme.id;

export function pickQuickToggleTarget({
  activeId,
  activeIsDark,
  lightId,
  darkId,
}: {
  activeId: ThemeId;
  activeIsDark: boolean;
  lightId: ThemeId;
  darkId: ThemeId;
}): ThemeId {
  if (activeId === lightId) return darkId;
  if (activeId === darkId) return lightId;
  return activeIsDark ? lightId : darkId;
}

/**
 * Returns true when the given ID matches a theme that ships with the app.
 * Used to guard mutators (built-in themes are immutable; "edit" duplicates
 * a built-in into a user theme first).
 */
export function isBuiltinThemeId(id: ThemeId | undefined | null): boolean {
  if (!id || typeof id !== "string") return false;
  return Object.hasOwn(BUILTIN_THEME_REGISTRY, id);
}

/**
 * Look up a theme by ID. By default searches only the built-in registry;
 * pass the combined registry from the store to resolve user themes too.
 * Guards against prototype-chain keys via Object.hasOwn.
 */
export function getThemeById(
  id: ThemeId | undefined | null,
  registry: Readonly<Record<ThemeId, Theme>> = BUILTIN_THEME_REGISTRY,
): Theme | undefined {
  if (!id || typeof id !== "string") return undefined;
  if (!Object.hasOwn(registry, id)) return undefined;
  return registry[id];
}

/**
 * App-shell CSS custom properties a user theme is allowed to override.
 * Limited to hex-color tokens because the in-house color picker emits hex
 * only; tokens that ship as rgba (border alpha) or oklch (charts) are
 * excluded.
 */
export const APP_TOKEN_KEYS = Object.freeze([
  "--background",
  "--cal-header-bg",
  "--card",
  "--card-foreground",
  "--popover",
  "--popover-foreground",
  "--secondary",
  "--secondary-foreground",
  "--muted",
  "--muted-foreground",
  "--accent",
  "--accent-foreground",
  "--ring",
  "--sidebar",
  "--sidebar-foreground",
  "--sidebar-accent",
  "--sidebar-accent-foreground",
  "--event-panel-bg",
  "--event-panel-contrast",
  "--event-panel-text",
  "--event-panel-muted-text",
  "--foreground",
  "--primary",
  "--primary-foreground",
  "--destructive",
  "--destructive-foreground",
  "--action-danger-armed",
  "--action-danger-armed-foreground",
  "--status-declined",
  "--status-declined-foreground",
  "--action-confirm",
  "--action-confirm-foreground",
  "--status-accepted",
  "--status-accepted-foreground",
  "--status-tentative",
  "--status-tentative-foreground",
] as const);

/**
 * Calendar-shell CSS custom properties a user theme is allowed to override.
 * Same hex-only restriction as APP_TOKEN_KEYS; cal-hover (rgba) and
 * cal-header-row-h (px) are excluded.
 */
export const CALENDAR_TOKEN_KEYS = Object.freeze([
  "--cal-bg",
  "--cal-gridline",
  "--cal-time-label",
  "--cal-timeline-rail",
  "--cal-current-time",
  "--cal-timeline-break",
  "--cal-timeline-focus",
] as const);

export type AppTokenKey = (typeof APP_TOKEN_KEYS)[number];
export type CalendarTokenKey = (typeof CALENDAR_TOKEN_KEYS)[number];
export type ThemeTokenKind = "source" | "app" | "calendar";

const SEMANTIC_SIGNAL_TOKEN_ALIASES: ReadonlyArray<
  Readonly<{
    sourceKey: keyof ThemeSources;
    tokens: readonly AppTokenKey[];
  }>
> = Object.freeze([
  {
    sourceKey: "destructive",
    tokens: ["--destructive", "--action-danger-armed", "--status-declined"],
  },
  {
    sourceKey: "destructiveText",
    tokens: [
      "--destructive-foreground",
      "--action-danger-armed-foreground",
      "--status-declined-foreground",
    ],
  },
  {
    sourceKey: "confirm",
    tokens: ["--action-confirm", "--status-accepted"],
  },
  {
    sourceKey: "confirmText",
    tokens: [
      "--action-confirm-foreground",
      "--status-accepted-foreground",
    ],
  },
  {
    sourceKey: "warning",
    tokens: ["--status-tentative"],
  },
  {
    sourceKey: "warningText",
    tokens: ["--status-tentative-foreground"],
  },
] as const);

const SEMANTIC_SIGNAL_APP_TOKEN_KEY_SET: ReadonlySet<string> = new Set(
  SEMANTIC_SIGNAL_TOKEN_ALIASES.flatMap((alias) => alias.tokens),
);

/**
 * The semantic signal families are not independently pinnable app tokens.
 * They are aliases of the visible background/text source pairs, so isolated
 * flags for these tokens are dropped during load, import, and clone, and
 * ignored during source cascades.
 */
export function normalizeSemanticSignalAppIsolated(
  set: ReadonlySet<string>,
): Set<string> {
  const out = new Set<string>();
  for (const key of set) {
    if (!SEMANTIC_SIGNAL_APP_TOKEN_KEY_SET.has(key)) out.add(key);
  }
  return out;
}

export function isSemanticSignalAppToken(key: string): boolean {
  return SEMANTIC_SIGNAL_APP_TOKEN_KEY_SET.has(key);
}

export function syncSemanticSignalAppTokens(
  sources: ThemeSources,
  appTokens: Readonly<Record<string, string>>,
): Record<string, string> {
  const out: Record<string, string> = { ...appTokens };
  for (const alias of SEMANTIC_SIGNAL_TOKEN_ALIASES) {
    for (const token of alias.tokens) out[token] = sources[alias.sourceKey];
  }
  return out;
}

export type ThemeTokenRowOrderEntry =
  | Readonly<{ kind: "source"; key: keyof ThemeSources }>
  | Readonly<{ kind: "app"; key: AppTokenKey }>
  | Readonly<{ kind: "calendar"; key: CalendarTokenKey }>;

export const THEME_TOKEN_ROW_ORDER = Object.freeze([
  { kind: "source", key: "canvas" },
  { kind: "app", key: "--background" },
  { kind: "app", key: "--cal-header-bg" },
  { kind: "app", key: "--card" },
  { kind: "app", key: "--card-foreground" },
  { kind: "app", key: "--popover" },
  { kind: "app", key: "--popover-foreground" },
  { kind: "app", key: "--secondary" },
  { kind: "app", key: "--secondary-foreground" },
  { kind: "app", key: "--muted" },
  { kind: "app", key: "--muted-foreground" },
  { kind: "app", key: "--accent" },
  { kind: "app", key: "--accent-foreground" },
  { kind: "app", key: "--ring" },
  { kind: "app", key: "--sidebar" },
  { kind: "app", key: "--sidebar-foreground" },
  { kind: "app", key: "--sidebar-accent" },
  { kind: "app", key: "--sidebar-accent-foreground" },
  { kind: "calendar", key: "--cal-bg" },
  { kind: "calendar", key: "--cal-gridline" },
  { kind: "calendar", key: "--cal-time-label" },
  { kind: "calendar", key: "--cal-current-time" },
  { kind: "calendar", key: "--cal-timeline-rail" },
  { kind: "calendar", key: "--cal-timeline-break" },
  { kind: "calendar", key: "--cal-timeline-focus" },
  { kind: "app", key: "--event-panel-bg" },
  { kind: "app", key: "--event-panel-contrast" },
  { kind: "app", key: "--event-panel-text" },
  { kind: "app", key: "--event-panel-muted-text" },
  { kind: "source", key: "ink" },
  { kind: "app", key: "--foreground" },
  { kind: "source", key: "primary" },
  { kind: "app", key: "--primary" },
  { kind: "app", key: "--primary-foreground" },
  { kind: "source", key: "destructive" },
  { kind: "source", key: "destructiveText" },
  { kind: "app", key: "--destructive" },
  { kind: "app", key: "--destructive-foreground" },
  { kind: "app", key: "--action-danger-armed" },
  { kind: "app", key: "--action-danger-armed-foreground" },
  { kind: "app", key: "--status-declined" },
  { kind: "app", key: "--status-declined-foreground" },
  { kind: "source", key: "confirm" },
  { kind: "source", key: "confirmText" },
  { kind: "app", key: "--action-confirm" },
  { kind: "app", key: "--action-confirm-foreground" },
  { kind: "app", key: "--status-accepted" },
  { kind: "app", key: "--status-accepted-foreground" },
  { kind: "source", key: "warning" },
  { kind: "source", key: "warningText" },
  { kind: "app", key: "--status-tentative" },
  { kind: "app", key: "--status-tentative-foreground" },
] as const satisfies readonly ThemeTokenRowOrderEntry[]);

/**
 * Hex defaults for the app-shell tokens, mirrored from `app.css` so the
 * editor can show what a token resolves to without consulting the live DOM
 * (which reflects the currently active theme, not the one being edited).
 */
export const BASE_APP_TOKENS: Readonly<
  Record<"light" | "dark", Readonly<Record<string, string>>>
> = Object.freeze({
  light: Object.freeze({
    "--background": "#F4F4F7",
    "--cal-header-bg": "#F4F4F7",
    "--card": "#FFFFFF",
    "--card-foreground": "#141420",
    "--popover": "#FFFFFF",
    "--popover-foreground": "#141420",
    "--secondary": "#E2E2E7",
    "--secondary-foreground": "#141420",
    "--muted": "#E2E2E7",
    "--muted-foreground": "#646470",
    "--accent": "#E8E8ED",
    "--accent-foreground": "#141420",
    "--ring": "#8C8C98",
    "--sidebar": "#DCDCE2",
    "--sidebar-foreground": "#141420",
    "--sidebar-accent": "#CFCFD6",
    "--sidebar-accent-foreground": "#141420",
    "--event-panel-bg": "#F0F4F9",
    "--event-panel-contrast": "#E8EDF5",
    "--event-panel-edge": "#0000004D",
    "--event-panel-shadow": "#0000001F",
    "--event-panel-divider": "#C4C7C5",
    "--event-panel-input-text": "#1F1F1F",
    "--event-panel-placeholder": "#444746",
    "--event-panel-text": "#141420",
    "--event-panel-muted-text": "#646470",
    "--foreground": "#141420",
    "--form-indicator": "#6B6F6E",
    "--primary": "#404048",
    "--primary-foreground": "#F4F4F7",
    "--destructive": "#D93B3B",
    "--destructive-foreground": "#FFFFFF",
    "--action-danger-armed": "#D93B3B",
    "--action-danger-armed-foreground": "#FFFFFF",
    "--status-declined": "#D93B3B",
    "--status-declined-foreground": "#FFFFFF",
    "--action-confirm": "#059669",
    "--action-confirm-foreground": "#FFFFFF",
    "--status-accepted": "#059669",
    "--status-accepted-foreground": "#FFFFFF",
    "--status-tentative": "#F59E0B",
    "--status-tentative-foreground": "#FFFFFF",
  }),
  dark: Object.freeze({
    "--background": "#27282A",
    "--cal-header-bg": "#27282A",
    "--card": "#2E2F31",
    "--card-foreground": "#ECECF2",
    "--popover": "#353638",
    "--popover-foreground": "#ECECF2",
    "--secondary": "#333436",
    "--secondary-foreground": "#ECECF2",
    "--muted": "#333436",
    "--muted-foreground": "#9494A0",
    "--accent": "#3B3B3F",
    "--accent-foreground": "#ECECF2",
    "--ring": "#606070",
    "--sidebar": "#1E1E23",
    "--sidebar-foreground": "#FFFFFF",
    "--sidebar-accent": "#3B3B3F",
    "--sidebar-accent-foreground": "#FFFFFF",
    "--event-panel-bg": "#2A2B2E",
    "--event-panel-contrast": "#222325",
    "--event-panel-edge": "#0000008C",
    "--event-panel-shadow": "#00000066",
    "--event-panel-divider": "#444746",
    "--event-panel-input-text": "#E3E3E3",
    "--event-panel-placeholder": "#C4C7C5",
    "--event-panel-text": "#C4C7C5",
    "--event-panel-muted-text": "#9EA1A0",
    "--foreground": "#ECECF2",
    "--form-indicator": "#ECECF2",
    "--primary": "#ECECF2",
    "--primary-foreground": "#27282A",
    "--destructive": "#E54545",
    "--destructive-foreground": "#FFFFFF",
    "--action-danger-armed": "#E54545",
    "--action-danger-armed-foreground": "#FFFFFF",
    "--status-declined": "#E54545",
    "--status-declined-foreground": "#FFFFFF",
    "--action-confirm": "#065F46",
    "--action-confirm-foreground": "#D1FAE5",
    "--status-accepted": "#065F46",
    "--status-accepted-foreground": "#FFFFFF",
    "--status-tentative": "#F59E0B",
    "--status-tentative-foreground": "#FFFFFF",
  }),
});

/**
 * Hex defaults for the calendar-shell tokens, mirrored from `app.css`.
 */
export const BASE_CALENDAR_TOKENS: Readonly<
  Record<"light" | "dark", Readonly<Record<string, string>>>
> = Object.freeze({
  light: Object.freeze({
    "--cal-bg": "#FFFFFF",
    "--cal-gridline": "#DDDDE3",
    "--cal-time-label": "#646470",
    "--cal-timeline-rail": "#E5E7EB",
    "--cal-current-time": "#B83A3A",
    "--cal-timeline-break": "#000000",
    "--cal-timeline-focus": "#39965c",
  }),
  dark: Object.freeze({
    "--cal-bg": "#131314",
    "--cal-gridline": "#333537",
    "--cal-time-label": "#9494A0",
    "--cal-timeline-rail": "#3F3F46",
    "--cal-current-time": "#B83A3A",
    "--cal-timeline-break": "#000000",
    "--cal-timeline-focus": "#2a8049",
  }),
});

export const DEFAULT_CALENDAR_DEFAULT_CUSTOM =
  BASE_APP_TOKENS.dark["--background"];
