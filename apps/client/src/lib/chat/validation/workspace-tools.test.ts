import { describe, expect, it } from "vitest";
import {
  parseChatReviewPatchPage,
  parseChatRestorePreview,
  parseChatWorkspaceChangeBatch,
  parseProjectWorkingFolderDirectory,
} from "./workspace-tools";

describe("Chat workspace response validation", () => {
  it("rejects directory responses above the Rust entry ceiling before parsing entries", () => {
    expect(() =>
      parseProjectWorkingFolderDirectory({
        relativePath: "",
        entries: Array.from({ length: 5_001 }, () => null),
        truncated: true,
      }),
    ).toThrow("workspaceDirectory.entries must contain at most 5000 entries");
  });

  it("rejects observer batches above the coalescing ceiling", () => {
    expect(() =>
      parseChatWorkspaceChangeBatch({
        workingFolderId: "folder-1",
        executionEnvironmentId: null,
        generation: 1,
        relativePaths: Array.from({ length: 513 }, (_, index) => `src/${index}.ts`),
        affectedParentDirectories: [],
        renames: [],
        gitMetadataChanged: false,
        overflowed: true,
        degradedReason: null,
      }),
    ).toThrow("workspaceChangeBatch.relativePaths must contain at most 512 entries");
  });

  it("rejects patch pages above the backend response ceiling", () => {
    expect(() =>
      parseChatReviewPatchPage({
        patches: Array.from({ length: 65 }, () => null),
        continuationCursor: null,
      }),
    ).toThrow("reviewPatchPage.patches must contain at most 64 entries");
  });

  it("accepts typed restore warnings and rejects untyped warning text", () => {
    const preview = {
      previewId: "preview-1",
      checkpointId: "checkpoint-1",
      expiresAt: "2026-11-12T10:15:00.000Z",
      files: [],
      stagedChanges: true,
      providerRollback: "unsupported",
      warnings: ["staging_restored", "provider_fork_required"],
    };
    expect(parseChatRestorePreview(preview).warnings).toEqual(["staging_restored", "provider_fork_required"]);
    expect(() =>
      parseChatRestorePreview({ ...preview, warnings: ["The real staging state will be restored to the checkpoint."] }),
    ).toThrow("restorePreview.warnings");
  });
});
