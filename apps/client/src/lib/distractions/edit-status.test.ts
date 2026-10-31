import { describe, expect, it } from "vitest";
import { en } from "$lib/i18n/messages/en";
import { es } from "$lib/i18n/messages/es";
import { distractionsEditProblem, distractionsEditProblemMessageKey } from "./edit-status";
import { DEFAULT_DISTRACTIONS_CONFIG, DISTRACTIONS_RULE_REFUSALS } from "./rules";

function catalogText(catalog: unknown, key: string): unknown {
  return key.split(".").reduce<unknown>(
    (node, part) => (node && typeof node === "object" ? (node as Record<string, unknown>)[part] : undefined),
    catalog,
  );
}

describe("distractionsEditProblem", () => {
  it("clears the problem when an edit saves", () => {
    expect(distractionsEditProblem({ status: "saved", config: DEFAULT_DISTRACTIONS_CONFIG })).toBeNull();
  });

  it("keeps the refusal reason", () => {
    expect(distractionsEditProblem({ status: "refused", reason: "too-large" })).toEqual({
      kind: "refused",
      reason: "too-large",
    });
  });
});

describe("distractionsEditProblemMessageKey", () => {
  it("explains specific refusals with their own message", () => {
    expect(distractionsEditProblemMessageKey({ kind: "refused", reason: "missing" })).toBe(
      "settings.distractions.editStatus.missing",
    );
    expect(distractionsEditProblemMessageKey({ kind: "refused", reason: "protected-source" })).toBe(
      "settings.distractions.editStatus.protectedSource",
    );
    expect(distractionsEditProblemMessageKey({ kind: "refused", reason: "duplicate-source" })).toBe(
      "settings.distractions.editStatus.duplicate",
    );
    expect(distractionsEditProblemMessageKey({ kind: "refused", reason: "too-large" })).toBe(
      "settings.distractions.editStatus.tooLarge",
    );
  });

  it("groups validation refusals under one message", () => {
    for (const reason of ["invalid-hosts", "invalid-minutes", "invalid-target"] as const) {
      expect(distractionsEditProblemMessageKey({ kind: "refused", reason })).toBe(
        "settings.distractions.editStatus.invalid",
      );
    }
  });

  it("reports a failed save separately from a refusal", () => {
    expect(distractionsEditProblemMessageKey({ kind: "failed" })).toBe("settings.distractions.editStatus.failed");
  });

  it("has English and Spanish text for every refusal and for failures", () => {
    const problems = [
      ...DISTRACTIONS_RULE_REFUSALS.map((reason) => ({ kind: "refused" as const, reason })),
      { kind: "failed" as const },
    ];
    for (const problem of problems) {
      const key = distractionsEditProblemMessageKey(problem);
      for (const catalog of [en, es]) {
        const text = catalogText(catalog, key);
        expect(typeof text === "string" && text.length > 0, `${key} needs text`).toBe(true);
      }
    }
  });
});
