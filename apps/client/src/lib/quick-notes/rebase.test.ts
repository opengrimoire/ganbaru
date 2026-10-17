import { describe, expect, it } from "vitest";
import { rebaseQuickNoteEdits, type QuickNoteEditableFields } from "./rebase";

function fields(overrides: Partial<QuickNoteEditableFields> = {}): QuickNoteEditableFields {
  return {
    title: "Plan",
    runs: [{ content: "Start", bold: false, italic: false, underline: false }],
    color: 1,
    tagId: null,
    pinned: false,
    ...overrides,
  };
}

function text(content: string, bold = false) {
  return [{ content, bold, italic: false, underline: false }];
}

describe("rebaseQuickNoteEdits", () => {
  it("keeps local edits and takes remote edits to other fields", () => {
    const result = rebaseQuickNoteEdits(
      fields(),
      fields({ runs: text("Start here") }),
      fields({ title: "Remote plan", pinned: true }),
    );
    expect(result).toEqual({
      kind: "rebased",
      fields: fields({ title: "Remote plan", pinned: true, runs: text("Start here") }),
    });
  });

  it("takes the canonical value when the editor changed nothing", () => {
    const result = rebaseQuickNoteEdits(fields(), fields(), fields({ color: 5, tagId: "tag-1" }));
    expect(result).toEqual({ kind: "rebased", fields: fields({ color: 5, tagId: "tag-1" }) });
  });

  it("accepts both sides making the same change", () => {
    const result = rebaseQuickNoteEdits(fields(), fields({ title: "Same" }), fields({ title: "Same" }));
    expect(result).toEqual({ kind: "rebased", fields: fields({ title: "Same" }) });
  });

  it("reports fields both sides changed differently", () => {
    const result = rebaseQuickNoteEdits(
      fields(),
      fields({ title: "Mine", runs: text("Local"), color: 3 }),
      fields({ title: "Theirs", runs: text("Remote"), pinned: true }),
    );
    expect(result).toEqual({ kind: "conflict", fields: ["title", "runs"] });
  });

  it("compares bodies after normalizing run boundaries", () => {
    const split = [
      { content: "Sta", bold: false, italic: false, underline: false },
      { content: "rt", bold: false, italic: false, underline: false },
    ];
    const result = rebaseQuickNoteEdits(fields(), fields({ runs: split }), fields({ runs: text("Remote") }));
    expect(result).toEqual({ kind: "rebased", fields: fields({ runs: text("Remote") }) });
  });

  it("treats a formatting change as a body change", () => {
    const result = rebaseQuickNoteEdits(fields(), fields({ runs: text("Start", true) }), fields({ runs: text("Remote") }));
    expect(result).toEqual({ kind: "conflict", fields: ["runs"] });
  });
});
