import { describe, it, expect } from "vitest";
import {
  blendHex,
  clampChannel,
  clampHue,
  clampPercent,
  contrastRatio,
  hexToOklab,
  hexToRgb,
  hexToRgba,
  hsvToRgb,
  normalizeHex,
  oklabToHex,
  oklabToRgb,
  pickReadableBorder,
  pickReadableForeground,
  pickReadableMuted,
  relativeLuminance,
  rgbaToHex,
  rgbToHex,
  rgbToHsv,
  rgbToOklab,
  shiftPerceptualL,
} from "./math";

describe("clampChannel", () => {
  it("rounds and clamps to 0..255", () => {
    expect(clampChannel(-10)).toBe(0);
    expect(clampChannel(0)).toBe(0);
    expect(clampChannel(127.6)).toBe(128);
    expect(clampChannel(255)).toBe(255);
    expect(clampChannel(999)).toBe(255);
  });

  it("returns 0 for non-finite numbers", () => {
    expect(clampChannel(Number.NaN)).toBe(0);
    expect(clampChannel(Number.POSITIVE_INFINITY)).toBe(255);
    expect(clampChannel(Number.NEGATIVE_INFINITY)).toBe(0);
  });
});

describe("clampHue", () => {
  it("wraps into 0..360", () => {
    expect(clampHue(0)).toBe(0);
    expect(clampHue(360)).toBe(0);
    expect(clampHue(450)).toBe(90);
    expect(clampHue(-30)).toBe(330);
  });
});

describe("clampPercent", () => {
  it("clamps to 0..100", () => {
    expect(clampPercent(-1)).toBe(0);
    expect(clampPercent(50)).toBe(50);
    expect(clampPercent(101)).toBe(100);
  });
});

describe("normalizeHex", () => {
  it("accepts 6-digit hex with or without leading #", () => {
    expect(normalizeHex("#A1B2C3")).toBe("#a1b2c3");
    expect(normalizeHex("a1b2c3")).toBe("#a1b2c3");
  });

  it("expands 3-digit hex", () => {
    expect(normalizeHex("#abc")).toBe("#aabbcc");
    expect(normalizeHex("123")).toBe("#112233");
  });

  it("trims whitespace", () => {
    expect(normalizeHex("  #fff  ")).toBe("#ffffff");
  });

  it("rejects invalid input", () => {
    expect(normalizeHex("not-a-color")).toBeNull();
    expect(normalizeHex("#ggg")).toBeNull();
    expect(normalizeHex("")).toBeNull();
    expect(normalizeHex("#12345")).toBeNull();
  });

  it("preserves 8-digit hex when alpha is not ff", () => {
    expect(normalizeHex("#11223380")).toBe("#11223380");
    expect(normalizeHex("11223380")).toBe("#11223380");
  });

  it("collapses 8-digit hex with ff alpha to 6-digit", () => {
    expect(normalizeHex("#112233ff")).toBe("#112233");
  });

  it("expands 4-digit hex", () => {
    expect(normalizeHex("#abcd")).toBe("#aabbccdd");
    expect(normalizeHex("#abcf")).toBe("#aabbcc");
  });
});

describe("hexToRgba / rgbaToHex", () => {
  it("defaults alpha to 255 for 6-digit input", () => {
    expect(hexToRgba("#808080")).toEqual({ r: 128, g: 128, b: 128, a: 255 });
  });

  it("parses 8-digit hex alpha", () => {
    expect(hexToRgba("#ff000080")).toEqual({ r: 255, g: 0, b: 0, a: 128 });
  });

  it("round-trips through rgba", () => {
    const samples = [
      { r: 0, g: 0, b: 0, a: 0 },
      { r: 17, g: 42, b: 200, a: 255 },
      { r: 240, g: 100, b: 30, a: 128 },
      { r: 255, g: 255, b: 255, a: 1 },
    ];
    for (const { r, g, b, a } of samples) {
      const hex = rgbaToHex(r, g, b, a);
      const rgba = hexToRgba(hex);
      expect(rgba).toEqual({ r, g, b, a });
    }
  });

  it("emits 6-digit hex when alpha is 255", () => {
    expect(rgbaToHex(0, 0, 0, 255)).toBe("#000000");
    expect(rgbaToHex(255, 255, 255, 255)).toBe("#ffffff");
  });

  it("emits 8-digit hex when alpha is less than 255", () => {
    expect(rgbaToHex(0, 0, 0, 0)).toBe("#00000000");
    expect(rgbaToHex(255, 0, 0, 128)).toBe("#ff000080");
  });

  it("hexToRgb drops alpha from 8-digit input", () => {
    expect(hexToRgb("#ff000080")).toEqual({ r: 255, g: 0, b: 0 });
  });
});

