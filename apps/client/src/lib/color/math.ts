/**
 * Pure color math helpers used by the in-house color picker.
 *
 * The picker stores state as HSV internally because the SV square plus hue
 * slider maps directly onto those three components. RGB and hex are derived
 * for display and persistence. Conversions are lossless within rounding
 * tolerance: hex round-trips through HSV with at most a one-count delta on
 * each channel due to integer rounding.
 */

export interface RgbColor {
  r: number;
  g: number;
  b: number;
}

/** RGB with an alpha channel in 0..255 (255 = fully opaque). */
export interface RgbaColor extends RgbColor {
  a: number;
}

export interface HsvColor {
  h: number;
  s: number;
  v: number;
}

const HEX8_RE = /^#?([0-9a-fA-F]{8})$/;
const HEX6_RE = /^#?([0-9a-fA-F]{6})$/;
const HEX4_RE = /^#?([0-9a-fA-F]{4})$/;
const HEX3_RE = /^#?([0-9a-fA-F]{3})$/;

/** Clamp a channel to 0..255 and round to the nearest integer. */
export function clampChannel(value: number): number {
  if (Number.isNaN(value)) return 0;
  if (value <= 0) return 0;
  if (value >= 255) return 255;
  return Math.round(value);
}

/** Clamp hue to 0..360 (wrapping around). */
export function clampHue(value: number): number {
  if (!Number.isFinite(value)) return 0;
  let h = value % 360;
  if (h < 0) h += 360;
  return h;
}

/** Clamp saturation/value to 0..100. */
export function clampPercent(value: number): number {
  if (Number.isNaN(value)) return 0;
  if (value <= 0) return 0;
  if (value >= 100) return 100;
  return value;
}

/**
 * Normalize a hex input. Accepts 3, 4, 6, and 8 digit hex with or without a
 * leading #. Returns "#rrggbb" when fully opaque, "#rrggbbaa" otherwise.
 * Returns null when the input cannot be parsed.
 */
export function normalizeHex(input: string): string | null {
  if (typeof input !== "string") return null;
  const trimmed = input.trim();
  const match8 = HEX8_RE.exec(trimmed);
  if (match8) {
    const lower = match8[1].toLowerCase();
    const alpha = lower.slice(6, 8);
    return alpha === "ff" ? `#${lower.slice(0, 6)}` : `#${lower}`;
  }
  const match6 = HEX6_RE.exec(trimmed);
  if (match6) return `#${match6[1].toLowerCase()}`;
  const match4 = HEX4_RE.exec(trimmed);
  if (match4) {
    const [r, g, b, a] = match4[1].toLowerCase();
    if (a === "f") return `#${r}${r}${g}${g}${b}${b}`;
    return `#${r}${r}${g}${g}${b}${b}${a}${a}`;
  }
  const match3 = HEX3_RE.exec(trimmed);
  if (match3) {
    const [r, g, b] = match3[1].toLowerCase();
    return `#${r}${r}${g}${g}${b}${b}`;
  }
  return null;
}

/** Convert a hex string to RGB components (alpha, if any, is discarded). */
export function hexToRgb(input: string): RgbColor | null {
  const rgba = hexToRgba(input);
  if (!rgba) return null;
  return { r: rgba.r, g: rgba.g, b: rgba.b };
}

/**
 * Convert a hex string to RGBA components. Inputs without alpha default to
 * a = 255 (fully opaque).
 */
export function hexToRgba(input: string): RgbaColor | null {
  const normalized = normalizeHex(input);
  if (!normalized) return null;
  const r = parseInt(normalized.slice(1, 3), 16);
  const g = parseInt(normalized.slice(3, 5), 16);
  const b = parseInt(normalized.slice(5, 7), 16);
  const a = normalized.length === 9 ? parseInt(normalized.slice(7, 9), 16) : 255;
  return { r, g, b, a };
}

