import { describe, expect, it } from "vitest";
import type { SegmentStatus, TimelineBand } from "$lib/calendar/types";
import { BreakMarkMotion, keyBreakBands, type BreakMark, type BreakMarkOwner } from "./break-mark-motion";
import { computeDayTimelineBands, type TimelineEvent } from "./segments";
import { createPresetPomodoroConfig } from "./rhythm";

const FRAME_SECONDS = 1 / 60;

function band(
  ownerId: string,
  topMinute: number,
  heightMinutes = 5,
  { status = "planned", anchored = true }: { status?: SegmentStatus; anchored?: boolean } = {},
): TimelineBand {
  return { topMinute, heightMinutes, phase: "short_break", status, ownerId, anchored };
}

function owners(entries: Record<string, [number, number]>): Map<string, BreakMarkOwner> {
  return new Map(Object.entries(entries).map(([id, [startMinute, endMinute]]) => [id, { startMinute, endMinute }]));
}

function mark(motion: BreakMarkMotion, key: string): BreakMark | undefined {
  return motion.marks().find((candidate) => candidate.key === key);
}

/** Steps the motion frame by frame until it rests, returning every drawn frame. */
function settle(motion: BreakMarkMotion, limitSeconds = 3): BreakMark[][] {
  const frames: BreakMark[][] = [];
  for (let elapsed = 0; motion.moving && elapsed < limitSeconds; elapsed += FRAME_SECONDS) {
    motion.step(FRAME_SECONDS);
    frames.push(motion.marks());
  }
  return frames;
}

describe("keyBreakBands", () => {
  it("numbers each owner's breaks in time order, ignoring focus fills", () => {
    const keyed = keyBreakBands([
      band("B", 700),
      { ...band("A", 600, 10, { status: "completed" }), phase: "focus" },
      band("A", 650),
      band("A", 620, 5, { status: "completed" }),
      band("B", 760),
    ]);
    expect(keyed.map(({ key, band: { topMinute } }) => [key, topMinute])).toEqual([
      ["A#0", 620], ["A#1", 650], ["B#0", 700], ["B#1", 760],
    ]);
  });
});

