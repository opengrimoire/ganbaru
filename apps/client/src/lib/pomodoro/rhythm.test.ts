import { describe, expect, it } from "vitest";
import {
  breakAfterFocusPosition,
  configEquals,
  createCustomCountPomodoroConfig,
  createCustomSequencePomodoroConfig,
  createPresetPomodoroConfig,
  deriveInitialPhaseFromInheritedFocus,
  deriveRhythmPlan,
  focusDurationMinutesAtPosition,
  isValidPomodoroConfig,
  nextRhythmPosition,
  phaseDurationMinutesAtPosition,
  summarizeSequencePomodoroRhythm,
} from "./rhythm";

describe("pomodoro rhythm engine", () => {
  it.each([
    null, undefined, [], {},
    { rhythmSource: "custom", presetKey: null, idleTimeoutMinutes: null },
    { ...createPresetPomodoroConfig("adaptive"), presetKey: "toString" },
    { ...createPresetPomodoroConfig("adaptive"), presetKey: "__proto__" },
    { ...createPresetPomodoroConfig("adaptive"), rhythm: null },
    { ...createPresetPomodoroConfig("adaptive"), rhythm: { kind: "sequence" } },
    { ...createPresetPomodoroConfig("adaptive"), rhythm: { kind: "sequence", steps: [null] } },
    { ...createPresetPomodoroConfig("adaptive"), rhythm: { kind: "sequence", steps: Array(1) } },
    { ...createPresetPomodoroConfig("adaptive"), rhythm: {
      kind: "unknown",
      steps: [{ focusDurationMinutes: 25, breakPhase: "short_break", breakDurationMinutes: 5 }],
    } },
    { ...createPresetPomodoroConfig("adaptive"), idleTimeoutMinutes: "5" },
    { ...createPresetPomodoroConfig("adaptive"), idleTimeoutMinutes: Infinity },
  ])("rejects malformed configuration without throwing: %j", (value) => {
    expect(isValidPomodoroConfig(value)).toBe(false);
  });

  it("accepts complete preset and sequence configurations", () => {
    for (const blockConfig of [
      createPresetPomodoroConfig("adaptive"),
      createCustomSequencePomodoroConfig([
        { focusDurationMinutes: 25, breakPhase: "short_break", breakDurationMinutes: 5 },
      ]),
    ]) {
      expect(isValidPomodoroConfig(blockConfig)).toBe(true);
    }
  });

  it("summarizes every step in a sequence cycle", () => {
    expect(summarizeSequencePomodoroRhythm([
      { focusDurationMinutes: 20, breakPhase: "short_break", breakDurationMinutes: 4 },
      { focusDurationMinutes: 30, breakPhase: "long_break", breakDurationMinutes: 12 },
    ])).toEqual({ stepCount: 2, focusMinutes: 50, breakMinutes: 16 });
  });

  it("derives a count rhythm with a long break after one focus period", () => {
    const config = createCustomCountPomodoroConfig({
      focusDurationMinutes: 25,
      shortBreakMinutes: 5,
      longBreakMinutes: 15,
      longBreakAfterFocusCount: 1,
    });

    expect(breakAfterFocusPosition(config, 1)).toEqual({
      phase: "long_break",
      durationMinutes: 15,
    });
    expect(nextRhythmPosition(config, 1)).toBe(1);
  });

  it("repeats a short, long sequence by rhythm position", () => {
    const config = createCustomSequencePomodoroConfig([
      { focusDurationMinutes: 20, breakPhase: "short_break", breakDurationMinutes: 4 },
      { focusDurationMinutes: 30, breakPhase: "long_break", breakDurationMinutes: 12 },
    ]);

    const plan = deriveRhythmPlan(config, 70);

    expect(plan.segments).toEqual([
      { rhythmPosition: 1, phase: "focus", startOffsetMinutes: 0, endOffsetMinutes: 20 },
      { rhythmPosition: 1, phase: "short_break", startOffsetMinutes: 20, endOffsetMinutes: 24 },
      { rhythmPosition: 2, phase: "focus", startOffsetMinutes: 24, endOffsetMinutes: 54 },
      { rhythmPosition: 2, phase: "long_break", startOffsetMinutes: 54, endOffsetMinutes: 66 },
      { rhythmPosition: 1, phase: "focus", startOffsetMinutes: 66, endOffsetMinutes: 70 },
    ]);
    expect(plan.trailingState).toEqual({ focusOffsetMinutes: 4, rhythmPosition: 1 });
  });

  it("repeats a short, short, long sequence", () => {
    const config = createCustomSequencePomodoroConfig([
      { focusDurationMinutes: 10, breakPhase: "short_break", breakDurationMinutes: 2 },
      { focusDurationMinutes: 11, breakPhase: "short_break", breakDurationMinutes: 3 },
      { focusDurationMinutes: 12, breakPhase: "long_break", breakDurationMinutes: 9 },
    ]);

    expect(breakAfterFocusPosition(config, 1).phase).toBe("short_break");
    expect(breakAfterFocusPosition(config, 2).phase).toBe("short_break");
    expect(breakAfterFocusPosition(config, 3).phase).toBe("long_break");
    expect(nextRhythmPosition(config, 3)).toBe(1);
  });

  it("uses per-position focus and break durations", () => {
    const config = createCustomSequencePomodoroConfig([
      { focusDurationMinutes: 18, breakPhase: "short_break", breakDurationMinutes: 3 },
      { focusDurationMinutes: 42, breakPhase: "long_break", breakDurationMinutes: 20 },
    ]);

    expect(focusDurationMinutesAtPosition(config, 1)).toBe(18);
    expect(focusDurationMinutesAtPosition(config, 2)).toBe(42);
    expect(phaseDurationMinutesAtPosition("short_break", config, 1)).toBe(3);
    expect(phaseDurationMinutesAtPosition("long_break", config, 2)).toBe(20);
  });

  it("wraps overflow positions and defaults invalid positions to one", () => {
    const config = createCustomSequencePomodoroConfig([
      { focusDurationMinutes: 10, breakPhase: "short_break", breakDurationMinutes: 2 },
      { focusDurationMinutes: 15, breakPhase: "long_break", breakDurationMinutes: 8 },
    ]);

    expect(focusDurationMinutesAtPosition(config, 3)).toBe(10);
    expect(focusDurationMinutesAtPosition(config, 0)).toBe(10);
    expect(focusDurationMinutesAtPosition(config, -1)).toBe(10);
    expect(nextRhythmPosition(config, 2)).toBe(1);
  });

  it("derives inherited focus below, equal to, and above the threshold", () => {
    const config = createCustomCountPomodoroConfig({
      focusDurationMinutes: 40,
      shortBreakMinutes: 5,
      longBreakMinutes: 10,
      longBreakAfterFocusCount: 4,
    });

    expect(deriveInitialPhaseFromInheritedFocus(config, 39, 1)).toMatchObject({
      phase: "focus",
      rhythmPosition: 1,
      remainingSeconds: 60,
      inheritedFocusSeconds: 39 * 60,
    });
    expect(deriveInitialPhaseFromInheritedFocus(config, 40, 1)).toMatchObject({
      phase: "short_break",
      rhythmPosition: 1,
      remainingSeconds: 5 * 60,
      inheritedFocusSeconds: 40 * 60,
    });
    expect(deriveInitialPhaseFromInheritedFocus(config, 45, 4)).toMatchObject({
      phase: "long_break",
      rhythmPosition: 4,
      remainingSeconds: 10 * 60,
      inheritedFocusSeconds: 40 * 60,
    });
  });

  it("advances skipped breaks to the next rhythm position", () => {
    const config = createCustomSequencePomodoroConfig([
      { focusDurationMinutes: 20, breakPhase: "short_break", breakDurationMinutes: 4 },
      { focusDurationMinutes: 30, breakPhase: "long_break", breakDurationMinutes: 12 },
    ]);

    expect(nextRhythmPosition(config, 1)).toBe(2);
    expect(nextRhythmPosition(config, 2)).toBe(1);
  });

  it("compares config snapshots by exact rhythm fields", () => {
    expect(configEquals(createPresetPomodoroConfig("adaptive"), createPresetPomodoroConfig("adaptive"))).toBe(true);
    expect(configEquals(createPresetPomodoroConfig("adaptive"), createPresetPomodoroConfig("deep"))).toBe(false);
    expect(createPresetPomodoroConfig("balanced").rhythm).toEqual({
      kind: "count",
      focusDurationMinutes: 30,
      shortBreakMinutes: 5,
      longBreakMinutes: 10,
      longBreakAfterFocusCount: 4,
    });
    expect(configEquals(
      createCustomCountPomodoroConfig({
        focusDurationMinutes: 40,
        shortBreakMinutes: 5,
        longBreakMinutes: 10,
        longBreakAfterFocusCount: 4,
      }),
      createCustomCountPomodoroConfig({
        focusDurationMinutes: 40,
        shortBreakMinutes: 5,
        longBreakMinutes: 10,
        longBreakAfterFocusCount: 3,
      }),
    )).toBe(false);
  });

  it("validates bounded count and sequence rhythms", () => {
    expect(isValidPomodoroConfig(createCustomSequencePomodoroConfig([]))).toBe(false);
    expect(isValidPomodoroConfig(createCustomCountPomodoroConfig({
      focusDurationMinutes: 0,
      shortBreakMinutes: 5,
      longBreakMinutes: 10,
      longBreakAfterFocusCount: 4,
    }))).toBe(false);
    expect(isValidPomodoroConfig(createCustomSequencePomodoroConfig([
      { focusDurationMinutes: 25, breakPhase: "short_break", breakDurationMinutes: 31 },
    ]))).toBe(false);
    expect(isValidPomodoroConfig({
      ...createPresetPomodoroConfig("adaptive"),
      idleTimeoutMinutes: 0,
    })).toBe(false);
  });
});
