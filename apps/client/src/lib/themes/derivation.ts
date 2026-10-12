import {
  pickBrightForeground,
  pickReadableBorder,
  pickReadableForeground,
  relativeLuminance,
  shiftPerceptualL,
  walkFraction,
} from "$lib/color/math";

import {
  APP_TOKEN_KEYS,
  BASE_APP_TOKENS,
  BASE_CALENDAR_TOKENS,
  CALENDAR_TOKEN_KEYS,
  DEFAULT_CALENDAR_DEFAULT_CUSTOM,
  darkTheme,
  lightTheme,
  syncSemanticSignalAppTokens,
  type CalendarColorDefaultMode,
  type CalendarTokenKey,
  type EventPaletteHexes,
  type Theme,
  type ThemeSources,
} from "./definitions";

const DERIVED_APP_TOKEN_KEYS: readonly string[] = Object.freeze([
  "--event-panel-edge",
  "--event-panel-shadow",
  "--event-panel-divider",
  "--event-panel-input-text",
  "--event-panel-placeholder",
  "--form-indicator",
] as const);

/**
 * Signed OKLab lightness offsets that drive every derived surface.
 *
 * Each delta is measured from the dark built-in's actual OKLab lightness
 * diff between `canvas` (#27282A, L=0.2766) and the corresponding surface
 * hex (card, popover, ...). Applying these deltas to any canvas via
 * `shiftPerceptualL` reproduces the dark built-in's surface hierarchy
 * regardless of canvas brightness:
 *
 * - card / popover / accent / secondary / muted lift upward
 * - sidebar recedes below canvas so the title bar frames the app (the
 *   "contrarian" step that makes the dark built-in read as layered)
 * - event panel sits just above canvas, and event-panel-contrast sits
 *   just below, keeping the panel's recessed band visible on any canvas
 *
 * The identity `deriveAppTokens(darkSources) === BASE_APP_TOKENS.dark`
 * holds on every shift-derived surface because the deltas are the
 * measured BASE_APP_TOKENS.dark OKLab-L differences from canvas. Any
 * custom canvas inherits the same relative surface stack by running
 * these same deltas.
 *
 * Near-white or near-black canvases clamp at the gamut boundary inside
 * `shiftPerceptualL`, so the hierarchy degrades gracefully instead of
 * wrapping. Foregrounds, borders, and muted captions are recomputed
 * from contrast math so legibility survives the clamp.
 */
const APP_DERIVATION = {
  cardDeltaL: +0.028345,
  popoverDeltaL: +0.056046,
  secondaryDeltaL: +0.048193,
  mutedDeltaL: +0.048193,
  accentDeltaL: +0.077214,
  sidebarDeltaL: -0.039433,
  sidebarAccentDeltaL: +0.077214,
  eventPanelBgDeltaL: +0.012636,
  eventPanelContrastDeltaL: -0.020696,
} as const;

/**
 * Per-token fractional walks calibrated from the dark built-in. Each
 * value is the fraction of the way from the chosen foreground anchor
 * toward the paired surface's OKLab lightness where BASE_APP_TOKENS.dark
 * sits. A contrast-target walk ("park at 3:1") cannot reproduce these
 * hexes because their contrast sits between 3:1 and AA (e.g.,
 * --muted-foreground at 4.9:1 is too deep for the 3:1 picker's landing
 * and too shallow for 4.5:1), so `walkFraction(fg, bg, f)` is used to
 * park each token at its BASE fraction.
 *
 * The chroma of the result comes from the foreground anchor (walk
 * preserves fg's `a`, `b`), which yields identity on dark for any token
 * whose BASE chroma is close to ink's. BASE hexes that were
 * hand-tuned with off-ink chroma (e.g., the slightly warmer
 * --event-panel-text) land within a few rgb units of the BASE hex.
 */
