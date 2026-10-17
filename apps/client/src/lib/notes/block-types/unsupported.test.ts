import { describe, expect, it } from "vitest";
import {
  NOTES_UNSUPPORTED_CONVERSION_TARGETS,
  unsupportedBlockHasRawPayload,
  unsupportedBlockJsonText,
  unsupportedBlockPlainText,
  unsupportedBlockSummaryText,
  unsupportedBlockTypeName,
  unsupportedBlockWarnings,
} from "./unsupported";
import type { NotesUnsupportedBlockPayload } from "$lib/notes/types";

describe("notes unsupported blocks", () => {
  it("uses block_type as the visible imported kind", () => {
    expect(unsupportedBlockTypeName({ block_type: "form", source_type: "notion" })).toBe("form");
  });

  it("falls back to source_type when block_type is missing", () => {
    expect(unsupportedBlockTypeName({ source_type: "notion_template" })).toBe("notion_template");
  });

  it("builds searchable text from imported kind and warnings", () => {
    expect(
      unsupportedBlockPlainText({
        block_type: "button",
        source_type: "notion",
        warnings: ["Action content is not exposed"],
        raw: { type: "unsupported" },
      }),
    ).toBe("button notion Action content is not exposed raw payload preserved");
  });

  it("does not duplicate source type when it is the imported kind fallback", () => {
    expect(unsupportedBlockPlainText({ source_type: "notion_template" })).toBe("notion_template");
  });

  it("drops empty warning text", () => {
    expect(unsupportedBlockWarnings({ warnings: ["", "  ", "Preserved raw payload"] })).toEqual([
      "Preserved raw payload",
    ]);
  });

  it("reports raw payload availability only for object payloads", () => {
    expect(unsupportedBlockHasRawPayload({ raw: { type: "unsupported" } })).toBe(true);
    expect(
      unsupportedBlockHasRawPayload({
        raw: "not stored",
      } as unknown as NotesUnsupportedBlockPayload),
    ).toBe(false);
    expect(unsupportedBlockHasRawPayload({})).toBe(false);
  });

  it("offers explicit conversion targets", () => {
    expect(NOTES_UNSUPPORTED_CONVERSION_TARGETS).toEqual(["paragraph", "code"]);
  });

  it("builds a paragraph conversion summary without claiming hidden content is editable", () => {
    expect(
      unsupportedBlockSummaryText({
        block_type: "form",
        warnings: ["Fields are not exposed"],
        raw: { type: "unsupported", unsupported: { block_type: "form" } },
      }),
    ).toBe("Unsupported block: form\nWarning: Fields are not exposed\nRaw payload preserved");
  });

  it("builds a JSON conversion from preserved raw payload when available", () => {
    expect(
      unsupportedBlockJsonText({
        block_type: "form",
        raw: { type: "unsupported", unsupported: { block_type: "form" } },
      }),
    ).toBe('{\n  "type": "unsupported",\n  "unsupported": {\n    "block_type": "form"\n  }\n}');
  });

  it("falls back to metadata JSON when raw payload is unavailable", () => {
    expect(unsupportedBlockJsonText({ block_type: "drive" })).toBe('{\n  "block_type": "drive"\n}');
  });
});