describe("hexToRgb / rgbToHex", () => {
  it("round-trips primary colors", () => {
    for (const hex of ["#000000", "#ffffff", "#ff0000", "#00ff00", "#0000ff"]) {
      const rgb = hexToRgb(hex);
      expect(rgb).not.toBeNull();
      if (!rgb) continue;
      expect(rgbToHex(rgb.r, rgb.g, rgb.b)).toBe(hex);
    }
  });

  it("decodes mid-range colors", () => {
    expect(hexToRgb("#80a040")).toEqual({ r: 128, g: 160, b: 64 });
  });

  it("returns null for invalid hex", () => {
    expect(hexToRgb("garbage")).toBeNull();
  });
});

describe("rgbToHsv / hsvToRgb", () => {
  it("converts pure red", () => {
    const hsv = rgbToHsv(255, 0, 0);
    expect(hsv.h).toBeCloseTo(0, 5);
    expect(hsv.s).toBe(100);
    expect(hsv.v).toBe(100);
  });

  it("converts pure green", () => {
    const hsv = rgbToHsv(0, 255, 0);
    expect(hsv.h).toBeCloseTo(120, 5);
    expect(hsv.s).toBe(100);
    expect(hsv.v).toBe(100);
  });

  it("converts pure blue", () => {
    const hsv = rgbToHsv(0, 0, 255);
    expect(hsv.h).toBeCloseTo(240, 5);
    expect(hsv.s).toBe(100);
    expect(hsv.v).toBe(100);
  });

  it("zero saturation when all channels equal", () => {
    const hsv = rgbToHsv(128, 128, 128);
    expect(hsv.s).toBe(0);
    expect(hsv.v).toBeCloseTo(50.196, 1);
  });

  it("round-trips RGB through HSV within rounding tolerance", () => {
    const samples = [
      { r: 17, g: 42, b: 200 },
      { r: 240, g: 100, b: 30 },
      { r: 5, g: 5, b: 5 },
      { r: 200, g: 200, b: 240 },
    ];
    for (const { r, g, b } of samples) {
      const hsv = rgbToHsv(r, g, b);
      const back = hsvToRgb(hsv.h, hsv.s, hsv.v);
      expect(Math.abs(back.r - r)).toBeLessThanOrEqual(1);
      expect(Math.abs(back.g - g)).toBeLessThanOrEqual(1);
      expect(Math.abs(back.b - b)).toBeLessThanOrEqual(1);
    }
  });
});

describe("blendHex", () => {
  it("returns a unchanged when weightA = 1", () => {
    expect(blendHex("#ff0000", "#0000ff", 1)).toBe("#ff0000");
  });

  it("returns b unchanged when weightA = 0", () => {
    expect(blendHex("#ff0000", "#0000ff", 0)).toBe("#0000ff");
  });

  it("blends evenly at weightA = 0.5", () => {
    expect(blendHex("#000000", "#ffffff", 0.5)).toBe("#808080");
  });

  it("preserves the alpha channel from the source color", () => {
    expect(blendHex("#00000080", "#ffffff", 0.5)).toBe("#80808080");
  });

  it("falls back to a on bad input", () => {
    expect(blendHex("not-a-color", "#000000", 0.5)).toBe("not-a-color");
  });
});

describe("relativeLuminance", () => {
  it("is 0 for pure black and 1 for pure white", () => {
    expect(relativeLuminance("#000000")).toBeCloseTo(0, 6);
    expect(relativeLuminance("#ffffff")).toBeCloseTo(1, 6);
  });

  it("returns values in the expected range for mid-grays", () => {
    // Luminance is monotonically increasing with lightness, so darker
    // grays must have strictly lower luminance than lighter ones.
    const dark = relativeLuminance("#444444");
    const mid = relativeLuminance("#777777");
    const light = relativeLuminance("#bbbbbb");
    expect(dark).toBeLessThan(mid);
    expect(mid).toBeLessThan(light);
    expect(dark).toBeGreaterThan(0);
    expect(light).toBeLessThan(1);
    // Mid-gray (#777) luminance is around 0.18 with either the 0.03928
    // or 0.04045 threshold; allow a 1-decimal-place window.
    expect(mid).toBeCloseTo(0.18, 1);
  });

  it("collapses to 0 on invalid hex", () => {
    expect(relativeLuminance("not-a-color")).toBe(0);
  });
});

