import { describe, expect, it } from "vitest";
import {
  shouldDeferNotesCompositionInput,
  shouldLetNativeCompositionHandleKeydown,
} from "./composition";

describe("notes IME composition planning", () => {
  it("defers input while a composition session is active", () => {
    expect(shouldDeferNotesCompositionInput({
      active: true,
      eventIsComposing: false,
    })).toBe(true);
    expect(shouldDeferNotesCompositionInput({
      active: false,
      eventIsComposing: true,
    })).toBe(true);
  });

  it("allows input after composition has committed", () => {
    expect(shouldDeferNotesCompositionInput({
      active: false,
      eventIsComposing: false,
    })).toBe(false);
  });

  it("lets native IME key handling own slash, enter, tab, and process keys", () => {
    for (const key of ["/", "Enter", "Tab", "Process"]) {
      expect(shouldLetNativeCompositionHandleKeydown({
        active: key !== "Process",
        eventIsComposing: key === "Process",
        key,
      })).toBe(true);
    }
  });

  it("keeps normal rich editor key handling outside composition", () => {
    expect(shouldLetNativeCompositionHandleKeydown({
      active: false,
      eventIsComposing: false,
      key: "/",
    })).toBe(false);
  });
});
