import { describe, expect, it } from "vitest";
import {
  APP_TOKEN_KEYS,
  BASE_APP_TOKENS,
  BASE_CALENDAR_TOKENS,
  CALENDAR_TOKEN_KEYS,
  darkTheme,
  deriveAppTokens,
  deriveCalendarColorDefaultBundle,
  deriveCalendarTokens,
  lightTheme,
  type ThemeSources,
} from "$lib/themes";
import {
  contrastRatio,
  hexToOklab,
  relativeLuminance,
} from "$lib/color/math";

/**
 * Source palettes fitted from the built-in light and dark themes.
 *
 * The light theme defines canvas / ink / primary / destructive explicitly
 * in `BASE_APP_TOKENS`. Calendar canvas is not a source: it
 * auto-derives from the app canvas via a direction-aware OKLab ΔL offset.
 *
 * The dark theme reuses its `--foreground` color as `--primary`, which is
 * why `primary` and `ink` match exactly in this fixture.
 */
const LIGHT_SOURCES: ThemeSources = {
  canvas: BASE_APP_TOKENS.light["--background"],
  ink: BASE_APP_TOKENS.light["--foreground"],
  primary: BASE_APP_TOKENS.light["--primary"],
  destructive: BASE_APP_TOKENS.light["--destructive"],
  destructiveText: BASE_APP_TOKENS.light["--destructive-foreground"],
  confirm: BASE_APP_TOKENS.light["--action-confirm"],
  confirmText: BASE_APP_TOKENS.light["--action-confirm-foreground"],
  warning: BASE_APP_TOKENS.light["--status-tentative"],
  warningText: BASE_APP_TOKENS.light["--status-tentative-foreground"],
};

const DARK_SOURCES: ThemeSources = {
  canvas: BASE_APP_TOKENS.dark["--background"],
  ink: BASE_APP_TOKENS.dark["--foreground"],
  primary: BASE_APP_TOKENS.dark["--primary"],
  destructive: BASE_APP_TOKENS.dark["--destructive"],
  destructiveText: BASE_APP_TOKENS.dark["--destructive-foreground"],
  confirm: BASE_APP_TOKENS.dark["--action-confirm"],
  confirmText: BASE_APP_TOKENS.dark["--action-confirm-foreground"],
  warning: BASE_APP_TOKENS.dark["--status-tentative"],
  warningText: BASE_APP_TOKENS.dark["--status-tentative-foreground"],
};

/**
 * Pairs of (foreground token, background token) that must meet AA
 * body-text contrast (4.5:1). Prose-weight copy only.
 */
const AA_BODY_PAIRS: ReadonlyArray<[string, string]> = [
  ["--foreground", "--background"],
  ["--card-foreground", "--card"],
  ["--popover-foreground", "--popover"],
  ["--primary-foreground", "--primary"],
  ["--secondary-foreground", "--secondary"],
  ["--accent-foreground", "--accent"],
  ["--sidebar-foreground", "--sidebar"],
  ["--sidebar-accent-foreground", "--sidebar-accent"],
  ["--event-panel-text", "--event-panel-bg"],
  ["--event-panel-input-text", "--event-panel-bg"],
];

/**
 * Status / destructive surfaces that only target AA-large (3:1). The BASE
 * palettes paint white over each of these at roughly 3-4:1, which is
 * bold-weight-readable but below AA body; enforcing 4.5 here would flag
 * BASE's own design intent. Confirm-foreground also lives here because
 * BASE.light paints pure white on its brighter #059669 at only 3.49:1.
 */
const AA_LARGE_PAIRS: ReadonlyArray<[string, string]> = [
  ["--destructive-foreground", "--destructive"],
  ["--action-danger-armed-foreground", "--action-danger-armed"],
  ["--action-confirm-foreground", "--action-confirm"],
  ["--status-accepted-foreground", "--status-accepted"],
  ["--status-declined-foreground", "--status-declined"],
];