const APP_FRACTIONS = {
  mutedForeground: 0.442563,
  ring: 0.673365,
  eventPanelText: 0.180110,
  eventPanelInputText: 0.044202,
  eventPanelPlaceholder: 0.180110,
  eventPanelMutedText: 0.363909,
  eventPanelDivider: 0.839033,
} as const;

/**
 * Calendar-surface derivation offsets, measured from the built-ins.
 * - `calendarCanvasDarkDeltaL`: BASE.dark --cal-bg #131314 sits at ΔL -0.0894
 *   below canvas #27282A.
 * - `calendarCanvasLightDeltaL`: BASE.light --cal-bg #FFFFFF clamps to L=1,
 *   ΔL +0.0320 above canvas #F4F4F7 (L=0.968). Asymmetric by design:
 *   dark pulls cal-bg into a recessed framing; light pushes it to paper
 *   white so the app canvas reads as a tinted border around it.
 * - `timelineRailDarkDeltaL`: BASE.dark rail #3F3F46 sits at +0.1832
 *   above the derived cal-bg, elevating the empty track behind events.
 * - `timelineRailLightDeltaL`: BASE.light rail #E5E7EB sits at -0.0724
 *   below cal-bg, recessing the track on a paper-white surface.
 */
const CALENDAR_DERIVATION = {
  calendarCanvasDarkDeltaL: -0.089432,
  calendarCanvasLightDeltaL: +0.031968,
  timelineRailDarkDeltaL: +0.183151,
  timelineRailLightDeltaL: -0.072415,
} as const;

/**
 * Fractional walks for calendar tokens that land at specific OKLab-L
 * recessions against `cal-bg`. Calibrated from BASE_CALENDAR_TOKENS.dark.
 * --cal-time-label matches --muted-foreground's fraction; both land at
 * the same BASE hex #9494A0.
 */
const CALENDAR_FRACTIONS = {
  calendarTimeLabel: 0.362173,
} as const;

/**
 * Derive every app-shell token from a source palette. Every surface
 * (card, popover, secondary, muted, accent, sidebar, event panel)
 * moves canvas by a per-token OKLab ΔL offset calibrated from the dark
 * built-in, so the same "card above canvas, sidebar below" hierarchy
 * shows on every canvas regardless of brightness. Foregrounds, borders,
 * and muted captions are recomputed from contrast math so the pairing
 * stays legible regardless of which sources the user picks:
 * `pickReadableForeground` guarantees AA 4.5:1 on body text,
 * `pickBrightForeground` snaps title-bar text to pure white (or ink, or
 * black), semantic signal text comes from the matching text sources, and
 * `walkFraction` parks recessed captions at the exact OKLab-L position
 * BASE.dark uses.
 *
 * Built-in themes carry no `sources` field and never reach this function
 * at resolve time; user themes call it when sources are edited, rebaked, or
 * cloned.
 */