/** Convert RGB components to a 6-digit lowercase hex string. */
export function rgbToHex(r: number, g: number, b: number): string {
  const cr = clampChannel(r).toString(16).padStart(2, "0");
  const cg = clampChannel(g).toString(16).padStart(2, "0");
  const cb = clampChannel(b).toString(16).padStart(2, "0");
  return `#${cr}${cg}${cb}`;
}

/**
 * Convert RGBA components to a hex string. Emits 6 digits when a = 255
 * (fully opaque), 8 digits otherwise. Keeps fully-opaque colors in their
 * canonical short form.
 */
export function rgbaToHex(r: number, g: number, b: number, a: number): string {
  const cr = clampChannel(r).toString(16).padStart(2, "0");
  const cg = clampChannel(g).toString(16).padStart(2, "0");
  const cb = clampChannel(b).toString(16).padStart(2, "0");
  const ca = clampChannel(a);
  if (ca === 255) return `#${cr}${cg}${cb}`;
  const caHex = ca.toString(16).padStart(2, "0");
  return `#${cr}${cg}${cb}${caHex}`;
}

/** Convert RGB (0..255) to HSV (h: 0..360, s/v: 0..100). */
export function rgbToHsv(r: number, g: number, b: number): HsvColor {
  const rN = clampChannel(r) / 255;
  const gN = clampChannel(g) / 255;
  const bN = clampChannel(b) / 255;
  const max = Math.max(rN, gN, bN);
  const min = Math.min(rN, gN, bN);
  const delta = max - min;
  let h = 0;
  if (delta !== 0) {
    if (max === rN) h = ((gN - bN) / delta) % 6;
    else if (max === gN) h = (bN - rN) / delta + 2;
    else h = (rN - gN) / delta + 4;
    h *= 60;
    if (h < 0) h += 360;
  }
  const s = max === 0 ? 0 : (delta / max) * 100;
  const v = max * 100;
  return { h, s, v };
}

/** Convert HSV (h: 0..360, s/v: 0..100) to RGB (0..255). */
export function hsvToRgb(h: number, s: number, v: number): RgbColor {
  const hue = clampHue(h);
  const sat = clampPercent(s) / 100;
  const val = clampPercent(v) / 100;
  const c = val * sat;
  const hPrime = hue / 60;
  const x = c * (1 - Math.abs((hPrime % 2) - 1));
  let r1 = 0;
  let g1 = 0;
  let b1 = 0;
  if (hPrime < 1) {
    r1 = c;
    g1 = x;
  } else if (hPrime < 2) {
    r1 = x;
    g1 = c;
  } else if (hPrime < 3) {
    g1 = c;
    b1 = x;
  } else if (hPrime < 4) {
    g1 = x;
    b1 = c;
  } else if (hPrime < 5) {
    r1 = x;
    b1 = c;
  } else {
    r1 = c;
    b1 = x;
  }
  const m = val - c;
  return {
    r: clampChannel((r1 + m) * 255),
    g: clampChannel((g1 + m) * 255),
    b: clampChannel((b1 + m) * 255),
  };
}

/** Convert HSV directly to a hex color string. */
export function hsvToHex(h: number, s: number, v: number): string {
  const { r, g, b } = hsvToRgb(h, s, v);
  return rgbToHex(r, g, b);
}

/**
 * Linearly blend two hex colors in sRGB space. The result keeps the alpha
 * channel from `a`, because callers use this to tint a source color toward
 * a canvas without changing that source color's opacity. `weightA` is the
 * fraction of `a` in the result (0..1), the rest is `b`. Invalid inputs
 * fall back to `a`.
 *
 * Note: sRGB-space blending is not perceptually uniform, but the event
 * palette dimming weights in `calendar/utils.ts` are calibrated against it.
 * For perceptual lightness walks use the OKLab pickers below instead.
 */
export function blendHex(a: string, b: string, weightA: number): string {
  const ra = hexToRgba(a);
  const rb = hexToRgba(b);
  if (!ra || !rb) return a;
  const wb = 1 - weightA;
  return rgbaToHex(
    ra.r * weightA + rb.r * wb,
    ra.g * weightA + rb.g * wb,
    ra.b * weightA + rb.b * wb,
    ra.a,
  );
}

