import { describe, expect, it } from "vitest";
import type { RateLimitStatusEvent } from "$lib/chat/contracts";
import { composerMeterRead } from "./composer-meter";

function rateLimit(usedPercent: number): RateLimitStatusEvent {
  return {
    limited: false,
    resetsAt: null,
    detail: null,
    providerData: {
      schemaVersion: 1,
      value: {
        rateLimitsByLimitId: {
          codex: { limitId: "codex", limitName: null, primary: { usedPercent, windowDurationMins: 300, resetsAt: null } },
        },
      },
    },
  };
}

describe("composerMeterRead", () => {
  it("shows nothing before the session reports usage or limits", () => {
    expect(composerMeterRead(null, null)).toEqual({ meter: null, windows: [] });
  });

  it("reads the context meter and the limit windows together", () => {
    const read = composerMeterRead({ contextTokens: 90, contextLimit: 100 }, rateLimit(40));

    expect(read.meter).toMatchObject({ usedTokens: 90, maximumTokens: 100, ratio: 0.9, warning: true });
    expect(read.windows).toEqual([
      { id: "codex:primary", label: "5h codex", usedPercent: 40, windowDurationMinutes: 300, resetsAtSeconds: null, accountWide: true },
    ]);
  });

  it("keeps the limits when the provider reported no context count", () => {
    const read = composerMeterRead({ contextTokens: null, contextLimit: 100 }, rateLimit(120));

    expect(read.meter).toBeNull();
    expect(read.windows[0]?.usedPercent).toBe(100);
  });

  it("shows usage without a ratio when the context limit is unknown", () => {
    expect(composerMeterRead({ contextTokens: 40, contextLimit: null }, null).meter)
      .toMatchObject({ usedTokens: 40, maximumTokens: null, ratio: null, warning: false });
  });
});