export function deriveAppTokens(
  sources: ThemeSources,
): Record<string, string> {
  const {
    canvas,
    ink,
    primary,
    destructive,
    destructiveText,
    confirm,
    confirmText,
    warning,
    warningText,
  } = sources;
  const deltas = APP_DERIVATION;
  const fractions = APP_FRACTIONS;
  const shift = (deltaL: number) => shiftPerceptualL(canvas, deltaL);
  const fg = (bg: string, target = 4.5) =>
    pickReadableForeground(bg, { ink, canvas, target });
  // Direction-aware walk anchor. Recessed tokens (muted captions, rings,
  // event-panel hint text, form indicator) must sit between their paired
  // surface and a *visible* foreground, not between the surface and raw
  // ink. When the user keeps a light ink but drags canvas to white, raw
  // ink collapses against every near-white surface and walkFraction
  // produces invisible light-gray text. Anchoring on a contrast-picked
  // foreground for each bg flips direction with canvas so the walk always
  // starts from a visible point. On BASE.dark (dark canvas + light ink),
  // `pickReadableForeground` returns ink on every app surface, so
  // dark-BASE identity is preserved.
  const anchorFor = (bg: string) =>
    pickReadableForeground(bg, { ink, canvas, target: 4.5 });
  const walk = (bg: string, fraction: number) =>
    walkFraction(anchorFor(bg), bg, fraction);
  const bright = (bg: string, target = 4.5) =>
    pickBrightForeground(bg, ink, target);
  const canvasIsDark = relativeLuminance(canvas) < 0.5;
  const card = shift(deltas.cardDeltaL);
  const popover = shift(deltas.popoverDeltaL);
  const secondary = shift(deltas.secondaryDeltaL);
  const mutedBg = shift(deltas.mutedDeltaL);
  const accent = shift(deltas.accentDeltaL);
  const sidebar = shift(deltas.sidebarDeltaL);
  const sidebarAccent = shift(deltas.sidebarAccentDeltaL);
  const eventPanelBg = shift(deltas.eventPanelBgDeltaL);
  const eventPanelContrast = shift(deltas.eventPanelContrastDeltaL);
  return {
    "--background": canvas,
    "--cal-header-bg": canvas,
    "--card": card,
    "--card-foreground": fg(card),
    "--popover": popover,
    "--popover-foreground": fg(popover),
    "--secondary": secondary,
    "--secondary-foreground": fg(secondary),
    "--muted": mutedBg,
    "--muted-foreground": walk(mutedBg, fractions.mutedForeground),
    "--accent": accent,
    "--accent-foreground": fg(accent),
    "--ring": walk(canvas, fractions.ring),
    "--sidebar": sidebar,
    "--sidebar-foreground": bright(sidebar, 4.5),
    "--sidebar-accent": sidebarAccent,
    "--sidebar-accent-foreground": bright(sidebarAccent, 4.5),
    "--event-panel-bg": eventPanelBg,
    "--event-panel-contrast": eventPanelContrast,
    "--event-panel-text": walk(eventPanelBg, fractions.eventPanelText),
    "--event-panel-muted-text": walk(eventPanelBg, fractions.eventPanelMutedText),
    "--event-panel-edge": canvasIsDark ? "#0000008C" : "#0000004D",
    "--event-panel-shadow": canvasIsDark ? "#00000066" : "#0000001F",
    "--event-panel-divider": walk(eventPanelBg, fractions.eventPanelDivider),
    "--event-panel-input-text": walk(eventPanelBg, fractions.eventPanelInputText),
    "--event-panel-placeholder": walk(eventPanelBg, fractions.eventPanelPlaceholder),
    "--foreground": fg(canvas),
    "--form-indicator": anchorFor(canvas),
    "--primary": primary,
    "--primary-foreground": fg(primary),
    "--destructive": destructive,
    "--destructive-foreground": destructiveText,
    "--action-danger-armed": destructive,
    "--action-danger-armed-foreground": destructiveText,
    "--status-declined": destructive,
    "--status-declined-foreground": destructiveText,
    "--action-confirm": confirm,
    "--action-confirm-foreground": confirmText,
    "--status-accepted": confirm,
    "--status-accepted-foreground": confirmText,
    "--status-tentative": warning,
    "--status-tentative-foreground": warningText,
  };
}

/**
 * Derive the calendar-shell tokens that can be computed from sources.
 *
 * `--cal-bg` is derived from the bundle basis via a direction-aware OKLab
 * ΔL. In app-canvas mode the basis is the app canvas, so editing App
 * canvas cascades through the internal calendar surface. Users who want a
 * fully independent surface can isolate `--cal-bg`; pinned values win over
 * derived values during source edits and rebakes.
 *
 * Gridlines are parked just above a minimum-visibility contrast against
 * `--cal-bg`. The target is intentionally subtle (1.4:1) to match how the
 * dark built-in renders its grid: a 3:1 target produces gridlines noticeably more prominent than
 * the built-in's curated hex, which users read as "uglier" on clones.
 *
 * The semantic marker tokens (current time, timeline break, timeline
 * focus) are intentionally omitted: those colors carry hard-coded meaning
 * that does not reduce to the source palette, so the resolver falls
 * through to the base CSS defaults for them.
 */
