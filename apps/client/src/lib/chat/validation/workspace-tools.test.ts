import { describe, expect, it } from "vitest";
import {
  parseChatTerminal,
  parseChatReviewPatchPage,
  parseChatReviewSnapshot,
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

  it("accepts assignment review snapshots and rejects unknown authored path states", () => {
    const snapshot = {
      snapshotId: "snapshot-1",
      reviewRevision: "revision-1",
      source: { kind: "assignment", assignmentId: "assignment-1" },
      sourceLabel: "Assignment",
      files: [],
      totals: { files: 0, additions: 0, deletions: 0 },
      preferredPatch: null,
      freshness: "current",
      authoredPaths: "computed",
    };
    expect(parseChatReviewSnapshot(snapshot).source).toEqual({ kind: "assignment", assignmentId: "assignment-1" });
    expect(() => parseChatReviewSnapshot({ ...snapshot, authoredPaths: "partial" })).toThrow("reviewSnapshot.authoredPaths");
    expect(() => parseChatReviewSnapshot({ ...snapshot, source: { kind: "assignment" } })).toThrow();
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

  it("reads the terminal status union and requires an exit code only for an exited shell", () => {
    const terminal = (status: unknown) => ({
      id: "terminal-1",
      channelId: "channel-1",
      workingFolderId: "folder-1",
      name: "Terminal 1",
      shell: "sh",
      columns: 80,
      rows: 24,
      status,
      lastSequence: 0,
    });
    expect(parseChatTerminal(terminal({ kind: "running" })).status).toEqual({ kind: "running" });
    expect(parseChatTerminal(terminal({ kind: "stopped", exitCode: 9 })).status).toEqual({ kind: "stopped" });
    expect(parseChatTerminal(terminal({ kind: "exited", exitCode: 2 })).status).toEqual({ kind: "exited", exitCode: 2 });
    expect(() => parseChatTerminal(terminal({ kind: "exited" }))).toThrow("terminal.status.exitCode");
    expect(() => parseChatTerminal(terminal({ kind: "paused" }))).toThrow("terminal.status.kind");
  });
});
