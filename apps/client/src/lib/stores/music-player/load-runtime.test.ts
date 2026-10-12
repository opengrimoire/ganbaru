import { describe, expect, it } from "vitest";
import { MusicLoadRuntime } from "./load-runtime";

describe("Music load runtime", () => {
  it("invalidates every older backend load with one shared generation", () => {
    const runtime = new MusicLoadRuntime();
    const firstLocal = runtime.begin();
    const secondLocal = runtime.begin();
    const youtube = runtime.begin();

    expect(runtime.isCurrent(firstLocal)).toBe(false);
    expect(runtime.isCurrent(secondLocal)).toBe(false);
    expect(runtime.isCurrent(youtube)).toBe(true);
    expect(runtime.current()).toBe(youtube);
  });
});
