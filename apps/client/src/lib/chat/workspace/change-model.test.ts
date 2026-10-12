import { describe, expect, it } from "vitest";
import type { ChatWorkspaceChangeBatch } from "$lib/chat/contracts";
import {
  mergeWorkspaceChangeBatches,
  workspaceDirectoryCachesToDrop,
  workspaceDirectoryRefreshTargets,
  workspacePathAfterRenames,
  workspacePreviewImpact,
} from "./change-model";

function batch(overrides: Partial<ChatWorkspaceChangeBatch> = {}): ChatWorkspaceChangeBatch {
  return {
    workingFolderId: "workspace:test",
    executionEnvironmentId: null,
    generation: 1,
    relativePaths: [],
    affectedParentDirectories: [],
    renames: [],
    gitMetadataChanged: false,
    overflowed: false,
    degradedReason: null,
    ...overrides,
  };
}

describe("Chat workspace change reconciliation", () => {
  it("refreshes only loaded affected parents and preserves their depth order", () => {
    expect(workspaceDirectoryRefreshTargets(batch({
      relativePaths: ["src/new.ts", "unopened/file.ts"],
      affectedParentDirectories: ["src", "unopened"],
    }), ["src", "src/lib"])).toEqual(["src"]);
  });

  it("refreshes every loaded cache after an overflow", () => {
    expect(workspaceDirectoryRefreshTargets(batch({ overflowed: true }), ["src/lib", "src"]))
      .toEqual(["", "src", "src/lib"]);
  });

  it("drops renamed directory caches without disturbing unrelated expansions", () => {
    expect(workspaceDirectoryCachesToDrop(batch({
      renames: [{ previousRelativePath: "src/old", relativePath: "src/new" }],
    }), ["src", "src/old", "src/old/nested", "tests"])).toEqual([
      "src/old",
      "src/old/nested",
    ]);
  });

  it("moves clean selected files across renames and protects dirty drafts", () => {
    const rename = batch({
      relativePaths: ["src/old.ts", "src/new.ts"],
      renames: [{ previousRelativePath: "src/old.ts", relativePath: "src/new.ts" }],
    });
    expect(workspacePreviewImpact(rename, "src/old.ts", false)).toEqual({
      kind: "renamed",
      relativePath: "src/new.ts",
    });
    expect(workspacePreviewImpact(rename, "src/old.ts", true)).toEqual({
      kind: "deleted_after_rename",
      relativePath: "src/new.ts",
    });
  });

  it("maps selected descendants when a containing directory is renamed", () => {
    const rename = batch({
      relativePaths: ["src/old", "src/new"],
      renames: [{ previousRelativePath: "src/old", relativePath: "src/new" }],
    });
    expect(workspacePreviewImpact(rename, "src/old/nested/file.ts", false)).toEqual({
      kind: "renamed",
      relativePath: "src/new/nested/file.ts",
    });
    expect(workspacePreviewImpact(rename, "src/old/nested/file.ts", true)).toEqual({
      kind: "deleted_after_rename",
      relativePath: "src/new/nested/file.ts",
    });
    expect(workspacePathAfterRenames("src/old/nested", rename.renames))
      .toBe("src/new/nested");
    expect(workspacePathAfterRenames("src/old/nested", [
      ...rename.renames,
      { previousRelativePath: "src/new", relativePath: "src/final" },
    ])).toBe("src/final/nested");
  });

  it("refreshes selected files for direct, ancestor, and overflow changes", () => {
    expect(workspacePreviewImpact(batch({ relativePaths: ["src/file.ts"] }), "src/file.ts", false).kind)
      .toBe("refresh");
    expect(workspacePreviewImpact(batch({ relativePaths: ["src"] }), "src/file.ts", false).kind)
      .toBe("refresh");
    expect(workspacePreviewImpact(batch({ overflowed: true }), "src/file.ts", false).kind)
      .toBe("refresh");
  });

  it("coalesces queued batches and preserves invalidation signals", () => {
    const merged = mergeWorkspaceChangeBatches(
      batch({ relativePaths: ["src/a.ts"], gitMetadataChanged: true }),
      batch({
        relativePaths: ["src/b.ts"],
        affectedParentDirectories: ["src"],
        overflowed: true,
        degradedReason: "watch_error",
      }),
    );
    expect(merged.relativePaths).toEqual(["src/a.ts", "src/b.ts"]);
    expect(merged.affectedParentDirectories).toEqual(["src"]);
    expect(merged.gitMetadataChanged).toBe(true);
    expect(merged.overflowed).toBe(true);
    expect(merged.degradedReason).toBe("watch_error");
  });
});
