import { describe, expect, it } from "vitest";
import {
  cursorClick,
  cursorRest,
  WELCOME_CURSOR_FADE_MS,
  WELCOME_CURSOR_IDLE_MS,
  WELCOME_CURSOR_MIN_TRAVEL_MS,
  WELCOME_CURSOR_REST_MS,
  WELCOME_CURSOR_TRAVEL_MS,
  welcomeCursorFrame,
  welcomeCursorHovered,
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

describe("welcome cursor idling", () => {
  /** A pointer that rests from the start and next clicks long after, with nothing else to do in between. */
  const IDLE_SCRIPT: WelcomeCursorScript = [cursorRest("start", 0), cursorClick("save", 6000)];
  const saveDepart = IDLE_SCRIPT[1].depart;

  it("fades in as the scene starts", () => {
    expect(welcomeCursorFrame(SCRIPT, 0).shown).toBe(0);
    expect(welcomeCursorFrame(SCRIPT, WELCOME_CURSOR_FADE_MS / 2).shown).toBeCloseTo(0.5);
    expect(welcomeCursorFrame(SCRIPT, WELCOME_CURSOR_FADE_MS).shown).toBe(1);
  });

  it("stays shown through short waits between its moves and clicks", () => {
    for (let elapsed = WELCOME_CURSOR_FADE_MS; elapsed <= 4000 + WELCOME_CURSOR_IDLE_MS; elapsed += 10) {
      expect(welcomeCursorFrame(SCRIPT, elapsed)).toMatchObject({ shown: 1, away: false });
    }
  });

  it("fades out once it has rested idle through a long wait, hovers nothing while hidden, and fades back in place before it moves", () => {
    expect(welcomeCursorFrame(IDLE_SCRIPT, WELCOME_CURSOR_IDLE_MS).shown).toBe(1);
    expect(welcomeCursorFrame(IDLE_SCRIPT, WELCOME_CURSOR_IDLE_MS + WELCOME_CURSOR_FADE_MS / 2).shown).toBeCloseTo(0.5);
    expect(welcomeCursorFrame(IDLE_SCRIPT, WELCOME_CURSOR_IDLE_MS + WELCOME_CURSOR_FADE_MS)).toMatchObject({ shown: 0, away: true });
    expect(welcomeCursorHovered(IDLE_SCRIPT, 2000)).toBeNull();
    expect(welcomeCursorFrame(IDLE_SCRIPT, saveDepart - WELCOME_CURSOR_FADE_MS / 2)).toMatchObject({ to: { target: "start" } });
    expect(welcomeCursorFrame(IDLE_SCRIPT, saveDepart - WELCOME_CURSOR_FADE_MS / 2).shown).toBeCloseTo(0.5);
    expect(welcomeCursorFrame(IDLE_SCRIPT, saveDepart)).toMatchObject({ shown: 1, away: false });
  });

  it("hides after its last click", () => {
    expect(welcomeCursorFrame(IDLE_SCRIPT, 6000 + WELCOME_CURSOR_IDLE_MS + WELCOME_CURSOR_FADE_MS)).toMatchObject({ shown: 0, away: true });
  });

  it("stays shown while it is in use where it rests, such as while it scrolls, and idles only around that", () => {
    const busy = { start: 2000, end: 3000 };
    const script: WelcomeCursorScript = [cursorRest("start", 0, { busy: [busy] }), cursorClick("save", 6000)];
    expect(welcomeCursorFrame(script, busy.start - WELCOME_CURSOR_FADE_MS - 10).away).toBe(true);
    for (let elapsed = busy.start; elapsed <= busy.end + WELCOME_CURSOR_IDLE_MS; elapsed += 10) expect(welcomeCursorFrame(script, elapsed).shown).toBe(1);
    expect(welcomeCursorHovered(script, (busy.start + busy.end) / 2)).toBe("start");
    expect(welcomeCursorFrame(script, busy.end + WELCOME_CURSOR_IDLE_MS + WELCOME_CURSOR_FADE_MS).away).toBe(true);
  });

  it("does not leave a place while its button is still held", () => {
    expect(welcomeCursorScriptOrdered([cursorClick("a", 1000), cursorRest("b", 1000)])).toBe(false);
    expect(welcomeCursorScriptOrdered([cursorClick("a", 1000), cursorRest("b", 1000 + WELCOME_CURSOR_TRAVEL_MS)])).toBe(true);
  });
});