export function deriveCalendarTokens(
  sources: ThemeSources,
): Record<string, string> {
  const { canvas, ink } = sources;
  const canvasIsDark = relativeLuminance(canvas) < 0.5;
  const calendarCanvasDelta = canvasIsDark
    ? CALENDAR_DERIVATION.calendarCanvasDarkDeltaL
    : CALENDAR_DERIVATION.calendarCanvasLightDeltaL;
  const calendarCanvas = shiftPerceptualL(canvas, calendarCanvasDelta);
  const calendarCanvasIsDark = relativeLuminance(calendarCanvas) < 0.5;
  const timelineRailDelta = calendarCanvasIsDark
    ? CALENDAR_DERIVATION.timelineRailDarkDeltaL
    : CALENDAR_DERIVATION.timelineRailLightDeltaL;
  // Same direction-aware anchor as deriveAppTokens: walk-fraction tokens
  // must start from a foreground that is actually visible against the
  // calendar surface, not from raw ink. Dark-BASE parity still holds
  // because pickReadableForeground returns ink on BASE.dark's calendarCanvas.
  const anchorFor = (bg: string) =>
    pickReadableForeground(bg, { ink, canvas, target: 4.5 });
  return {
    "--cal-bg": calendarCanvas,
    "--cal-gridline": pickReadableBorder(calendarCanvas, ink, { target: 1.4 }),
    "--cal-time-label": walkFraction(
      anchorFor(calendarCanvas),
      calendarCanvas,
      CALENDAR_FRACTIONS.calendarTimeLabel,
    ),
    "--cal-timeline-rail": shiftPerceptualL(calendarCanvas, timelineRailDelta),
  };
}

export interface CalendarColorDefaultBundle {
  calendarTokens: Record<CalendarTokenKey, string>;
  runtimeTokens: Record<CalendarRuntimeTokenKey, string>;
  eventPalette: EventPaletteHexes;
  blendCanvas: string;
  paletteBase: "light" | "dark";
}

type CalendarRuntimeTokenKey =
  | "--cal-scrollbar-thumb"
  | "--cal-scrollbar-thumb-hover";

const CALENDAR_RUNTIME_TOKEN_KEYS = Object.freeze([
  "--cal-scrollbar-thumb",
  "--cal-scrollbar-thumb-hover",
] as const satisfies readonly CalendarRuntimeTokenKey[]);

const CALENDAR_SCROLLBAR_THUMB_CONTRAST_TARGET = 1.6;
const CALENDAR_SCROLLBAR_THUMB_HOVER_CONTRAST_TARGET = 4.5;

function calendarRuntimeTokensFromAppTokens(
  appTokens: Readonly<Record<string, string>>,
): Record<CalendarRuntimeTokenKey, string> {
  return {
    "--cal-scrollbar-thumb":
      appTokens["--muted"] ?? BASE_APP_TOKENS.dark["--muted"],
    "--cal-scrollbar-thumb-hover":
      appTokens["--muted-foreground"] ??
      BASE_APP_TOKENS.dark["--muted-foreground"],
  };
}

function calendarRuntimeTokensFromCanvas(
  calendarCanvas: string,
): Record<CalendarRuntimeTokenKey, string> {
  const contrastAnchor =
    relativeLuminance(calendarCanvas) < 0.5 ? "#FFFFFF" : "#000000";
  return {
    "--cal-scrollbar-thumb": pickReadableBorder(calendarCanvas, contrastAnchor, {
      target: CALENDAR_SCROLLBAR_THUMB_CONTRAST_TARGET,
    }),
    "--cal-scrollbar-thumb-hover": pickReadableBorder(
      calendarCanvas,
      contrastAnchor,
      {
        target: CALENDAR_SCROLLBAR_THUMB_HOVER_CONTRAST_TARGET,
      },
    ),
  };
}

