// @vitest-environment jsdom

import { describe, expect, it } from "vitest";
import { cloneData } from "./clone";

describe("cloneData with Svelte state", () => {
  it("copies a deep state proxy into plain data", () => {
    const draft = $state({ references: [{ id: "a", range: { start: 0, end: 2 } }], target: null as string | null });
    expect(() => structuredClone(draft.references)).toThrow();

    const copy = cloneData(draft);

    expect(copy).toEqual({ references: [{ id: "a", range: { start: 0, end: 2 } }], target: null });
    expect(() => structuredClone(copy)).not.toThrow();
    copy.references[0]!.range.end = 9;
    expect(draft.references[0]?.range.end).toBe(2);
  });
});
