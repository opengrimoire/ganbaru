import { describe, expect, it } from "vitest";
import {
  cursorClick,
  cursorRest,
  WELCOME_CURSOR_MIN_TRAVEL_MS,
  WELCOME_CURSOR_REST_MS,
  WELCOME_CURSOR_TRAVEL_MS,
  welcomeCursorFrame,
  welcomeCursorOn,
  welcomeCursorScriptOrdered,
  type WelcomeCursorScript,
} from "./welcome-cursor";
import { FILM_TAP_MS } from "./welcome-film";

const SCRIPT: WelcomeCursorScript = [
  cursorRest("start", 0),
  cursorClick("field", 2000, { glyph: "text" }),
  cursorClick("save", 4000),
];

describe("welcome cursor", () => {
  it("times a click so the pointer travels, rests, and holds the button until the tap lands", () => {
    const click = cursorClick("save", 4000);
    expect(click.press).toEqual({ start: 4000 - FILM_TAP_MS, end: 4000 });
    expect(click.arrive).toBe(4000 - FILM_TAP_MS - WELCOME_CURSOR_REST_MS);
    expect(click.depart).toBe(click.arrive - WELCOME_CURSOR_TRAVEL_MS);
  });

  it("waits for `after` before leaving, trading rest for a short glide before the button goes down", () => {
    const late = cursorClick("save", 4000, { after: 3400 });
    expect(late.depart).toBe(3400);
    expect(late.arrive).toBe(late.depart + WELCOME_CURSOR_MIN_TRAVEL_MS);
    expect(late.arrive).toBeLessThanOrEqual(late.press?.start ?? 0);
    const tooLate = cursorClick("save", 4000, { after: 3900 });
    expect(tooLate.arrive).toBe(4000 - FILM_TAP_MS);
    expect(tooLate.arrive - tooLate.depart).toBe(WELCOME_CURSOR_MIN_TRAVEL_MS);
    const early = cursorClick("save", 4000, { after: 1000 });
    expect(early.depart).toBe(early.arrive - WELCOME_CURSOR_TRAVEL_MS);
  });

  it("rests on the first place until the next one's departure, then travels and arrives", () => {
    expect(welcomeCursorFrame(SCRIPT, 0)).toMatchObject({ from: null, progress: 1 });
    expect(welcomeCursorFrame(SCRIPT, SCRIPT[1].depart - 1).to.target).toBe("start");
    const midway = welcomeCursorFrame(SCRIPT, (SCRIPT[1].depart + SCRIPT[1].arrive) / 2);
    expect(midway.from?.target).toBe("start");
    expect(midway.to.target).toBe("field");
    expect(midway.progress).toBeGreaterThan(0);
    expect(midway.progress).toBeLessThan(1);
    expect(welcomeCursorFrame(SCRIPT, SCRIPT[1].arrive).progress).toBe(1);
  });

  it("takes a place's shape as it reaches it and keeps it until it leaves", () => {
    expect(welcomeCursorFrame(SCRIPT, (SCRIPT[1].depart + SCRIPT[1].arrive) / 2).glyph).toBe("arrow");
    expect(welcomeCursorFrame(SCRIPT, SCRIPT[1].arrive).glyph).toBe("text");
    expect(welcomeCursorFrame(SCRIPT, SCRIPT[2].depart).glyph).toBe("text");
    expect(welcomeCursorFrame(SCRIPT, SCRIPT[2].arrive).glyph).toBe("arrow");
  });

  it("holds the button only through each click", () => {
    expect(welcomeCursorFrame(SCRIPT, 2000 - FILM_TAP_MS - 1).pressed).toBe(0);
    expect(welcomeCursorFrame(SCRIPT, 2000 - FILM_TAP_MS / 2).pressed).toBe(1);
    expect(welcomeCursorFrame(SCRIPT, 2000).pressed).toBe(0);
  });

  it("sees the pointer on a place from its arrival until it leaves", () => {
    expect(welcomeCursorOn(SCRIPT, SCRIPT[1].depart, "field")).toBe(false);
    expect(welcomeCursorOn(SCRIPT, SCRIPT[1].arrive, "field")).toBe(true);
    expect(welcomeCursorOn(SCRIPT, SCRIPT[2].depart, "field")).toBe(true);
    expect(welcomeCursorOn(SCRIPT, SCRIPT[2].arrive, "field")).toBe(false);
  });

  it("checks that places come one after another", () => {
    expect(welcomeCursorScriptOrdered(SCRIPT)).toBe(true);
    expect(welcomeCursorScriptOrdered([cursorRest("a", 0), cursorClick("b", 5000), cursorClick("c", 4000)])).toBe(false);
  });
});