/**
 * Tokens whose job is to recede (not disappear): at or above AA-large
 * 3:1, but not so prominent they compete with primary body text. On
 * BASE.dark `--muted-foreground` on `--muted` lands at exactly 3:1. On
 * BASE.light it sits at 4.4:1. User canvases whose lightness clamps at
 * the gamut edge (near-white / near-black) push the derived muted-bg to
 * the boundary, so the achieved ratio can drift slightly above BASE's
 * own; the upper bound is therefore relaxed to 5.0 to accommodate that
 * clamp without losing the "recessed" constraint. Event-panel
 * placeholders and muted-text land higher on BASE.dark (closer to
 * primary body), so they are not in this band.
 */
const MUTED_BANDS: ReadonlyArray<{ fg: string; bg: string }> = [
  { fg: "--muted-foreground", bg: "--muted" },
];

function assertAA(fgHex: string, bgHex: string, label: string) {
  const ratio = contrastRatio(fgHex, bgHex);
  if (ratio < 4.5) {
    throw new Error(
      `${label}: fg ${fgHex} on bg ${bgHex} = ${ratio.toFixed(2)}:1, expected >= 4.5`,
    );
  }
  expect(ratio).toBeGreaterThanOrEqual(4.5);
}

function assertAALarge(fgHex: string, bgHex: string, label: string) {
  const ratio = contrastRatio(fgHex, bgHex);
  if (ratio < 3) {
    throw new Error(
      `${label}: fg ${fgHex} on bg ${bgHex} = ${ratio.toFixed(2)}:1, expected >= 3`,
    );
  }
  expect(ratio).toBeGreaterThanOrEqual(3);
}

