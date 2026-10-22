import { describe, expect, it } from "vitest";
import { cloneData } from "./clone";

describe("cloneData", () => {
  it("copies data held behind proxies, which structuredClone rejects", () => {
    const source = new Proxy(
      { references: new Proxy([{ id: "a", range: { start: 0, end: 2 } }], {}), target: null },
      {},
    );
    expect(() => structuredClone(source)).toThrow();

    const copy = cloneData(source);

    expect(copy).toEqual({ references: [{ id: "a", range: { start: 0, end: 2 } }], target: null });
    expect(Array.isArray(copy.references)).toBe(true);
    expect(copy.references).not.toBe(source.references);
    expect(copy.references[0]?.range).not.toBe(source.references[0]?.range);
  });

  it("leaves the source unchanged when the copy is edited", () => {
    const source = { rich_text: [{ plain_text: "hello", annotations: { bold: false } }] };

    const copy = cloneData(source);
    copy.rich_text[0]!.annotations.bold = true;
    copy.rich_text.push({ plain_text: "added", annotations: { bold: true } });

    expect(source).toEqual({ rich_text: [{ plain_text: "hello", annotations: { bold: false } }] });
  });

  it("copies a parsed __proto__ key as data instead of replacing the prototype", () => {
    const source = JSON.parse('{"__proto__": {"polluted": true}, "kept": 1}') as Record<string, unknown>;

    const copy = cloneData(source);

    expect(Object.getPrototypeOf(copy)).toBe(Object.prototype);
    expect("polluted" in copy).toBe(false);
    expect(Object.hasOwn(copy, "__proto__")).toBe(true);
    expect(Object.getOwnPropertyDescriptor(copy, "__proto__")?.value).toEqual({ polluted: true });
    expect(copy.kept).toBe(1);
  });

  it("keeps primitives, undefined fields, and null prototypes", () => {
    const bare = Object.assign(Object.create(null) as Record<string, unknown>, { key: "value" });

    expect(cloneData(4)).toBe(4);
    expect(cloneData(null)).toBeNull();
    expect(cloneData({ optional: undefined })).toHaveProperty("optional", undefined);
    expect(Object.getPrototypeOf(cloneData(bare))).toBeNull();
    expect(cloneData(bare)).toEqual(bare);
  });

  it("copies dates as dates rather than strings", () => {
    const at = new Date("2026-10-14T12:00:00Z");

    const copy = cloneData({ at });

    expect(copy.at).toBeInstanceOf(Date);
    expect(copy.at).not.toBe(at);
    expect(copy.at.getTime()).toBe(at.getTime());
  });
});