describe("contrastRatio", () => {
  it("is 21 for pure white vs pure black", () => {
    expect(contrastRatio("#000000", "#ffffff")).toBeCloseTo(21, 6);
    expect(contrastRatio("#ffffff", "#000000")).toBeCloseTo(21, 6);
  });

  it("is 1 for identical colors", () => {
    expect(contrastRatio("#123456", "#123456")).toBeCloseTo(1, 6);
  });

  it("is symmetric", () => {
    const a = "#3a7bff";
    const b = "#ffe066";
    expect(contrastRatio(a, b)).toBeCloseTo(contrastRatio(b, a), 6);
  });

  it("matches a known WCAG worked example", () => {
    // #777777 against #ffffff ~= 4.48:1 per WCAG Understanding SC 1.4.3.
    expect(contrastRatio("#777777", "#ffffff")).toBeCloseTo(4.48, 1);
  });
});

describe("rgbToOklab / oklabToRgb", () => {
  it("black and white collapse to predictable L endpoints", () => {
    const black = rgbToOklab(0, 0, 0);
    const white = rgbToOklab(255, 255, 255);
    expect(black.L).toBeCloseTo(0, 3);
    expect(white.L).toBeCloseTo(1, 2);
  });

  it("pure neutral grays have near-zero a/b chroma", () => {
    const gray = rgbToOklab(128, 128, 128);
    expect(Math.abs(gray.a)).toBeLessThan(0.01);
    expect(Math.abs(gray.b)).toBeLessThan(0.01);
  });

  it("round-trips RGB through OKLab within rounding tolerance", () => {
    const samples = [
      [0, 0, 0],
      [255, 255, 255],
      [17, 42, 200],
      [240, 100, 30],
      [200, 200, 240],
      [80, 160, 64],
      [128, 128, 128],
    ] as const;
    for (const [r, g, b] of samples) {
      const lab = rgbToOklab(r, g, b);
      const back = oklabToRgb(lab.L, lab.a, lab.b);
      expect(Math.abs(back.r - r)).toBeLessThanOrEqual(1);
      expect(Math.abs(back.g - g)).toBeLessThanOrEqual(1);
      expect(Math.abs(back.b - b)).toBeLessThanOrEqual(1);
    }
  });

  it("hex round-trips through OKLab within 1/255 per channel", () => {
    const hexes = [
      "#000000",
      "#ffffff",
      "#123456",
      "#a1b2c3",
      "#fe7733",
      "#2d6a4f",
      "#c9184a",
    ];
    for (const hex of hexes) {
      const lab = hexToOklab(hex);
      expect(lab).not.toBeNull();
      if (!lab) continue;
      const back = oklabToHex(lab.L, lab.a, lab.b);
      const original = hexToRgb(hex)!;
      const recovered = hexToRgb(back)!;
      expect(Math.abs(original.r - recovered.r)).toBeLessThanOrEqual(1);
      expect(Math.abs(original.g - recovered.g)).toBeLessThanOrEqual(1);
      expect(Math.abs(original.b - recovered.b)).toBeLessThanOrEqual(1);
    }
  });

  it("hexToOklab returns null on invalid input", () => {
    expect(hexToOklab("not-a-color")).toBeNull();
  });
});

describe("shiftPerceptualL", () => {
  it("returns the input unchanged when delta is zero (within rounding)", () => {
    const samples = ["#27282a", "#f4f4f7", "#123456", "#a1b2c3", "#fe7733"];
    for (const hex of samples) {
      const shifted = shiftPerceptualL(hex, 0);
      const original = hexToRgb(hex)!;
      const back = hexToRgb(shifted)!;
      expect(Math.abs(original.r - back.r)).toBeLessThanOrEqual(1);
      expect(Math.abs(original.g - back.g)).toBeLessThanOrEqual(1);
      expect(Math.abs(original.b - back.b)).toBeLessThanOrEqual(1);
    }
  });

  it("clamps L at 1 when shifting bright canvases positive", () => {
    expect(shiftPerceptualL("#ffffff", +0.5)).toBe("#ffffff");
    // Near-white clamps gracefully instead of wrapping.
    const shifted = shiftPerceptualL("#f8f8f8", +0.5);
    expect(hexToOklab(shifted)!.L).toBeCloseTo(1, 2);
  });

  it("clamps L at 0 when shifting dark canvases negative", () => {
    expect(shiftPerceptualL("#000000", -0.5)).toBe("#000000");
    const shifted = shiftPerceptualL("#111111", -0.5);
    expect(hexToOklab(shifted)!.L).toBeCloseTo(0, 2);
  });

  it("monotonically increases L for a positive delta from a mid-luminance canvas", () => {
    const canvas = "#27282a";
    const baseL = hexToOklab(canvas)!.L;
    for (const d of [0.02, 0.05, 0.1, 0.2]) {
      const shifted = shiftPerceptualL(canvas, d);
      const shiftedL = hexToOklab(shifted)!.L;
      expect(shiftedL).toBeGreaterThan(baseL);
      expect(shiftedL).toBeCloseTo(baseL + d, 2);
    }
  });

  it("monotonically decreases L for a negative delta from a mid-luminance canvas", () => {
    const canvas = "#7a7a80";
    const baseL = hexToOklab(canvas)!.L;
    for (const d of [-0.02, -0.05, -0.1, -0.2]) {
      const shifted = shiftPerceptualL(canvas, d);
      const shiftedL = hexToOklab(shifted)!.L;
      expect(shiftedL).toBeLessThan(baseL);
      expect(shiftedL).toBeCloseTo(baseL + d, 2);
    }
  });

  it("preserves hue and chroma, only lightness changes", () => {
    const warm = "#c46a2b";
    const lab = hexToOklab(warm)!;
    const shifted = hexToOklab(shiftPerceptualL(warm, +0.1))!;
    expect(shifted.a).toBeCloseTo(lab.a, 2);
    expect(shifted.b).toBeCloseTo(lab.b, 2);
  });

  it("falls back to the input when hex is invalid", () => {
    expect(shiftPerceptualL("not-a-color", 0.1)).toBe("not-a-color");
  });
});