/**
 * Gamma-decode an sRGB channel (0..1) to linear-light (0..1). Uses the
 * 0.04045 threshold from the sRGB standard (IEC 61966-2-1); WCAG 2.1 cites
 * 0.03928, but the two thresholds give indistinguishable contrast results
 * for 8-bit channels.
 */
function sRgbToLinear(c: number): number {
  return c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);
}

/** Gamma-encode a linear-light channel (0..1) back to sRGB (0..1). */
function linearToSRgb(c: number): number {
  return c <= 0.0031308 ? 12.92 * c : 1.055 * Math.pow(c, 1 / 2.4) - 0.055;
}

/**
 * WCAG 2.1 relative luminance for a hex color. Returns a value in 0..1
 * (0 = pure black, 1 = pure white). Gamma-decodes sRGB before applying
 * the Rec. 709 luminance coefficients, which is the WCAG-compliant
 * formula, as opposed to the raw-sRGB approximation in
 * `pickContrastText` in `calendar/utils.ts`.
 */
export function relativeLuminance(hex: string): number {
  const rgb = hexToRgb(hex);
  if (!rgb) return 0;
  const rL = sRgbToLinear(rgb.r / 255);
  const gL = sRgbToLinear(rgb.g / 255);
  const bL = sRgbToLinear(rgb.b / 255);
  return 0.2126 * rL + 0.7152 * gL + 0.0722 * bL;
}

/**
 * WCAG contrast ratio between two hex colors: `(Lmax + 0.05) / (Lmin +
 * 0.05)`. Result is in 1..21 (1 = identical, 21 = pure black vs pure
 * white). Invalid inputs collapse to `1` (via the luminance-0 fallback).
 */
export function contrastRatio(a: string, b: string): number {
  const la = relativeLuminance(a);
  const lb = relativeLuminance(b);
  const lmax = Math.max(la, lb);
  const lmin = Math.min(la, lb);
  return (lmax + 0.05) / (lmin + 0.05);
}

/**
 * Perceptually uniform OKLab color. `L` is lightness in 0..1; `a` and
 * `b` are chroma components that typically fall in roughly -0.4..0.4
 * for realistic sRGB colors (Björn Ottosson,
 * bottosson.github.io/posts/oklab).
 */
export interface OklabColor {
  L: number;
  a: number;
  b: number;
}

/**
 * Convert sRGB (0..255) to OKLab. Walking lightness in OKLab produces
 * perceptually uniform steps instead of the chunky, hue-shifting walks
 * a raw-sRGB or HSV walk would produce, which is what the contrast
 * pickers below rely on.
 */
export function rgbToOklab(r: number, g: number, b: number): OklabColor {
  const rL = sRgbToLinear(clampChannel(r) / 255);
  const gL = sRgbToLinear(clampChannel(g) / 255);
  const bL = sRgbToLinear(clampChannel(b) / 255);
  const l = 0.4122214708 * rL + 0.5363325363 * gL + 0.0514459929 * bL;
  const m = 0.2119034982 * rL + 0.6806995451 * gL + 0.1073969566 * bL;
  const s = 0.0883024619 * rL + 0.2817188376 * gL + 0.6299787005 * bL;
  const lPrime = Math.cbrt(l);
  const mPrime = Math.cbrt(m);
  const sPrime = Math.cbrt(s);
  return {
    L: 0.2104542553 * lPrime + 0.793617785 * mPrime - 0.0040720468 * sPrime,
    a: 1.9779984951 * lPrime - 2.428592205 * mPrime + 0.4505937099 * sPrime,
    b: 0.0259040371 * lPrime + 0.7827717662 * mPrime - 0.808675766 * sPrime,
  };
}

/**
 * Convert OKLab back to sRGB (0..255, clamped). Out-of-gamut OKLab
 * points are clipped at the sRGB boundary rather than gamut-mapped:
 * every walk in this module starts from an in-gamut color and nudges
 * only lightness, so modest clipping at the edges is tolerable.
 */