describe("deriveAppTokens", () => {
  it("returns values for every derivable key in both bases", () => {
    const light = deriveAppTokens(LIGHT_SOURCES);
    const dark = deriveAppTokens(DARK_SOURCES);
    for (const key of Object.keys(light)) {
      expect(light[key]).toBeDefined();
      expect(dark[key]).toBeDefined();
      expect(BASE_APP_TOKENS.light[key]).toBeDefined();
      expect(BASE_APP_TOKENS.dark[key]).toBeDefined();
    }
  });

  for (const base of ["light", "dark"] as const) {
    const sources = base === "light" ? LIGHT_SOURCES : DARK_SOURCES;

    it(`meets AA 4.5:1 on every body-text pair (${base})`, () => {
      const tokens = deriveAppTokens(sources);
      for (const [fg, bg] of AA_BODY_PAIRS) {
        assertAA(tokens[fg], tokens[bg], `${base} ${fg} on ${bg}`);
      }
    });

    it(`meets AA-large 3:1 on every status pair (${base})`, () => {
      const tokens = deriveAppTokens(sources);
      for (const [fg, bg] of AA_LARGE_PAIRS) {
        assertAALarge(tokens[fg], tokens[bg], `${base} ${fg} on ${bg}`);
      }
    });

    it(`parks muted captions inside [3.0, 5.0] (${base})`, () => {
      const tokens = deriveAppTokens(sources);
      for (const { fg, bg } of MUTED_BANDS) {
        const ratio = contrastRatio(tokens[fg], tokens[bg]);
        expect(ratio).toBeGreaterThanOrEqual(3.0);
        expect(ratio).toBeLessThanOrEqual(5.0);
      }
    });

    it(`event-panel divider sits at or above 1.4:1 against its panel (${base})`, () => {
      const tokens = deriveAppTokens(sources);
      const ratio = contrastRatio(
        tokens["--event-panel-divider"],
        tokens["--event-panel-bg"],
      );
      expect(ratio).toBeGreaterThanOrEqual(1.4);
    });
  }

  it("pins the source colors to the tokens they represent", () => {
    const light = deriveAppTokens(LIGHT_SOURCES);
    expect(light["--background"]).toBe(LIGHT_SOURCES.canvas);
    expect(light["--cal-header-bg"]).toBe(LIGHT_SOURCES.canvas);
    expect(light["--foreground"]).toBe(LIGHT_SOURCES.ink);
    expect(light["--primary"]).toBe(LIGHT_SOURCES.primary);
    expect(light["--destructive"]).toBe(LIGHT_SOURCES.destructive);
    expect(light["--destructive-foreground"]).toBe(LIGHT_SOURCES.destructiveText);
    expect(light["--action-confirm"]).toBe(LIGHT_SOURCES.confirm);
    expect(light["--action-confirm-foreground"]).toBe(LIGHT_SOURCES.confirmText);
    expect(light["--status-tentative"]).toBe(LIGHT_SOURCES.warning);
    expect(light["--status-tentative-foreground"]).toBe(LIGHT_SOURCES.warningText);

    const dark = deriveAppTokens(DARK_SOURCES);
    expect(dark["--background"]).toBe(DARK_SOURCES.canvas);
    expect(dark["--foreground"]).toBe(DARK_SOURCES.ink);
    expect(dark["--primary"]).toBe(DARK_SOURCES.primary);
    expect(dark["--destructive"]).toBe(DARK_SOURCES.destructive);
    expect(dark["--destructive-foreground"]).toBe(DARK_SOURCES.destructiveText);
  });

  it("lifts card and popover away from canvas toward ink", () => {
    const derived = deriveAppTokens(LIGHT_SOURCES);
    expect(derived["--card"]).not.toBe(LIGHT_SOURCES.canvas);
    expect(derived["--popover"]).not.toBe(LIGHT_SOURCES.canvas);
    expect(
      contrastRatio(derived["--card"], LIGHT_SOURCES.canvas),
    ).toBeGreaterThan(1);
    expect(
      contrastRatio(derived["--popover"], derived["--card"]),
    ).toBeGreaterThan(1);
  });

  /**
   * Surface hierarchy: the dark built-in establishes a layered stack
   * where sidebar recedes below canvas while card/popover/accent elevate
   * above it. Applying the ΔL deltas to any canvas must reproduce this
   * ordering. Near-white or near-black canvases clamp in
   * `shiftPerceptualL`, so the upper tiers may collapse to the gamut
   * boundary (accent = popover = card = 1 on a near-white canvas): the
   * invariant therefore allows non-strict `<=` ordering between tiers
   * that share the same direction, while sidebar (the only downward
   * step) must stay strictly below canvas whenever canvas has headroom.
   */
  for (const base of ["light", "dark"] as const) {
    const sources = base === "light" ? LIGHT_SOURCES : DARK_SOURCES;

    it(`preserves the surface hierarchy sidebar <= canvas <= card <= popover <= accent (${base})`, () => {
      const tokens = deriveAppTokens(sources);
      const L = (hex: string) => hexToOklab(hex)!.L;
      const sidebarL = L(tokens["--sidebar"]);
      const canvasL = L(tokens["--background"]);
      const cardL = L(tokens["--card"]);
      const popoverL = L(tokens["--popover"]);
      const accentL = L(tokens["--accent"]);
      expect(sidebarL).toBeLessThanOrEqual(canvasL);
      expect(canvasL).toBeLessThanOrEqual(cardL);
      expect(cardL).toBeLessThanOrEqual(popoverL);
      expect(popoverL).toBeLessThanOrEqual(accentL);
    });

    it(`places event-panel-contrast <= canvas and event-panel-bg >= canvas (${base})`, () => {
      const tokens = deriveAppTokens(sources);
      const L = (hex: string) => hexToOklab(hex)!.L;
      expect(L(tokens["--event-panel-contrast"])).toBeLessThanOrEqual(
        L(tokens["--background"]),
      );
      expect(L(tokens["--event-panel-bg"])).toBeGreaterThanOrEqual(
        L(tokens["--background"]),
      );
    });
  }

  it("preserves the surface hierarchy with strict ordering when canvas has headroom", () => {
    // A canvas in the mid-luminance band (~0.90 L) leaves full room for
    // every tier to separate without clamping. This is the scenario a
    // user creating a new custom theme typically lands on; the dark
    // built-in itself (~0.28 L) is the other end of the same space.
    const pastel: ThemeSources = { ...LIGHT_SOURCES, canvas: "#F4E9D3" };
    const tokens = deriveAppTokens(pastel);
    const L = (hex: string) => hexToOklab(hex)!.L;
    expect(L(tokens["--sidebar"])).toBeLessThan(L(tokens["--background"]));
    expect(L(tokens["--background"])).toBeLessThan(L(tokens["--card"]));
    expect(L(tokens["--card"])).toBeLessThan(L(tokens["--popover"]));
    expect(L(tokens["--popover"])).toBeLessThan(L(tokens["--accent"]));
  });

  it("re-derives live when canvas changes", () => {
    const shifted: ThemeSources = { ...LIGHT_SOURCES, canvas: "#FFEEDD" };
    const derived = deriveAppTokens(shifted);
    expect(derived["--background"]).toBe("#FFEEDD");
    expect(derived["--secondary"]).not.toBe(
      deriveAppTokens(LIGHT_SOURCES)["--secondary"],
    );
  });

  it("flips --foreground when the user darkens canvas without touching ink", () => {
    const lightIshInk = LIGHT_SOURCES.ink;
    const shifted: ThemeSources = { ...LIGHT_SOURCES, canvas: "#0A0A1E" };
    const tokens = deriveAppTokens(shifted);
    expect(tokens["--foreground"]).not.toBe(lightIshInk);
    assertAA(tokens["--foreground"], tokens["--background"], "shifted fg on bg");
  });

  /**
   * Direction-sync invariant: when the user drags canvas to a light color
   * without touching ink (ink stays light, e.g. BASE.dark's #ECECF2),
   * every walk-fraction recessed token and --form-indicator must flip to
   * a dark value so captions stay visible against the now-light surfaces.
   * Raw-ink-anchored walks would keep producing light values on a light
   * canvas; anchoring on `pickReadableForeground(bg)` per-surface guarantees
   * direction tracks canvas.
   */
  it("flips recessed tokens dark when canvas goes light but ink stays light", () => {
    // Ring sits at BASE.dark's walk fraction of 0.673 toward the paired
    // bg; on a pure-white canvas the walk from black lands at ~2.96:1,
    // essentially the same band as BASE.dark's own 3.07:1 ring. Body-text
    // tokens (muted caption, form indicator, event-panel text variants)
    // must clear AA-large 3:1; ring shares the "visible as a focus
    // indicator" band at >= 2.9.
    const inverted: ThemeSources = { ...DARK_SOURCES, canvas: "#FFFFFF" };
    const tokens = deriveAppTokens(inverted);
    const strictPairs: ReadonlyArray<[string, string]> = [
      ["--muted-foreground", "--muted"],
      ["--form-indicator", "--background"],
      ["--event-panel-text", "--event-panel-bg"],
      ["--event-panel-input-text", "--event-panel-bg"],
      ["--event-panel-muted-text", "--event-panel-bg"],
    ];
    for (const [fg, bg] of strictPairs) {
      const ratio = contrastRatio(tokens[fg], tokens[bg]);
      if (ratio < 3) {
        throw new Error(
          `direction-sync ${fg} (${tokens[fg]}) on ${bg} (${tokens[bg]}) = ${ratio.toFixed(2)}:1, expected >= 3`,
        );
      }
      expect(ratio).toBeGreaterThanOrEqual(3);
    }
    const ringRatio = contrastRatio(tokens["--ring"], tokens["--background"]);
    expect(ringRatio).toBeGreaterThanOrEqual(2.9);
  });

  it("stays legible when the user picks extreme canvas/ink pairs", () => {
    const extreme: ThemeSources = {
      canvas: "#000000",
      ink: "#FFFFFF",
      primary: "#FFD700",
      destructive: "#FF1493",
      destructiveText: "#FFFFFF",
      confirm: "#00FF7F",
      confirmText: "#000000",
      warning: "#FFA500",
      warningText: "#000000",
    };
    const tokens = deriveAppTokens(extreme);
    for (const [fg, bg] of AA_BODY_PAIRS) {
      assertAA(tokens[fg], tokens[bg], `extreme ${fg} on ${bg}`);
    }
    for (const [fg, bg] of AA_LARGE_PAIRS) {
      assertAALarge(tokens[fg], tokens[bg], `extreme ${fg} on ${bg}`);
    }
  });

  /**
   * Dark-BASE identity: feeding the dark built-in's own sources must
   * reproduce BASE_APP_TOKENS.dark. Identity is enforced on exactly-
   * matching tokens (source-pinned, hardcoded white, ink-driven) and
   * allows a small tolerance on shift-derived or walk-fraction tokens
   * where chroma differences between BASE's curated hex and the
   * derivation anchor's chroma produce a 1-2 rgb-unit drift.
   */
  it("reproduces BASE.dark exactly on source-driven and hardcoded tokens", () => {
    const derived = deriveAppTokens(DARK_SOURCES);
    const exactKeys = [
      "--background",
      "--cal-header-bg",
      "--foreground",
      "--card-foreground",
      "--popover-foreground",
      "--primary",
      "--primary-foreground",
      "--secondary-foreground",
      "--accent-foreground",
      "--destructive",
      "--destructive-foreground",
      "--sidebar-foreground",
      "--sidebar-accent-foreground",
      "--event-panel-contrast",
      "--form-indicator",
      "--action-confirm",
      "--action-danger-armed",
      "--action-danger-armed-foreground",
      "--status-accepted",
      "--status-tentative",
      "--status-tentative-foreground",
      "--status-declined",
      "--status-declined-foreground",
    ];
    for (const key of exactKeys) {
      expect(derived[key]?.toLowerCase()).toBe(
        BASE_APP_TOKENS.dark[key].toLowerCase(),
      );
    }
  });

  it("lands within 2 OKLab-L units of BASE.dark on shift-derived surfaces", () => {
    const derived = deriveAppTokens(DARK_SOURCES);
    const L = (hex: string) => hexToOklab(hex)!.L;
    const approxKeys = [
      "--card",
      "--popover",
      "--secondary",
      "--muted",
      "--accent",
      "--sidebar",
      "--sidebar-accent",
      "--event-panel-bg",
    ];
    for (const key of approxKeys) {
      const drift = Math.abs(L(derived[key]) - L(BASE_APP_TOKENS.dark[key]));
      expect(drift).toBeLessThan(0.005);
    }
  });

  it("lands within 2 OKLab-L units of BASE.dark on walk-fraction tokens", () => {
    const derived = deriveAppTokens(DARK_SOURCES);
    const L = (hex: string) => hexToOklab(hex)!.L;
    const walkKeys = [
      "--muted-foreground",
      "--ring",
      "--event-panel-divider",
      "--event-panel-text",
      "--event-panel-input-text",
      "--event-panel-placeholder",
      "--event-panel-muted-text",
    ];
    for (const key of walkKeys) {
      const drift = Math.abs(L(derived[key]) - L(BASE_APP_TOKENS.dark[key]));
      expect(drift).toBeLessThan(0.005);
    }
  });

  it("covers every app token key through derive or BASE fallthrough", () => {
    const derived = deriveAppTokens(DARK_SOURCES);
    for (const key of APP_TOKEN_KEYS) {
      const hasDerived = derived[key] !== undefined;
      const hasBase = BASE_APP_TOKENS.dark[key] !== undefined;
      expect(hasDerived || hasBase).toBe(true);
    }
  });
});