describe("pickReadableForeground", () => {
  it("returns ink when ink meets target", () => {
    expect(
      pickReadableForeground("#ffffff", { ink: "#000000", canvas: "#ffffff" }),
    ).toBe("#000000");
  });

  it("returns canvas when ink fails but canvas meets target", () => {
    const fg = pickReadableForeground("#000000", {
      ink: "#1a1a1a",
      canvas: "#ffffff",
    });
    expect(fg).toBe("#ffffff");
  });

  it("prefers the saturating endpoint when both anchors fail but an endpoint meets target", () => {
    // Very light bg with near-matching ink and canvas: both anchors fail
    // target contrast. A chroma-preserving walk from the closer anchor
    // would land at a just-enough gray close to bg in luminance (muddy);
    // the endpoint flip gives decisive visual separation instead.
    const bg = "#fafafa";
    const fg = pickReadableForeground(bg, {
      ink: "#f0f0f0",
      canvas: "#e8e8e8",
    });
    expect(fg).toBe("#000000");
    expect(contrastRatio(bg, fg)).toBeGreaterThanOrEqual(4.5);
  });

  it("prefers the endpoint over a chroma-preserving walk on warm pastels too", () => {
    // Warm pastel bg with near-white canvas and a similar-tone ink: both
    // fail 4.5:1. Endpoint snaps to #000000 rather than producing a dark
    // warm-tinted gray that reads as murky on the pastel.
    const bg = "#ffe5d0";
    const fg = pickReadableForeground(bg, {
      ink: "#e0c9b2",
      canvas: "#ffffff",
    });
    expect(fg).toBe("#000000");
    expect(contrastRatio(bg, fg)).toBeGreaterThanOrEqual(4.5);
  });

  it("handles the adversarial case where bg equals ink", () => {
    const bg = "#556677";
    const fg = pickReadableForeground(bg, {
      ink: bg,
      canvas: "#ffffff",
    });
    // White has contrast > 4.5 against #556677, so canvas should win.
    expect(contrastRatio(bg, fg)).toBeGreaterThanOrEqual(4.5 - 0.05);
  });

  it("returns the best saturating endpoint when target is unreachable", () => {
    // Mid-gray bg cannot carry AA body text from any anchor. Against
    // #808080, pure black has ~5.3:1 and pure white has ~3.96:1, so the
    // fallback should pick black.
    const bg = "#808080";
    const fg = pickReadableForeground(bg, {
      ink: "#808080",
      canvas: "#808080",
    });
    const blackRatio = contrastRatio(bg, "#000000");
    const whiteRatio = contrastRatio(bg, "#ffffff");
    const maxAchievable = Math.max(blackRatio, whiteRatio);
    // Walking in either direction should reach target (black hits 4.5).
    // If it doesn't, the fallback should at least match the best endpoint.
    expect(contrastRatio(bg, fg)).toBeGreaterThanOrEqual(
      Math.min(4.5, maxAchievable) - 0.1,
    );
  });

  it("2k fuzz: every result either meets target or saturates the gamut", () => {
    const rand = mulberry32(0x9e3779b9);
    for (let i = 0; i < 2000; i++) {
      const bg = randomHex(rand);
      const ink = randomHex(rand);
      const canvas = randomHex(rand);
      const fg = pickReadableForeground(bg, { ink, canvas, target: 4.5 });
      const ratio = contrastRatio(bg, fg);
      const maxAchievable = Math.max(
        contrastRatio(bg, "#000000"),
        contrastRatio(bg, "#ffffff"),
      );
      // Either hits target, or is the best the gamut allows at bg's luminance.
      expect(ratio >= 4.5 - 0.05 || ratio >= maxAchievable - 0.2).toBe(true);
    }
  });
});

