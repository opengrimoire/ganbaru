import { describe, it, expect } from "vitest";
import { formatElapsed, pickTicks, samplesToCsv, SAMPLE_CAP, SAMPLE_INTERVAL_MS } from "./memory-samples";
import type { MemorySample } from "./memory-samples";

describe("formatElapsed", () => {
  it("renders sub-minute durations with the seconds suffix only", () => {
    expect(formatElapsed(0)).toBe("0s");
    expect(formatElapsed(500)).toBe("1s");
    expect(formatElapsed(5_000)).toBe("5s");
    expect(formatElapsed(59_000)).toBe("59s");
  });

  it("renders sub-hour durations with minute and second components", () => {
    expect(formatElapsed(60_000)).toBe("1m");
    expect(formatElapsed(65_000)).toBe("1m 5s");
    expect(formatElapsed(125_000)).toBe("2m 5s");
    expect(formatElapsed(59 * 60_000 + 30_000)).toBe("59m 30s");
  });

  it("renders multi-hour durations with all non-zero components", () => {
    expect(formatElapsed(60 * 60_000)).toBe("1h");
    expect(formatElapsed(60 * 60_000 + 5_000)).toBe("1h 5s");
    expect(formatElapsed(60 * 60_000 + 30 * 60_000)).toBe("1h 30m");
    expect(formatElapsed(2 * 60 * 60_000 + 5 * 60_000 + 9_000)).toBe("2h 5m 9s");
  });

  it("clamps negative inputs to zero", () => {
    expect(formatElapsed(-1)).toBe("0s");
  });
});

describe("pickTicks", () => {
  it("returns three ticks at narrow widths", () => {
    const ticks = pickTicks(0, 10_000, 180);
    expect(ticks).toHaveLength(3);
    expect(ticks[0]).toBe(0);
    expect(ticks[2]).toBe(10_000);
  });

  it("returns four ticks at wider widths", () => {
    const ticks = pickTicks(0, 30_000, 260);
    expect(ticks).toHaveLength(4);
    expect(ticks[0]).toBe(0);
    expect(ticks[3]).toBe(30_000);
  });

  it("collapses to a single tick when domain has no span", () => {
    expect(pickTicks(5_000, 5_000, 260)).toEqual([5_000]);
  });
});

describe("samplesToCsv", () => {
  function sample(t: number, total: number, procs: { name: string; mb: number }[]): MemorySample {
    return { t, totalMb: total, processes: procs };
  }

  it("returns an empty string for an empty buffer", () => {
    expect(samplesToCsv([])).toBe("");
  });

  it("emits a header plus one row per sample with sorted process columns", () => {
    const csv = samplesToCsv([
      sample(0, 250.5, [
        { name: "Backend", mb: 80.1 },
        { name: "Frontend", mb: 150.2 },
      ]),
      sample(5_000, 260.0, [
        { name: "Backend", mb: 82.0 },
        { name: "Frontend", mb: 158.0 },
      ]),
    ]);
    const [header, row1, row2] = csv.split("\n");
    expect(header).toBe("t_ms,total_mb,backend,frontend");
    expect(row1).toBe("0,250.50,80.10,150.20");
    expect(row2).toBe("5000,260.00,82.00,158.00");
  });

  it("names the total column after the memory metric when provided", () => {
    const csv = samplesToCsv([
      sample(0, 250.5, [{ name: "Backend", mb: 250.5 }]),
    ], "working_set");
    const [header] = csv.split("\n");

    expect(header).toBe("t_ms,total_working_set_mb,backend");
  });

  it("leaves a missing process cell empty when only some samples include it", () => {
    const csv = samplesToCsv([
      sample(0, 100, [{ name: "Backend", mb: 100 }]),
      sample(5_000, 260, [
        { name: "Backend", mb: 80 },
        { name: "Network", mb: 180 },
      ]),
    ]);
    const [header, row1, row2] = csv.split("\n");
    expect(header).toBe("t_ms,total_mb,backend,network");
    expect(row1).toBe("0,100.00,100.00,");
    expect(row2).toBe("5000,260.00,80.00,180.00");
  });
});

describe("constants", () => {
  it("SAMPLE_CAP times SAMPLE_INTERVAL_MS spans one hour", () => {
    expect(SAMPLE_CAP * SAMPLE_INTERVAL_MS).toBe(60 * 60 * 1000);
  });
});
