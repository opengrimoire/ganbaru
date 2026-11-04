import { describe, expect, it } from "vitest";
import { readJsonValue } from "./readers";

describe("readJsonValue", () => {
  it("keeps untrusted __proto__ keys as data without changing object prototypes", () => {
    const parsed: unknown = JSON.parse('{"nested": {"__proto__": {"isAdmin": true}}, "__proto__": []}');

    const value = readJsonValue(parsed, "payload");

    if (value === null || typeof value !== "object" || Array.isArray(value)) throw new Error("expected an object");
    expect(Object.getPrototypeOf(value)).toBe(Object.prototype);
    expect(Object.getOwnPropertyDescriptor(value, "__proto__")?.value).toEqual([]);
    const nested = value.nested;
    if (nested === null || typeof nested !== "object" || Array.isArray(nested)) throw new Error("expected an object");
    expect(Object.getPrototypeOf(nested)).toBe(Object.prototype);
    expect("isAdmin" in nested).toBe(false);
    expect(Object.getOwnPropertyDescriptor(nested, "__proto__")?.value).toEqual({ isAdmin: true });
  });

  it("rejects values that are not JSON", () => {
    expect(() => readJsonValue({ value: Number.NaN }, "payload")).toThrow("payload.value is not valid JSON");
    expect(() => readJsonValue({ value: () => undefined }, "payload")).toThrow();
  });
});