export function oklabToRgb(L: number, a: number, b: number): RgbColor {
  const lPrime = L + 0.3963377774 * a + 0.2158037573 * b;
  const mPrime = L - 0.1055613458 * a - 0.0638541728 * b;
  const sPrime = L - 0.0894841775 * a - 1.291485548 * b;
  const l = lPrime * lPrime * lPrime;
  const m = mPrime * mPrime * mPrime;
  const s = sPrime * sPrime * sPrime;
  const rL = 4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s;
  const gL = -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s;
  const bL = -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s;
  return {
    r: clampChannel(linearToSRgb(Math.max(0, Math.min(1, rL))) * 255),
    g: clampChannel(linearToSRgb(Math.max(0, Math.min(1, gL))) * 255),
    b: clampChannel(linearToSRgb(Math.max(0, Math.min(1, bL))) * 255),
  };
}

/** Convert a hex color directly to OKLab. Returns null on bad input. */
export function hexToOklab(hex: string): OklabColor | null {
  const rgb = hexToRgb(hex);
  if (!rgb) return null;
  return rgbToOklab(rgb.r, rgb.g, rgb.b);
}

/** Convert OKLab directly to a 6-digit hex color (clipped to sRGB gamut). */
export function oklabToHex(L: number, a: number, b: number): string {
  const rgb = oklabToRgb(L, a, b);
  return rgbToHex(rgb.r, rgb.g, rgb.b);
}

/**
 * Shift a hex color's OKLab lightness by `deltaL` (absolute, not relative).
 * Positive values brighten, negative values darken, the color's hue and
 * chroma are preserved. `L` is clamped to `[0, 1]`, so near-white bgs
 * shifted positive or near-black bgs shifted negative stay in gamut
 * instead of saturating at the endpoint. Invalid input falls back to the
 * original hex so callers never receive `null`.
 *
 * Used by `deriveAppTokens` to walk canvas by per-token signed offsets
 * calibrated from the dark built-in, producing the same surface hierarchy
 * (card above canvas, sidebar below) on any custom canvas.
 */
export function shiftPerceptualL(hex: string, deltaL: number): string {
  const lab = hexToOklab(hex);
  if (!lab) return hex;
  const nextL = Math.max(0, Math.min(1, lab.L + deltaL));
  return oklabToHex(nextL, lab.a, lab.b);
}

/**
 * Blend two colors in OKLab by `fraction` (0 = return `fg`, 1 = return
 * `bg`'s lightness on `fg`'s hue line). Blends lightness fully; chroma
 * (`a`, `b`) stays anchored on `fg` so the result keeps `fg`'s hue even
 * when bg has a different tint. Useful for deriving "muted caption",
 * "recessed label", and similar tokens whose BASE hex sits at a curated
 * fraction of the way from a foreground anchor toward a surface, because
 * contrast-target walks cannot express "park at the curated recession"
 * for values whose contrast sits between 3:1 and 4.5:1 without sweeping
 * the entire gamut.
 *
 * Invalid input falls back to `fg` so callers never receive `null`.
 */
export function walkFraction(fg: string, bg: string, fraction: number): string {
  const fgLab = hexToOklab(fg);
  const bgLab = hexToOklab(bg);
  if (!fgLab || !bgLab) return fg;
  const L = fgLab.L + fraction * (bgLab.L - fgLab.L);
  return oklabToHex(Math.max(0, Math.min(1, L)), fgLab.a, fgLab.b);
}

/**
 * Pick a "bright signal" foreground that prefers pure `#FFFFFF` when it
 * meets `target` contrast against `bg`, falling back to `ink`, then
 * pure `#000000`, then `pickReadableForeground` as a last-ditch walk.
 *
 * Used for status-colored surfaces (destructive, confirm, declined,
 * accepted) and sidebar foregrounds where the design convention is "white
 * unless the bg is too bright for white to work". `pickReadableForeground`
 * prefers the ink anchor first, which returns near-white on dark surfaces
 * (matching BASE), but `pickBrightForeground` snaps decisively to pure
 * white on mid-dark bgs that would otherwise land on ink's off-white
 * #ECECF2 when the design calls for #FFFFFF.
 */
