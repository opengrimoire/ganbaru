import { describe, expect, it } from "vitest";
import type { ChangedFileSummary } from "$lib/chat/contracts";
import {
  isWorkspaceRelativeChangedFilePath,
  normalizeChangedFileSummaries,
} from "./changed-files";

function changed(relativePath: string, additions = 0): ChangedFileSummary {
  return {
    relativePath,
    previousRelativePath: null,
    additions,
    deletions: 0,
    binary: false,
    status: additions > 0 ? "added" : "modified",
  };
}

describe("changed file summaries", () => {
  it("drops absolute provider paths and retains the usable relative summary", () => {
    const files = normalizeChangedFileSummaries([
      changed("/home/user/workspace/hello.py"),
      changed("hello.py", 10),
    ]);

    expect(files).toEqual([changed("hello.py", 10)]);
  });

  it("uses the last bounded summary when providers repeat a path", () => {
    const files = normalizeChangedFileSummaries([
      changed("src/app.ts", 0),
      { ...changed("src/app.ts", 4), previousRelativePath: "src/old-app.ts", status: "renamed" },
    ]);

    expect(files).toEqual([
      { ...changed("src/app.ts", 4), previousRelativePath: "src/old-app.ts", status: "renamed" },
    ]);
  });

  it("rejects traversal, Windows absolute paths, separators, and control characters", () => {
    expect(isWorkspaceRelativeChangedFilePath("src/app.ts")).toBe(true);
    expect(isWorkspaceRelativeChangedFilePath("../app.ts")).toBe(false);
    expect(isWorkspaceRelativeChangedFilePath("C:\\workspace\\app.ts")).toBe(false);
    expect(isWorkspaceRelativeChangedFilePath("src\\app.ts")).toBe(false);
    expect(isWorkspaceRelativeChangedFilePath("src/\u0000app.ts")).toBe(false);
  });
});
