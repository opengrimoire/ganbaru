import { describe, expect, it } from "vitest";
import { translate } from "$lib/i18n/translator.svelte";
import { planLimitInstantLabel, planLimitWindowLine } from "./plan-limit-labels";
import type { ProviderPlanLimitWindow } from "./provider-readiness";

const NOW_MS = Date.parse("2026-11-16T12:00:00Z");
const context = { locale: "en" as const, t: translate };

function window(overrides: Partial<ProviderPlanLimitWindow>): ProviderPlanLimitWindow {
  return {
    id: "codex:primary",
    label: "5h codex",
    durationMinutes: 300,
    remainingPercent: 93,
    resetsAtMs: null,
    accountWide: true,
    ...overrides,
  };
}

describe("plan limit labels", () => {
  it("writes a window with what is left and leaves out a reset it does not know", () => {
    expect(planLimitWindowLine(context, window({}), NOW_MS)).toBe("5h 93% left");
    expect(planLimitWindowLine(context, window({ durationMinutes: 10_080, remainingPercent: 99.4 }), NOW_MS))
      .toBe("Week 99% left");
  });

  it("adds the reset in parentheses on the same line", () => {
    expect(planLimitWindowLine(context, window({ resetsAtMs: NOW_MS + 3_600_000 }), NOW_MS))
      .toMatch(/^5h 93% left \(resets .+\)$/);
  });
});

describe("plan limit instants", () => {
  it("names the weekday before the day for a reset on another day", () => {
    const label = planLimitInstantLabel(context, NOW_MS + 7 * 86_400_000, NOW_MS);
    expect(label).toMatch(/^Mon, Nov 23, /);
  });
});
