import { describe, expect, it } from "vitest";
import { createEquationPayload } from "$lib/notes/blocks/factory";
import { equationExpressionPlainText, equationPreviewText } from "./equation";

describe("notes equation blocks", () => {
  it("creates Notion-shaped equation payloads", () => {
    const payload = createEquationPayload("e=mc^2");

    expect(payload).toEqual({ expression: "e=mc^2" });
    expect(equationExpressionPlainText(payload)).toBe("e=mc^2");
  });

  it("uses a stable empty preview without changing the saved expression", () => {
    expect(equationPreviewText("")).toBe("e=mc^2");
    expect(equationPreviewText("  \\frac{a}{b}  ")).toBe("\\frac{a}{b}");
  });
});
