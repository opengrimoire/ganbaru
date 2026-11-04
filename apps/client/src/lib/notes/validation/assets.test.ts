import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MockInstance } from "vitest";
import { createNotesDesignCover } from "$lib/notes/pages/cover";
import { parseNullableNotesIcon, parseNullablePageCover, readTolerantNotesCover, readTolerantNotesIcon } from "./assets";

const SHA256 = "a".repeat(64);
const ICON_ASSET = `notes/page-icons/${SHA256}.png`;
const PROJECT_ICON_ASSET = `project-icons/${SHA256}.png`;
const COVER_ASSET = `notes/page-covers/${SHA256}.png`;
const HTTP_ICON = { type: "external", external: { url: "http://example.com/icon.png" } };
const HTTP_COVER = { type: "external", external: { url: "http://example.com/cover.png" } };

const VALID_ICONS = [
  { type: "emoji", emoji: "📌" },
  { type: "custom_emoji", custom_emoji: { id: "local", name: "Local", url: `ganbaru-asset:${PROJECT_ICON_ASSET}`, ganbaru_asset_path: PROJECT_ICON_ASSET } },
  { type: "icon", icon: { name: "book", color: "blue" } },
  { type: "external", external: { url: "https://example.com/icon.png" } },
  { type: "file", file: { url: `ganbaru-asset:${ICON_ASSET}`, content_type: "image/png", byte_size: 42, sha256: SHA256, ganbaru_asset_path: ICON_ASSET } },
];

const VALID_COVERS = [
  createNotesDesignCover("grid", 0),
  { type: "external", external: { url: "https://example.com/cover.png" } },
  { type: "file", file: { url: `ganbaru-asset:${COVER_ASSET}`, content_type: "image/png", byte_size: 42, sha256: SHA256, ganbaru_asset_path: COVER_ASSET } },
];

describe("tolerant synced appearance readers", () => {
  let warn: MockInstance<typeof console.warn>;

  beforeEach(() => {
    warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
  });

  afterEach(() => {
    warn.mockRestore();
  });

  it("keeps every valid icon type and a stored null without a diagnostic", () => {
    for (const icon of VALID_ICONS) {
      expect(readTolerantNotesIcon(icon, "page.icon")).toEqual(icon);
    }
    expect(readTolerantNotesIcon(null, "page.icon")).toBeNull();
    expect(warn).not.toHaveBeenCalled();
  });

  it("keeps every valid cover type and a stored null without a diagnostic", () => {
    for (const cover of VALID_COVERS) {
      expect(readTolerantNotesCover(cover, "page.cover")).toEqual(cover);
    }
    expect(readTolerantNotesCover(null, "page.cover")).toBeNull();
    expect(warn).not.toHaveBeenCalled();
  });

  it.each([
    ["a refused URL", HTTP_ICON],
    ["an unknown type", { type: "hologram" }],
    ["a type without its payload", { type: "file" }],
    ["text left unparsed", "{{"],
    ["a scalar", 7],
    ["a missing field", undefined],
  ])("reads %s as no icon and names the field", (_name, value) => {
    expect(readTolerantNotesIcon(value, "database.icon")).toBeNull();
    expect(warn).toHaveBeenCalledExactlyOnceWith("Ignoring unusable synced database.icon", expect.any(Error));
  });

  it.each([
    ["a refused URL", HTTP_COVER],
    ["an unknown design", { type: "design", design: { pattern: "unknown", color: 0 } }],
    ["a focal point outside the image", { ...VALID_COVERS[1], focal_point: { x: 2, y: 0 } }],
    ["text left unparsed", "{{"],
  ])("reads %s as no cover and names the field", (_name, value) => {
    expect(readTolerantNotesCover(value, "page.cover")).toBeNull();
    expect(warn).toHaveBeenCalledExactlyOnceWith("Ignoring unusable synced page.cover", expect.any(Error));
  });

  it("leaves the attacker's URL out of the diagnostic", () => {
    readTolerantNotesIcon(HTTP_ICON, "page.icon");
    expect(JSON.stringify(warn.mock.calls.map(([message, error]) => [message, String(error)]))).not.toContain("example.com");
  });

  it("keeps the strict parsers throwing for input being written", () => {
    expect(() => parseNullableNotesIcon(HTTP_ICON, "callout clipboard icon")).toThrow(
      "callout clipboard icon.external.url must be a supported HTTPS image URL",
    );
    expect(() => parseNullablePageCover(HTTP_COVER, "cover")).toThrow(
      "cover.external.url must be a supported HTTPS image URL",
    );
  });
});
