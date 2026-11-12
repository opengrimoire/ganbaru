import { describe, expect, it } from "vitest";
import {
  hasTimelineReadObservers,
  observeTimelineSinceReads,
  reportTimelineSinceRead,
  type TimelineSinceReadSample,
} from "./read-observer";

describe("timeline read observer", () => {
  it("reports the UTF-8 size of each read only while observed", () => {
    const samples: TimelineSinceReadSample[] = [];
    expect(hasTimelineReadObservers()).toBe(false);
    reportTimelineSinceRead({ items: ["ignored"] }, 1);

    const stop = observeTimelineSinceReads((sample) => samples.push(sample));
    expect(hasTimelineReadObservers()).toBe(true);
    reportTimelineSinceRead({ text: "añ" }, 2);
    stop();
    reportTimelineSinceRead({ text: "after" }, 3);

    expect(samples).toEqual([{ bytes: JSON.stringify({ text: "añ" }).length + 1, itemCount: 2 }]);
    expect(hasTimelineReadObservers()).toBe(false);
  });

  it("reports to every observer and stops each one independently", () => {
    const first: number[] = [];
    const second: number[] = [];
    const stopFirst = observeTimelineSinceReads((sample) => first.push(sample.itemCount));
    const stopSecond = observeTimelineSinceReads((sample) => second.push(sample.itemCount));

    reportTimelineSinceRead({}, 1);
    stopFirst();
    reportTimelineSinceRead({}, 2);
    stopSecond();

    expect(first).toEqual([1]);
    expect(second).toEqual([1, 2]);
  });
});