describe("BreakMarkMotion", () => {
  it("places the first marks exactly, without motion", () => {
    const motion = new BreakMarkMotion();
    motion.retarget([band("A", 640), band("A", 685)], owners({ A: [600, 720] }));
    expect(motion.moving).toBe(false);
    expect(motion.marks()).toEqual([
      { key: "A#0", topMinute: 640, heightMinutes: 5, phase: "short_break", status: "planned", opacity: 1 },
      { key: "A#1", topMinute: 685, heightMinutes: 5, phase: "short_break", status: "planned", opacity: 1 },
    ]);
  });

  it("slides a replanned break steadily to its new place without passing it", () => {
    const motion = new BreakMarkMotion();
    const range = owners({ B: [600, 720] });
    motion.retarget([band("B", 640)], range);
    motion.retarget([band("B", 610, 10)], range);

    expect(mark(motion, "B#0")?.topMinute).toBe(640);
    const frames = settle(motion);
    const tops = frames.map((frame) => frame[0].topMinute);
    expect(tops.length).toBeGreaterThan(10);
    for (let index = 1; index < tops.length; index++) {
      expect(tops[index]).toBeLessThanOrEqual(tops[index - 1]);
      expect(tops[index]).toBeGreaterThanOrEqual(610);
    }
    expect(motion.marks()).toEqual([
      { key: "B#0", topMinute: 610, heightMinutes: 10, phase: "short_break", status: "planned", opacity: 1 },
    ]);
    expect(frames.length * FRAME_SECONDS).toBeLessThan(1);
  });

  it("moves the marks of a dragged event with it at once", () => {
    const motion = new BreakMarkMotion();
    motion.retarget([band("A", 640), band("A", 685)], owners({ A: [600, 720] }));
    motion.retarget([band("A", 670), band("A", 715)], owners({ A: [630, 750] }));
    expect(motion.moving).toBe(false);
    expect(motion.marks().map((drawn) => drawn.topMinute)).toEqual([670, 715]);
  });

  it("carries the marks of a dragged event along and eases only the replanned part", () => {
    const motion = new BreakMarkMotion();
    motion.retarget([band("A", 640)], owners({ A: [600, 720] }));
    // Moved 30 minutes later, and its break now falls 10 minutes earlier within the event.
    motion.retarget([band("A", 660)], owners({ A: [630, 750] }));
    expect(mark(motion, "A#0")?.topMinute).toBe(670);
    settle(motion);
    expect(mark(motion, "A#0")?.topMinute).toBe(660);
  });

  it("keeps the marks of an event cut by an earlier one still while it is dragged under that event", () => {
    const motion = new BreakMarkMotion();
    // B starts under A, so its plan opens at A's end at 660 and does not move with B's start.
    motion.retarget([band("B", 700, 5, { anchored: false })], owners({ B: [630, 780] }));
    motion.retarget([band("B", 700, 5, { anchored: false })], owners({ B: [645, 780] }));
    expect(motion.moving).toBe(false);
    expect(mark(motion, "B#0")?.topMinute).toBe(700);
  });

  it("eases the marks of an event into the room an overlapping event leaves", () => {
    const motion = new BreakMarkMotion();
    motion.retarget([band("B", 700, 5, { anchored: false })], owners({ B: [630, 780] }));
    // The overlapping event moved away, so B's plan opens at its own start instead.
    motion.retarget([band("B", 670)], owners({ B: [630, 780] }));
    expect(mark(motion, "B#0")?.topMinute).toBe(700);
    expect(motion.moving).toBe(true);
    settle(motion);
    expect(mark(motion, "B#0")?.topMinute).toBe(670);
  });

  it("fades out a break its event loses, in place and clipped to the event", () => {
    const motion = new BreakMarkMotion();
    motion.retarget([band("A", 640), band("A", 685)], owners({ A: [600, 720] }));
    motion.retarget([band("A", 640)], owners({ A: [600, 687] }));

    const leaving = mark(motion, "A#1");
    expect(leaving).toMatchObject({ topMinute: 685, heightMinutes: 2, opacity: 1 });
    const frames = settle(motion);
    const opacities = frames.map((frame) => frame.find((drawn) => drawn.key === "A#1")?.opacity ?? 0);
    for (let index = 1; index < opacities.length; index++) expect(opacities[index]).toBeLessThanOrEqual(opacities[index - 1]);
    for (const frame of frames) {
      const drawn = frame.find((candidate) => candidate.key === "A#1");
      if (drawn) expect(drawn.topMinute).toBe(685);
    }
    expect(motion.marks().map((drawn) => drawn.key)).toEqual(["A#0"]);
    expect(motion.moving).toBe(false);
  });

  it("does not draw a leaving break once its event no longer covers it", () => {
    const motion = new BreakMarkMotion();
    motion.retarget([band("A", 640), band("A", 685)], owners({ A: [600, 720] }));
    motion.retarget([band("A", 640)], owners({ A: [600, 675] }));
    expect(motion.marks().map((drawn) => drawn.key)).toEqual(["A#0"]);
  });

  it("fades in a break an existing event gains, already in its place", () => {
    const motion = new BreakMarkMotion();
    motion.retarget([band("A", 640)], owners({ A: [600, 680] }));
    motion.retarget([band("A", 640), band("A", 685)], owners({ A: [600, 720] }));

    expect(mark(motion, "A#1")).toBeUndefined();
    const frames = settle(motion);
    for (const frame of frames) {
      const drawn = frame.find((candidate) => candidate.key === "A#1");
      if (drawn) expect(drawn.topMinute).toBe(685);
    }
    expect(mark(motion, "A#1")?.opacity).toBe(1);
  });

  it("shows and hides the marks of an event that appears or disappears together with its rail", () => {
    const motion = new BreakMarkMotion();
    motion.retarget([band("A", 640)], owners({ A: [600, 680] }));
    motion.retarget([band("A", 640), band("B", 745)], owners({ A: [600, 680], B: [700, 800] }));
    expect(mark(motion, "B#0")?.opacity).toBe(1);

    motion.retarget([band("A", 640)], owners({ A: [600, 680] }));
    expect(mark(motion, "B#0")).toBeUndefined();
    expect(motion.moving).toBe(false);
  });

  it("puts started and recorded breaks exactly where they ran", () => {
    const motion = new BreakMarkMotion();
    const range = owners({ A: [600, 720] });
    motion.retarget([band("A", 640)], range);
    motion.retarget([band("A", 643, 1, { status: "active", anchored: false })], range);
    expect(motion.moving).toBe(false);
    expect(mark(motion, "A#0")).toMatchObject({ topMinute: 643, heightMinutes: 1, status: "active" });

    motion.retarget([band("A", 643, 6, { status: "completed", anchored: false })], range);
    expect(motion.moving).toBe(false);
    expect(mark(motion, "A#0")).toMatchObject({ topMinute: 643, heightMinutes: 6, status: "completed" });
  });

  it("removes a recorded break at once when it is gone", () => {
    const motion = new BreakMarkMotion();
    const range = owners({ A: [600, 720] });
    motion.retarget([band("A", 643, 6, { status: "completed", anchored: false })], range);
    motion.retarget([], range);
    expect(motion.marks()).toEqual([]);
    expect(motion.moving).toBe(false);
  });

  it("follows a plan that starts at now in small steps without animating", () => {
    const motion = new BreakMarkMotion();
    motion.retarget([band("A", 645, 5, { anchored: false })], owners({ A: [540, 720] }));
    motion.retarget([band("A", 645 + 1 / 60, 5, { anchored: false })], owners({ A: [540, 720] }));
    expect(motion.moving).toBe(false);
    expect(mark(motion, "A#0")?.topMinute).toBeCloseTo(645 + 1 / 60, 9);
  });

  it("bends toward a target that changes mid-slide instead of stopping", () => {
    const motion = new BreakMarkMotion();
    const range = owners({ B: [600, 800] });
    motion.retarget([band("B", 700)], range);
    motion.retarget([band("B", 680)], range);
    for (let frame = 0; frame < 5; frame++) motion.step(FRAME_SECONDS);
    const earlier = mark(motion, "B#0")!.topMinute;
    motion.step(FRAME_SECONDS);
    const before = mark(motion, "B#0")!.topMinute;
    motion.retarget([band("B", 650)], range);
    motion.step(FRAME_SECONDS);
    const after = mark(motion, "B#0")!.topMinute;
    // The retarget keeps the speed it had, and the farther target only adds to it.
    expect(before - after).toBeGreaterThanOrEqual(earlier - before);
    settle(motion);
    expect(mark(motion, "B#0")?.topMinute).toBe(650);
  });

  it("does not move for a plan that changes and changes back before the next frame", () => {
    const motion = new BreakMarkMotion();
    const range = owners({ A: [600, 720], B: [720, 840] });
    const settledBands = [band("A", 640), band("B", 760)];
    motion.retarget(settledBands, range);
    motion.retarget([band("A", 640)], owners({ A: [600, 720] }));
    motion.retarget([band("A", 640), band("B", 760)], range);
    motion.retarget([band("A", 640), band("B", 745)], range);
    motion.retarget(settledBands, range);
    expect(motion.moving).toBe(false);
    expect(motion.marks().map((drawn) => [drawn.key, drawn.topMinute, drawn.opacity])).toEqual([
      ["A#0", 640, 1], ["B#0", 760, 1],
    ]);
  });

  it("clips a mark sliding past its event's edge to the event", () => {
    const motion = new BreakMarkMotion();
    motion.retarget([band("A", 700)], owners({ A: [600, 720] }));
    // The event was shortened from its start, and its break replanned to its new top.
    motion.retarget([band("A", 690, 5, { anchored: false })], owners({ A: [690, 720] }));
    motion.retarget([band("A", 690, 5, { anchored: false })], owners({ A: [690, 702] }));
    expect(mark(motion, "A#0")).toMatchObject({ topMinute: 700, heightMinutes: 2 });
  });

  it("jumps everything into place and drops leaving marks when motion is reduced", () => {
    const motion = new BreakMarkMotion();
    motion.retarget([band("A", 640), band("A", 685)], owners({ A: [600, 720] }));
    motion.retarget([band("A", 620)], owners({ A: [600, 720] }), true);
    expect(motion.moving).toBe(false);
    expect(motion.marks().map((drawn) => [drawn.key, drawn.topMinute])).toEqual([["A#0", 620]]);
  });

  it("places another day's marks in place after clearing, even for the same events", () => {
    const motion = new BreakMarkMotion();
    motion.retarget([band("A", 640)], owners({ A: [600, 720] }));
    motion.clear();
    motion.retarget([band("A", 100), band("A", 160)], owners({ A: [0, 200] }));
    expect(motion.moving).toBe(false);
    expect(motion.marks().map((drawn) => [drawn.topMinute, drawn.opacity])).toEqual([[100, 1], [160, 1]]);
  });

  it("finishes a slide after a long pause without jumping past it", () => {
    const motion = new BreakMarkMotion();
    const range = owners({ A: [600, 720] });
    motion.retarget([band("A", 640)], range);
    motion.retarget([band("A", 610)], range);
    motion.step(30);
    const top = mark(motion, "A#0")!.topMinute;
    expect(top).toBeGreaterThan(610);
    expect(top).toBeLessThan(640);
  });

  it("slides a later event's breaks when an adjacent event is drawn ahead of it", () => {
    const day = new Date("2026-03-21T00:00:00").getTime();
    const event = (id: string, start: number, end: number, preset: "extended" | "creative"): TimelineEvent => ({
      id, config: createPresetPomodoroConfig(preset),
      startMs: day + start * 60_000, endMs: day + end * 60_000, startMinute: start, endMinute: end,
    });
    const later = event("later", 765, 885, "creative");
    const ahead = event("ahead", 600, 765, "extended");
    const rangeOf = (events: TimelineEvent[]) => new Map(events.map((ev) => [ev.id, { startMinute: ev.startMinute, endMinute: ev.endMinute }]));
    const past = day - 86_400_000;

    const motion = new BreakMarkMotion();
    const alone = computeDayTimelineBands([later], null, day, past);
    motion.retarget(alone, rangeOf([later]));
    const together = computeDayTimelineBands([ahead, later], null, day, past);
    motion.retarget(together, rangeOf([ahead, later]));

    const laterTargets = keyBreakBands(together).filter(({ band: { ownerId } }) => ownerId === "later");
    const aloneTargets = keyBreakBands(alone);
    expect(laterTargets.map(({ band: { topMinute } }) => topMinute))
      .not.toEqual(aloneTargets.map(({ band: { topMinute } }) => topMinute));
    for (const { key, band: { topMinute } } of aloneTargets) {
      if (laterTargets.some((target) => target.key === key)) expect(mark(motion, key)?.topMinute).toBe(topMinute);
    }
    for (const { key, band: { topMinute } } of keyBreakBands(together).filter(({ band: { ownerId } }) => ownerId === "ahead")) {
      expect(mark(motion, key)).toMatchObject({ topMinute, opacity: 1 });
    }

    settle(motion);
    for (const { key, band: { topMinute } } of keyBreakBands(together)) expect(mark(motion, key)?.topMinute).toBe(topMinute);
  });
});