function fullCalendarSnapshot(
  base: "light" | "dark",
  derived: Readonly<Record<string, string>>,
): Record<CalendarTokenKey, string> {
  return buildSnapshot(
    CALENDAR_TOKEN_KEYS,
    BASE_CALENDAR_TOKENS[base],
    derived,
    {},
  ) as Record<CalendarTokenKey, string>;
}

export function deriveCalendarColorDefaultBundle(
  sources: ThemeSources,
  mode: CalendarColorDefaultMode,
  customBasis: string = DEFAULT_CALENDAR_DEFAULT_CUSTOM,
): CalendarColorDefaultBundle {
  if (mode === "light" || mode === "dark") {
    return {
      calendarTokens: { ...BASE_CALENDAR_TOKENS[mode] } as Record<
        CalendarTokenKey,
        string
      >,
      runtimeTokens: calendarRuntimeTokensFromAppTokens(BASE_APP_TOKENS[mode]),
      eventPalette: mode === "light" ? lightTheme.eventPalette : darkTheme.eventPalette,
      blendCanvas: BASE_CALENDAR_TOKENS[mode]["--cal-bg"],
      paletteBase: mode,
    };
  }

  const basis = mode === "custom" ? customBasis : sources.canvas;
  const basisSources: ThemeSources = { ...sources, canvas: basis };
  const derived = deriveCalendarTokens(basisSources);
  const calendarCanvas = derived["--cal-bg"] ?? basis;
  const paletteBase: "light" | "dark" = defaultIconLabelFromCanvas(calendarCanvas);
  const calendarTokens = fullCalendarSnapshot(paletteBase, derived);
  return {
    calendarTokens,
    runtimeTokens: calendarRuntimeTokensFromCanvas(
      calendarTokens["--cal-bg"],
    ),
    eventPalette:
      paletteBase === "light" ? lightTheme.eventPalette : darkTheme.eventPalette,
    blendCanvas: calendarTokens["--cal-bg"],
    paletteBase,
  };
}

/**
 * Resolve every app-shell token for a theme. User themes paint editable
 * values from the stored snapshot and derive implementation-only values at
 * runtime; built-ins fall through to the base CSS rules.
 */
export function resolveAppTokens(theme: Theme): Record<string, string> {
  if (theme.kind === "user") {
    const derived = deriveAppTokens(theme.sources);
    const editable = syncSemanticSignalAppTokens(
      theme.sources,
      pickTokens(theme.appTokens, APP_TOKEN_KEYS),
    );
    return {
      ...editable,
      ...pickTokens(derived, DERIVED_APP_TOKEN_KEYS),
    };
  }
  return { ...BASE_APP_TOKENS[theme.base] };
}

