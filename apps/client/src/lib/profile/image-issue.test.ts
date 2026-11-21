import { describe, expect, it } from "vitest";
import type { ManagedImageFileIssue } from "$lib/browser-file-policy";
import { en } from "$lib/i18n/messages/en";
import { es } from "$lib/i18n/messages/es";
import { translateFromPartialCatalog, type Translate } from "$lib/i18n/translator.svelte";
import { profileImageIssueMessage } from "./image-issue";

const ISSUES = Object.keys({
  empty: true,
  "unsupported-type": true,
  "too-large": true,
  "dimensions-too-large": true,
  "too-many-pixels": true,
  "invalid-image": true,
} satisfies Record<ManagedImageFileIssue, true>) as ManagedImageFileIssue[];

function translator(catalog: unknown): Translate {
  return ((key, ...args) => translateFromPartialCatalog(catalog, key, ...args)) as Translate;
}

describe("profile picture issue messages", () => {
  it("gives every inspection issue its own English and Spanish sentence", () => {
    const english = ISSUES.map((issue) => profileImageIssueMessage(translator(en), issue));
    const spanish = ISSUES.map((issue) => profileImageIssueMessage(translator(es), issue));

    expect(new Set(english).size).toBe(ISSUES.length);
    for (const [index, message] of english.entries()) {
      expect(message).not.toMatch(/^settings\./u);
      expect(spanish[index]).not.toMatch(/^settings\./u);
      expect(spanish[index]).not.toBe(message);
    }
  });

  it("names the size limit for oversized pictures", () => {
    expect(profileImageIssueMessage(translator(en), "too-large")).toMatch(/\d+ MB/u);
  });
});
