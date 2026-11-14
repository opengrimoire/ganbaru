import { describe, expect, it } from "vitest";

import { LocalizedError } from "$lib/i18n/user-errors";
import { notesErrorDetail, notesFailure } from "./failure";

describe("notesErrorDetail", () => {
  it("keeps raw text for copying", () => {
    expect(notesErrorDetail(new Error("disk full"))).toBe("disk full");
  });

  it("never returns empty text, so a stored failure stays visible", () => {
    expect(notesErrorDetail(new Error("  "))).not.toBe("");
    expect(notesErrorDetail(null)).not.toBe("");
  });
});

describe("notesFailure", () => {
  it("shows the fallback for backend text and keeps the raw text as detail", () => {
    expect(notesFailure(new Error("database is locked"), "The page could not be saved.")).toEqual({
      message: "The page could not be saved.",
      detail: "database is locked",
    });
  });

  it("shows localized text the error carries", () => {
    const failure = notesFailure(new LocalizedError("The layout is locked."), "The page could not be saved.");
    expect(failure.message).toBe("The layout is locked.");
  });

  it("finds localized text in the cause chain", () => {
    const error = new Error("wrapper", { cause: new LocalizedError("Choose a smaller image.") });
    expect(notesFailure(error, "The cover could not be saved.").message).toBe("Choose a smaller image.");
  });

  it("leaves the detail empty when the failure has no text", () => {
    expect(notesFailure(undefined, "Rows could not be loaded.")).toEqual({
      message: "Rows could not be loaded.",
      detail: undefined,
    });
  });

  it("uses plain string rejections only as detail", () => {
    expect(notesFailure("Notes edit conflict: stale base", "The page could not be saved.")).toEqual({
      message: "The page could not be saved.",
      detail: "Notes edit conflict: stale base",
    });
  });
});