export function pickBrightForeground(
  bg: string,
  ink: string,
  target = 4.5,
): string {
  if (contrastRatio(bg, "#FFFFFF") >= target) return "#FFFFFF";
  if (contrastRatio(bg, ink) >= target) return ink;
  if (contrastRatio(bg, "#000000") >= target) return "#000000";
  return pickReadableForeground(bg, { ink, canvas: ink, target });
}

/**
 * Returns whichever of `#000000` / `#ffffff` has higher contrast against
 * `bg`. For mid-luminance bgs (around #808080) this is counterintuitive:
 * black actually beats white because luminance is nonlinear. Using this
 * as the saturating fallback avoids the classic "light bg gets black text,
 * dark bg gets white text" heuristic, which is wrong at the edges.
 */
function bestEndpoint(bg: string): string {
  return contrastRatio(bg, "#000000") >= contrastRatio(bg, "#ffffff")
    ? "#000000"
    : "#ffffff";
}

/**
 * Walk OKLab lightness from `anchor`'s L in `direction`, preserving the
 * anchor's a/b chroma, until contrast against `bg` hits `target`. Returns
 * the first candidate to meet the target, or null when the walk saturates
 * (L leaves 0..1) without reaching it.
 */
function stepWalk(
  bg: string,
  anchorLab: OklabColor,
  direction: 1 | -1,
  target: number,
): string | null {
  const step = 0.02;
  let L = anchorLab.L;
  for (let i = 0; i < 60; i++) {
    const candidate = oklabToHex(L, anchorLab.a, anchorLab.b);
    if (contrastRatio(candidate, bg) >= target) return candidate;
    L += direction * step;
    if (L <= 0 || L >= 1) break;
  }
  return null;
}

/**
 * Walk `anchor`'s OKLab lightness until the candidate meets `target`
 * contrast against `bg`. Tries the direction away from `bg`'s L first
 * (the intuitive one), then the other direction; some low-chroma anchors
 * reach target faster in the opposite direction. If both walks saturate,
 * falls back to the best of `#000000` / `#ffffff`. Warm foregrounds stay
 * warm and cool ones stay cool: only lightness is adjusted.
 */
function walkToContrast(bg: string, anchor: string, target: number): string {
  const bgLab = hexToOklab(bg);
  const anchorLab = hexToOklab(anchor);
  if (!bgLab || !anchorLab) return bestEndpoint(bg);
  const preferred: 1 | -1 = anchorLab.L >= bgLab.L ? 1 : -1;
  const primary = stepWalk(bg, anchorLab, preferred, target);
  if (primary) return primary;
  const opposite: 1 | -1 = preferred === 1 ? -1 : 1;
  const secondary = stepWalk(bg, anchorLab, opposite, target);
  if (secondary) return secondary;
  return bestEndpoint(bg);
}

/**
 * Pick a foreground color that meets `target` contrast (default 4.5 =
 * WCAG AA body text) against `bg`, preferring the theme's `ink` and
 * `canvas` anchors first. When neither anchor meets the target, prefer
 * the saturating endpoint (`#000000` or `#ffffff`) if it meets target:
 * a chroma-preserving OKLab walk from the closer anchor would land at
 * a just-enough value still close to `bg` in luminance, which looks
 * muddy even though it technically meets target. The endpoint snaps
 * decisively ("switch to dark on a light surface, switch to light on a
 * dark surface"). Only when the endpoint also fails does the walk run,
 * preserving hue intent for gamut-limited bgs.
 */
export function pickReadableForeground(
  bg: string,
  opts: { ink: string; canvas: string; target?: number },
): string {
  const target = opts.target ?? 4.5;
  const inkRatio = contrastRatio(bg, opts.ink);
  if (inkRatio >= target) return opts.ink;
  const canvasRatio = contrastRatio(bg, opts.canvas);
  if (canvasRatio >= target) return opts.canvas;
  const endpoint = bestEndpoint(bg);
  if (contrastRatio(bg, endpoint) >= target) return endpoint;
  const anchor = inkRatio >= canvasRatio ? opts.ink : opts.canvas;
  return walkToContrast(bg, anchor, target);
}

