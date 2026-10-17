import { describe, expect, it } from "vitest";
import { calendarWindowIncludesGlobalCount } from "./window-count";

describe("Calendar window global count policy", () => {
  it("counts once at boot and not during navigation or prefetch", () => {
    const requests = [true, ...Array.from({ length: 20 }, () => false)];
    expect(requests.filter(calendarWindowIncludesGlobalCount)).toHaveLength(1);
  });
});