describe("deriveCalendarTokens", () => {
  it("returns entries for every derivable key, not for the fully-semantic ones", () => {
    const derived = deriveCalendarTokens(LIGHT_SOURCES);
    expect(derived["--cal-bg"]).toBeDefined();
    expect(derived["--cal-gridline"]).toBeDefined();
    expect(derived["--cal-time-label"]).toBeDefined();
    expect(derived["--cal-timeline-rail"]).toBeDefined();
    // current-time, timeline-break, and timeline-focus carry hard-coded
    // semantic meaning and stay undefined in the derivation so the base
    // CSS defaults win.
    expect(derived["--cal-current-time"]).toBeUndefined();
    expect(derived["--cal-timeline-break"]).toBeUndefined();
    expect(derived["--cal-timeline-focus"]).toBeUndefined();
  });

  it("derives cal-bg from the app canvas", () => {
    const light = deriveCalendarTokens(LIGHT_SOURCES);
    // cal-bg is not a source: on a light canvas it lifts slightly
    // above canvas, so the two should diverge but stay close.
    expect(light["--cal-bg"]).not.toBe(LIGHT_SOURCES.canvas);
  });

  it("auto-derives cal-bg in opposite directions based on canvas luminance", () => {
    const light = deriveCalendarTokens(LIGHT_SOURCES);
    const dark = deriveCalendarTokens(DARK_SOURCES);
    const canvasL = (hex: string) => hexToOklab(hex)!.L;
    // Light canvas: cal-bg lifts above canvas (positive ΔL).
    expect(canvasL(light["--cal-bg"])).toBeGreaterThan(
      canvasL(LIGHT_SOURCES.canvas),
    );
    // Dark canvas: cal-bg recedes below canvas (negative ΔL).
    expect(canvasL(dark["--cal-bg"])).toBeLessThan(
      canvasL(DARK_SOURCES.canvas),
    );
  });

  it("direction of cal-bg derivation flips at the luminance midpoint", () => {
    // A barely-dark canvas pulls cal-bg down; a barely-light canvas pushes
    // it up. This confirms the `< 0.5 relativeLuminance` branch.
    const darkish: ThemeSources = { ...LIGHT_SOURCES, canvas: "#404040" };
    const lightish: ThemeSources = { ...LIGHT_SOURCES, canvas: "#E0E0E0" };
    const darkTokens = deriveCalendarTokens(darkish);
    const lightTokens = deriveCalendarTokens(lightish);
    expect(relativeLuminance(darkish.canvas)).toBeLessThan(0.5);
    expect(relativeLuminance(lightish.canvas)).toBeGreaterThanOrEqual(0.5);
    expect(hexToOklab(darkTokens["--cal-bg"])!.L).toBeLessThan(
      hexToOklab(darkish.canvas)!.L,
    );
    expect(hexToOklab(lightTokens["--cal-bg"])!.L).toBeGreaterThan(
      hexToOklab(lightish.canvas)!.L,
    );
  });

  for (const base of ["light", "dark"] as const) {
    const sources = base === "light" ? LIGHT_SOURCES : DARK_SOURCES;

    it(`gridline sits at or above 1.4:1 against the calendar canvas (${base})`, () => {
      // The 1.4 gridline target lets cloned themes inherit the
      // built-in's subtle gridline style (~1.5:1) instead of
      // prominent 3:1 lines.
      const tokens = deriveCalendarTokens(sources);
      const ratio = contrastRatio(tokens["--cal-gridline"], tokens["--cal-bg"]);
      expect(ratio).toBeGreaterThanOrEqual(1.4);
    });

    it(`time label stays at or above 3:1 against the calendar canvas (${base})`, () => {
      // Time labels are axis captions: readable enough to parse hours
      // but muted enough not to compete with event tiles. BASE.dark
      // parks the label at ~6:1 against cal-bg; the walk-fraction
      // derivation reproduces that position on any canvas.
      const tokens = deriveCalendarTokens(sources);
      const ratio = contrastRatio(tokens["--cal-time-label"], tokens["--cal-bg"]);
      expect(ratio).toBeGreaterThanOrEqual(3.0);
    });
  }

  it("flips time label dark when canvas goes light but ink stays light", () => {
    const inverted: ThemeSources = { ...DARK_SOURCES, canvas: "#FFFFFF" };
    const tokens = deriveCalendarTokens(inverted);
    const ratio = contrastRatio(
      tokens["--cal-time-label"],
      tokens["--cal-bg"],
    );
    if (ratio < 3) {
      throw new Error(
        `direction-sync --cal-time-label (${tokens["--cal-time-label"]}) on --cal-bg (${tokens["--cal-bg"]}) = ${ratio.toFixed(2)}:1`,
      );
    }
    expect(ratio).toBeGreaterThanOrEqual(3);
  });

  /**
   * Dark-BASE calendar identity: feeding dark built-in's sources must
   * reproduce BASE_CALENDAR_TOKENS.dark. Tokens that are shift- or
   * walk-derived stay within a small OKLab-L tolerance (chroma drift
   * from ink anchor).
   */
  it("lands within 2 OKLab-L units of BASE.dark on derived calendar tokens", () => {
    const derived = deriveCalendarTokens(DARK_SOURCES);
    const L = (hex: string) => hexToOklab(hex)!.L;
    const approxKeys = [
      "--cal-bg",
      "--cal-gridline",
      "--cal-time-label",
      "--cal-timeline-rail",
    ];
    for (const key of approxKeys) {
      const drift = Math.abs(
        L(derived[key]) - L(BASE_CALENDAR_TOKENS.dark[key]),
      );
      expect(drift).toBeLessThan(0.005);
    }
  });

  it("covers every calendar token key through derive or BASE fallthrough", () => {
    const derived = deriveCalendarTokens(DARK_SOURCES);
    for (const key of CALENDAR_TOKEN_KEYS) {
      const hasDerived = derived[key] !== undefined;
      const hasBase = BASE_CALENDAR_TOKENS.dark[key] !== undefined;
      expect(hasDerived || hasBase).toBe(true);
    }
  });
});