function pickTokens(
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
 * Calendar-shell counterpart to {@link resolveAppTokens}. User themes paint
 * editable values from the stored snapshot; built-ins fall through to the
 * base CSS.
 */
export function resolveCalendarTokens(theme: Theme): Record<string, string> {
  if (theme.kind === "user") {
    const runtime = deriveCalendarColorDefaultBundle(
      theme.sources,
      theme.calendarDefaultMode,
      theme.calendarDefaultCustom,
    ).runtimeTokens;
    return {
      ...pickTokens(theme.calendarTokens, CALENDAR_TOKEN_KEYS),
      ...pickTokens(runtime, CALENDAR_RUNTIME_TOKEN_KEYS),
    };
  }
  return {
    ...BASE_CALENDAR_TOKENS[theme.base],
    ...calendarRuntimeTokensFromAppTokens(BASE_APP_TOKENS[theme.base]),
  };
}

/**
 * Luminance cutoff below which a surface is treated as "dark" for binary
 * decisions like applying Tailwind's `.dark` class or picking dark-calendar
 * contrast behavior. Sits at 0.4 so any sub-midpoint canvas is treated
 * as dark, but a mid-gray canvas (~#888 at 0.5) still resolves as light.
 * Only a bucket test, not a WCAG contrast check.
 */
const DARK_SURFACE_THRESHOLD = 0.4;

/**
 * Resolve the effective app background a theme will actually paint. Reads
 * the token snapshot for user themes and the base CSS rule for built-ins.
 * Used to drive luminance-aware decisions from the actual painted color
 * rather than the cosmetic label.
 */
export function resolveCanvas(theme: Theme): string {
  if (theme.kind === "user") return theme.appTokens["--background"];
  return BASE_APP_TOKENS[theme.base]["--background"];
}

/**
 * Resolve the effective calendar background. Used to pick event text,
 * dimming, and calendar outline mixes based on the actual painted surface.
 */
export function resolveCalendarCanvas(theme: Theme): string {
  if (theme.kind === "user") return theme.calendarTokens["--cal-bg"];
  return BASE_CALENDAR_TOKENS[theme.base]["--cal-bg"];
}

/** True when the resolved app canvas crosses into dark-mode territory. */
export function isThemeDark(theme: Theme): boolean {
  return relativeLuminance(resolveCanvas(theme)) < DARK_SURFACE_THRESHOLD;
}

/**
 * Pick the default decorative iconLabel for a canvas hex. Used by the clone
 * path and derived theme defaults.
 */
export function defaultIconLabelFromCanvas(
  canvasHex: string,
): "light" | "dark" {
  return relativeLuminance(canvasHex) < DARK_SURFACE_THRESHOLD ? "dark" : "light";
}

/** True when the resolved calendar canvas crosses into dark-mode territory. */
export function isThemeCalendarDark(theme: Theme): boolean {
  return relativeLuminance(resolveCalendarCanvas(theme)) < DARK_SURFACE_THRESHOLD;
}

/**
 * Build a full token snapshot by layering derived values over base CSS,
 * then overrides over derived. Every key in `order` ends up in the result.
 */
function buildSnapshot(
  order: readonly string[],
  base: Readonly<Record<string, string>>,
  derived: Readonly<Record<string, string>>,
  overrides: Readonly<Record<string, string>>,
): Record<string, string> {
  const out: Record<string, string> = {};
  for (const key of order) {
    out[key] = overrides[key] ?? derived[key] ?? base[key];
  }
  return out;
}

/**
 * Compute the CSS custom property changes needed to apply a theme when
 * `previouslyApplied` tokens were set by the last theme.
 *
 * User themes paint their editable token snapshot plus runtime-only derived
 * implementation tokens; built-ins paint nothing and let the base CSS rules
 * show through. The result tells the caller which tokens to set, which
 * leftover keys to clear, and the new set of applied keys to remember for
 * the next switch (without this bookkeeping, switching from a user theme to
 * a built-in would leave the previous theme's values stuck on the root).
 */
export function computeThemeTokenOps(
  theme: Theme,
  previouslyApplied: ReadonlySet<string>,
): {
  toSet: ReadonlyMap<string, string>;
  toClear: ReadonlySet<string>;
  applied: Set<string>;
} {
  const toSet = new Map<string, string>();
  const applied = new Set<string>();
  if (theme.kind === "user") {
    for (const [key, value] of Object.entries(resolveAppTokens(theme))) {
      toSet.set(key, value);
      applied.add(key);
    }
    for (const [key, value] of Object.entries(resolveCalendarTokens(theme))) {
      toSet.set(key, value);
      applied.add(key);
    }
  }
  const toClear = new Set<string>();
  for (const key of previouslyApplied) {
    if (!applied.has(key)) toClear.add(key);
  }
  return { toSet, toClear, applied };
}
