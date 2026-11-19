import { describe, expect, it } from "vitest";
import type { UserThemeRead, UserThemeWrite } from "$lib/api/themes";
import { PALETTE_SIZE } from "$lib/calendar/types";
import {
  BASE_APP_TOKENS,
  BASE_CALENDAR_TOKENS,
  CALENDAR_TOKEN_KEYS,
  DEFAULT_CALENDAR_DEFAULT_CUSTOM,
  THEME_TOKEN_ROW_ORDER,
  darkTheme,
  lightTheme,
} from "./definitions";
import { userThemeFromRead, userThemeToWrite } from "./stored-rows";

const THEME_ID = "harbor-night";

/** A stored theme with no token or palette rows, so every value takes its fallback. */
function emptyRead(overrides: Partial<UserThemeRead["theme"]> = {}): UserThemeRead {
  return {
    theme: {
      id: THEME_ID,
      display_name: "Harbor night",
      blend_canvas: "#101820",
      seed_blend_canvas: "#101820",
      derivation_engine_version: 1,
      calendar_default_mode: "app-canvas",
      calendar_default_custom: "#101820",
      seed_calendar_default_mode: "app-canvas",
      seed_calendar_default_custom: "#101820",
      icon_label: "dark",
      seed_icon_label: "dark",
      created_at_ms: 0,
      updated_at_ms: 0,
      has_conflict: false,
      ...overrides,
    },
    tokens: [],
    palette: [],
    seedTokens: [],
    seedPalette: [],
  };
}

/** Turn written row groups back into the shape the themes API reads. */
function readFromWrite(write: UserThemeWrite, base: UserThemeRead): UserThemeRead {
  const tokenRows = (rows: UserThemeWrite["tokens"]) =>
    rows.map((row) => ({ theme_id: write.id, kind: row.kind, key: row.key, value: row.value, isolated: row.isolated ? 1 : 0 }));
  const paletteRows = (rows: UserThemeWrite["palette"]) =>
    rows.map((row) => ({ theme_id: write.id, slot: row.slot, value: row.value }));
  return {
    theme: base.theme,
    tokens: tokenRows(write.tokens),
    palette: paletteRows(write.palette),
    seedTokens: tokenRows(write.seedTokens),
    seedPalette: paletteRows(write.seedPalette),
  };
}

describe("userThemeFromRead", () => {
  it("backfills a theme without rows from the dark base tables", () => {
    const theme = userThemeFromRead(emptyRead());

    expect(theme.sources.canvas).toBe(BASE_APP_TOKENS.dark["--background"]);
    expect(theme.sources.ink).toBe(BASE_APP_TOKENS.dark["--foreground"]);
    expect(theme.eventPalette).toEqual([...darkTheme.eventPalette]);
    for (const key of CALENDAR_TOKEN_KEYS) {
      expect(theme.calendarTokens[key]).toBe(BASE_CALENDAR_TOKENS.dark[key]);
    }
    expect(theme.appIsolated.size).toBe(0);
  });

  it("picks the light fallback tables from a light canvas row", () => {
    const read = emptyRead();
    read.tokens = [{ theme_id: THEME_ID, kind: "source", key: "canvas", value: "#ffffff", isolated: 0 }];

    const theme = userThemeFromRead(read);

    expect(theme.sources.canvas).toBe("#ffffff");
    expect(theme.sources.ink).toBe(BASE_APP_TOKENS.light["--foreground"]);
    expect(theme.eventPalette).toEqual([...lightTheme.eventPalette]);
    expect(theme.seedEventPalette).toEqual([...darkTheme.eventPalette]);
  });

  it("keeps in-range palette slots and ignores slots outside the palette", () => {
    const read = emptyRead();
    read.palette = [
      { theme_id: THEME_ID, slot: 0, value: "#123456" },
      { theme_id: THEME_ID, slot: -1, value: "#abcdef" },
      { theme_id: THEME_ID, slot: PALETTE_SIZE, value: "#abcdef" },
    ];

    const theme = userThemeFromRead(read);

    expect(theme.eventPalette).toHaveLength(darkTheme.eventPalette.length);
    expect(theme.eventPalette[0]).toBe("#123456");
    expect(theme.eventPalette).not.toContain("#abcdef");
  });

  it("falls back from unknown calendar default modes and malformed custom colors", () => {
    const theme = userThemeFromRead(emptyRead({
      calendar_default_mode: "sepia",
      calendar_default_custom: "blue",
      seed_calendar_default_mode: "custom",
      seed_calendar_default_custom: "#a1b2c3d4",
    }));

    expect(theme.calendarDefaultMode).toBe("app-canvas");
    expect(theme.calendarDefaultCustom).toBe(theme.sources.canvas);
    expect(theme.seedCalendarDefaultMode).toBe("custom");
    expect(theme.seedCalendarDefaultCustom).toBe("#a1b2c3d4");
  });

  it("uses the default custom color when neither the row nor the canvas is a hex color", () => {
    const read = emptyRead({ calendar_default_custom: "blue" });
    read.tokens = [{ theme_id: THEME_ID, kind: "source", key: "canvas", value: "rgb(0 0 0)", isolated: 0 }];

    expect(userThemeFromRead(read).calendarDefaultCustom).toBe(DEFAULT_CALENDAR_DEFAULT_CUSTOM);
  });
});

describe("userThemeToWrite", () => {
  it("writes token rows in the stored row order", () => {
    const write = userThemeToWrite(userThemeFromRead(emptyRead()));

    expect(write.tokens.map((row) => `${row.kind}:${row.key}`)).toEqual(
      THEME_TOKEN_ROW_ORDER.map((entry) => `${entry.kind}:${entry.key}`),
    );
    expect(write.palette.map((row) => row.slot)).toEqual(darkTheme.eventPalette.map((_, slot) => slot));
  });

  it("round trips through the stored rows without losing values or isolation", () => {
    const read = emptyRead({ calendar_default_mode: "custom", calendar_default_custom: "#203040" });
    const calendarKey = CALENDAR_TOKEN_KEYS[0];
    read.tokens = [
      { theme_id: THEME_ID, kind: "source", key: "primary", value: "#3366ff", isolated: 0 },
      { theme_id: THEME_ID, kind: "calendar", key: calendarKey, value: "#445566", isolated: 1 },
    ];
    read.palette = [{ theme_id: THEME_ID, slot: 2, value: "#778899" }];
    const theme = userThemeFromRead(read);

    const reread = userThemeFromRead(readFromWrite(userThemeToWrite(theme), read));

    expect(reread).toEqual(theme);
    expect(reread.calendarIsolated.has(calendarKey)).toBe(true);
    expect(reread.eventPalette[2]).toBe("#778899");
  });
});
