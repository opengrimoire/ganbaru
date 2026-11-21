import { describe, expect, it } from "vitest";
import { buildDraftPrompt, type DraftPromptText } from "./draft-prompt";

describe("draft prompt", () => {
  const text: DraftPromptText = {
    instruction: "Write it.",
    changedFiles: "Files",
    omittedFiles: (count) => `Not listed: ${count}`,
    subjects: "Subjects",
    patch: "Patch",
    patchTruncated: "Cut short.",
    patchOmitted: "Too large.",
  };

  it("states every cut and fences the patch past its longest backtick run", () => {
    const prompt = buildDraftPrompt({
      stagedPaths: ["a.ts"],
      patch: "+const fence = \"````\";\n",
      patchTruncated: true,
      omittedFiles: 2,
      recentSubjects: ["feat: one"],
    }, text);
    expect(prompt).toBe([
      "Write it.",
      "Files\n- a.ts\nNot listed: 2",
      "Subjects\n- feat: one",
      "Patch\n`````diff\n+const fence = \"````\";\n`````\nCut short.",
    ].join("\n\n"));
  });

  it("names a patch too large to include and leaves out empty sections", () => {
    expect(buildDraftPrompt({
      stagedPaths: ["a.ts"],
      patch: "",
      patchTruncated: true,
      omittedFiles: 0,
      recentSubjects: [],
    }, text)).toBe("Write it.\n\nFiles\n- a.ts\n\nToo large.");
  });
});
