import { describe, expect, it } from "vitest";
import {
  loadNotesOptionalComponent,
  loadNotesSurface,
  notesOptionalComponentHasLoaded,
  notesSurfaceHasLoaded,
} from "./component-registry";

describe("Notes component registry", () => {
  it("loads only the requested active surface and caches its constructor", async () => {
    const first = loadNotesSurface("archive");
    const second = loadNotesSurface("archive");

    expect(second).toBe(first);
    await expect(first).resolves.toMatchObject({ kind: "archive" });
    expect(notesSurfaceHasLoaded("archive")).toBe(true);
    expect(notesSurfaceHasLoaded("trash")).toBe(false);
  });

  it("keeps maintenance constructors separate and single-flight", async () => {
    const first = loadNotesOptionalComponent("destination-picker");
    const second = loadNotesOptionalComponent("destination-picker");

    expect(second).toBe(first);
    await expect(first).resolves.toMatchObject({ kind: "destination-picker" });
    expect(notesOptionalComponentHasLoaded("destination-picker")).toBe(true);
    expect(notesOptionalComponentHasLoaded("project-history")).toBe(false);
    expect(notesOptionalComponentHasLoaded("confirm-dialog")).toBe(false);
  });
});
