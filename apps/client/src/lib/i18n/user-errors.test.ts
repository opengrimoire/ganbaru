import { describe, expect, it } from "vitest";

import { LocalizedError, localizedErrorText, technicalErrorDetail, userErrorMessage } from "./user-errors";

describe("userErrorMessage", () => {
  it("shows localized text carried by the failure", () => {
    expect(userErrorMessage(new LocalizedError("No se pudo guardar"), "Fallback")).toBe("No se pudo guardar");
  });

  it("finds localized text through a bounded cause chain", () => {
    const wrapped = new Error("outer", { cause: new Error("middle", { cause: new LocalizedError("Localizado") }) });
    expect(localizedErrorText(wrapped)).toBe("Localizado");
  });

  it("stops following causes after the depth limit", () => {
    let error: Error = new LocalizedError("Deep");
    for (let index = 0; index < 10; index += 1) error = new Error("wrap", { cause: error });
    expect(localizedErrorText(error)).toBeNull();
  });

  it("never shows raw backend or plain error text", () => {
    expect(userErrorMessage(new Error("database is locked"), "Could not save")).toBe("Could not save");
    expect(userErrorMessage("Permission denied (os error 13)", "Could not save")).toBe("Could not save");
    expect(userErrorMessage({ code: "validation", message: "Name too long" }, "Could not save")).toBe("Could not save");
    expect(userErrorMessage(null, "Could not save")).toBe("Could not save");
  });

  it("ignores blank localized text", () => {
    expect(userErrorMessage(new LocalizedError("  "), "Fallback")).toBe("Fallback");
  });
});

describe("technicalErrorDetail", () => {
  it("reads raw text from errors, strings, and direct or nested records", () => {
    expect(technicalErrorDetail(new Error(" boom "))).toBe("boom");
    expect(technicalErrorDetail("stderr line")).toBe("stderr line");
    expect(technicalErrorDetail({ message: "direct" })).toBe("direct");
    expect(technicalErrorDetail({ error: { detail: "nested" } })).toBe("nested");
    expect(technicalErrorDetail({ code: "x" })).toBeNull();
    expect(technicalErrorDetail(undefined)).toBeNull();
  });

  it("bounds long detail", () => {
    const detail = technicalErrorDetail("x".repeat(5_000));
    expect(detail?.length).toBe(2_001);
    expect(detail?.endsWith("…")).toBe(true);
  });
});
