import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { checkPin, formatPins, parsePins, sourceDigest } from "./pinned-sources.mjs";

const URL_A = "https://example.org/a@1.0.0/data.txt";
const URL_B = "https://example.org/b@2.0.0/data.txt";
const bytes = new TextEncoder().encode("upstream source\n");
const digest = sourceDigest(bytes);

describe("pinned codegen sources", () => {
  it("accept only the recorded digest for a pinned URL", () => {
    const pins = new Map([[URL_A, digest]]);
    expect(checkPin(pins, URL_A, digest, false)).toBe("pinned");
    const altered = sourceDigest(new TextEncoder().encode("upstream source, altered\n"));
    expect(() => checkPin(pins, URL_A, altered, false)).toThrow(/changed upstream/u);
    expect(() => checkPin(pins, URL_A, altered, true)).toThrow(/changed upstream/u);
  });

  it("record an unpinned URL only when the maintainer asks", () => {
    const pins = new Map([[URL_A, digest]]);
    expect(() => checkPin(pins, URL_B, digest, false)).toThrow(/is not pinned/u);
    expect(checkPin(pins, URL_B, digest, true)).toBe("new");
  });

  it("refuse non-HTTPS URLs and malformed digests", () => {
    expect(() => parsePins({ "http://example.org/a": digest })).toThrow(/HTTPS/u);
    expect(() => parsePins({ [URL_A]: digest.toUpperCase() })).toThrow(/malformed/u);
    expect(() => parsePins([])).toThrow(/object/u);
  });

  it("keep every committed generator source pinned in stable order", () => {
    const text = readFileSync(new URL("./pinned-sources.json", import.meta.url), "utf8");
    const pins = parsePins(JSON.parse(text));
    expect(formatPins(pins)).toBe(text);
    expect([...pins.keys()].some((url) => url.includes("lucide-static@"))).toBe(true);
    expect([...pins.keys()].some((url) => url.includes("/emoji/emoji-test.txt"))).toBe(true);
  });
});