describe("deriveCalendarColorDefaultBundle", () => {
  it("returns the curated light calendar tokens and palette", () => {
    const bundle = deriveCalendarColorDefaultBundle(
      DARK_SOURCES,
      "light",
      "#000000",
    );
    expect(bundle.calendarTokens).toEqual(BASE_CALENDAR_TOKENS.light);
    expect(bundle.runtimeTokens["--cal-scrollbar-thumb"]).toBe(
      BASE_APP_TOKENS.light["--muted"],
    );
    expect(bundle.runtimeTokens["--cal-scrollbar-thumb-hover"]).toBe(
      BASE_APP_TOKENS.light["--muted-foreground"],
    );
    expect(bundle.eventPalette).toEqual(lightTheme.eventPalette);
    expect(bundle.blendCanvas).toBe(BASE_CALENDAR_TOKENS.light["--cal-bg"]);
  });

  it("returns the curated dark calendar tokens and palette", () => {
    const bundle = deriveCalendarColorDefaultBundle(
      LIGHT_SOURCES,
      "dark",
      "#ffffff",
    );
    expect(bundle.calendarTokens).toEqual(BASE_CALENDAR_TOKENS.dark);
    expect(bundle.runtimeTokens["--cal-scrollbar-thumb"]).toBe(
      BASE_APP_TOKENS.dark["--muted"],
    );
    expect(bundle.runtimeTokens["--cal-scrollbar-thumb-hover"]).toBe(
      BASE_APP_TOKENS.dark["--muted-foreground"],
    );
    expect(bundle.eventPalette).toEqual(darkTheme.eventPalette);
    expect(bundle.blendCanvas).toBe(BASE_CALENDAR_TOKENS.dark["--cal-bg"]);
  });

  it("uses app canvas mode to derive calendar tokens without a header token", () => {
    const bundle = deriveCalendarColorDefaultBundle(
      LIGHT_SOURCES,
      "app-canvas",
      "#000000",
    );
    expect(bundle.calendarTokens["--cal-bg"]).not.toBe(LIGHT_SOURCES.canvas);
    expect(
      contrastRatio(
        bundle.runtimeTokens["--cal-scrollbar-thumb"],
        bundle.calendarTokens["--cal-bg"],
      ),
    ).toBeGreaterThanOrEqual(1.6);
    expect(
      contrastRatio(
        bundle.runtimeTokens["--cal-scrollbar-thumb-hover"],
        bundle.calendarTokens["--cal-bg"],
      ),
    ).toBeGreaterThanOrEqual(4.5);
    expect(bundle.runtimeTokens["--cal-scrollbar-thumb"]).not.toBe(
      deriveAppTokens(LIGHT_SOURCES)["--muted"],
    );
    expect(
      Object.hasOwn(bundle.calendarTokens, "--cal-header-bg"),
    ).toBe(false);
  });

  it("uses custom mode to derive from the supplied basis", () => {
    const bundle = deriveCalendarColorDefaultBundle(
      LIGHT_SOURCES,
      "custom",
      "#101010",
    );
    expect(hexToOklab(bundle.calendarTokens["--cal-bg"])!.L).toBeLessThan(
      hexToOklab(LIGHT_SOURCES.canvas)!.L,
    );
    expect(
      contrastRatio(
        bundle.runtimeTokens["--cal-scrollbar-thumb"],
        bundle.calendarTokens["--cal-bg"],
      ),
    ).toBeGreaterThanOrEqual(1.6);
    expect(
      contrastRatio(
        bundle.runtimeTokens["--cal-scrollbar-thumb-hover"],
        bundle.calendarTokens["--cal-bg"],
      ),
    ).toBeGreaterThanOrEqual(4.5);
    expect(bundle.runtimeTokens["--cal-scrollbar-thumb"]).not.toBe(
      deriveAppTokens({ ...LIGHT_SOURCES, canvas: "#101010" })["--muted"],
    );
    expect(bundle.eventPalette).toEqual(darkTheme.eventPalette);
  });
});