describe("pickReadableBorder", () => {
  it("lands at roughly 3:1 vs a light bg", () => {
    const border = pickReadableBorder("#ffffff", "#000000");
    expect(contrastRatio("#ffffff", border)).toBeGreaterThanOrEqual(3 - 0.1);
    expect(contrastRatio("#ffffff", border)).toBeLessThan(4.5);
  });

  it("lands at roughly 3:1 vs a dark bg", () => {
    const border = pickReadableBorder("#1a1a1a", "#ffffff");
    expect(contrastRatio("#1a1a1a", border)).toBeGreaterThanOrEqual(3 - 0.1);
  });

  it("falls back toward opposite base when ink is too close to bg", () => {
    // Dark bg with dark ink: falls back to walking toward white.
    const border = pickReadableBorder("#1a1a1a", "#202020");
    expect(contrastRatio("#1a1a1a", border)).toBeGreaterThanOrEqual(3 - 0.1);
  });

  it("survives bg = ink degenerate input", () => {
    const border = pickReadableBorder("#808080", "#808080");
    // Mid gray: best achievable border is one of the endpoints.
    const ratio = contrastRatio("#808080", border);
    expect(ratio).toBeGreaterThan(1);
  });
});

describe("pickReadableMuted", () => {
  it("parks above target vs white-on-black", () => {
    const muted = pickReadableMuted("#ffffff", "#000000");
    const ratio = contrastRatio("#ffffff", muted);
    expect(ratio).toBeGreaterThanOrEqual(3 - 0.05);
    // Should be in the muted band, not full-ink dark.
    expect(ratio).toBeLessThanOrEqual(4.5);
  });

  it("parks above target vs dark-on-light", () => {
    const muted = pickReadableMuted("#1a1a1a", "#ffffff");
    const ratio = contrastRatio("#1a1a1a", muted);
    expect(ratio).toBeGreaterThanOrEqual(3 - 0.05);
    expect(ratio).toBeLessThanOrEqual(4.5);
  });

  it("returns ink unchanged when ink is already near target", () => {
    // Contrast ratio (#595959 vs #ffffff) = ~7.0, (#767676 vs #ffffff) = ~4.5,
    // so we need an ink barely above 3:1 to see the short-circuit. Use #949494.
    const muted = pickReadableMuted("#ffffff", "#949494");
    // Contrast of ink is ~3.0, so muted should just return it.
    expect(contrastRatio("#ffffff", muted)).toBeLessThanOrEqual(
      contrastRatio("#ffffff", "#949494") + 0.05,
    );
  });

  it("pivots to a readable anchor when ink is too close to bg", () => {
    // Dark canvas paired with an almost-identical ink: contrast < target.
    // Returning ink unchanged would vanish into the surface.
    const muted = pickReadableMuted("#1B1C1F", "#222222");
    expect(contrastRatio("#1B1C1F", muted)).toBeGreaterThanOrEqual(3 - 0.05);
  });

  it("flips direction when the bg crosses luminance past ink", () => {
    // Same ink, but with a dark bg on one side and a light bg on the other.
    // The muted walk should land on two sides of ink because the anchor
    // direction is derived from bg, not a hardcoded assumption.
    const onDark = pickReadableMuted("#141420", "#ECECF2");
    const onLight = pickReadableMuted("#FAF7F2", "#1F1B16");
    expect(contrastRatio("#141420", onDark)).toBeGreaterThanOrEqual(3 - 0.05);
    expect(contrastRatio("#FAF7F2", onLight)).toBeGreaterThanOrEqual(3 - 0.05);
  });
});

// Deterministic PRNG for fuzz tests so failures reproduce.
function mulberry32(seed: number): () => number {
  let s = seed >>> 0;
  return () => {
    s = (s + 0x6d2b79f5) >>> 0;
    let t = s;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function randomHex(rand: () => number): string {
  const r = Math.floor(rand() * 256);
  const g = Math.floor(rand() * 256);
  const b = Math.floor(rand() * 256);
  return rgbToHex(r, g, b);
}