/**
 * Pick a border color for `bg` that meets `target` contrast (default
 * 3 = WCAG AA non-text). Walks from `bg` toward `ink` in OKLab,
 * preserving ink's chroma. If ink itself is too close to bg to carry
 * a readable border, walks toward the opposite-luminance anchor
 * (#000 on light bgs, #fff on dark).
 */
export function pickReadableBorder(
  bg: string,
  ink: string,
  opts: { target?: number } = {},
): string {
  const target = opts.target ?? 3;
  const bgLab = hexToOklab(bg);
  const inkLab = hexToOklab(ink);
  if (!bgLab || !inkLab) return ink;
  let anchorLab = inkLab;
  if (contrastRatio(bg, ink) < target) {
    const fallback = bestEndpoint(bg);
    const fallbackLab = hexToOklab(fallback);
    if (fallbackLab) anchorLab = fallbackLab;
  }
  const direction = anchorLab.L >= bgLab.L ? 1 : -1;
  const step = 0.02;
  let L = bgLab.L + direction * step;
  for (let i = 0; i < 60; i++) {
    if (L <= 0 || L >= 1) break;
    const candidate = oklabToHex(L, anchorLab.a, anchorLab.b);
    if (contrastRatio(bg, candidate) >= target) return candidate;
    L += direction * step;
  }
  return bestEndpoint(bg);
}

/**
 * Pick a muted foreground for `bg` that parks at the deepest recession
 * still meeting `target` contrast (default 3 = WCAG AA large text).
 * Walks from the best available anchor toward `bg` in OKLab and stops
 * just before contrast drops below the target. Useful for secondary
 * captions that should recede but stay readable.
 *
 * When `ink` already clears the target, ink is the anchor and the walk
 * recedes ink toward bg. When ink does not clear the target (e.g., a
 * dark canvas paired with a dark ink), the anchor switches to the
 * higher-contrast sRGB endpoint (`#ffffff` or `#000000`) so the result
 * still lands readable instead of returning ink unchanged.
 */
export function pickReadableMuted(
  bg: string,
  ink: string,
  opts: { target?: number } = {},
): string {
  const target = opts.target ?? 3;
  const bgLab = hexToOklab(bg);
  if (!bgLab) return ink;
  let anchor = ink;
  let anchorLab = hexToOklab(ink);
  if (!anchorLab || contrastRatio(bg, ink) < target) {
    anchor = bestEndpoint(bg);
    anchorLab = hexToOklab(anchor);
    if (!anchorLab) return anchor;
  }
  const direction = bgLab.L >= anchorLab.L ? 1 : -1;
  const step = 0.01;
  let L = anchorLab.L;
  let lastGood = anchor;
  for (let i = 0; i < 200; i++) {
    const nextL = L + direction * step;
    if (nextL <= 0 || nextL >= 1) break;
    if (direction === 1 ? nextL >= bgLab.L : nextL <= bgLab.L) break;
    const candidate = oklabToHex(nextL, anchorLab.a, anchorLab.b);
    if (contrastRatio(bg, candidate) < target) break;
    lastGood = candidate;
    L = nextL;
  }
  return lastGood;
}

/**
 * Pick a legible shade for a calendar day number given its cell state.
 * Returns `ink` for active cells (in-month, not past), a muted AA-large
 * shade (~3:1) for inactive-in-month cells, and an intentionally
 * sub-AA shade (~2:1) for off-month cells so they recede visually
 * without disappearing. The 2:1 floor guards against plausible themes
 * where a lower target would collapse the digit into the background.
 */
export function dayNumberShade(
  bg: string,
  ink: string,
  isInMonth: boolean,
  isPast: boolean,
): string {
  if (isInMonth && !isPast) return ink;
  if (isInMonth) return pickReadableMuted(bg, ink, { target: 3 });
  return pickReadableMuted(bg, ink, { target: 2 });
}
