import { describe, expect, it } from "vitest";
import type { ChatChangedFileRead, ChatRestorePreviewRead } from "$lib/chat/contracts";
import { buildRestoreDialogModel } from "./restore-dialog-model";

const EXPIRES_AT = "2026-11-12T10:15:00Z";
const EXPIRES_AT_MS = Date.parse(EXPIRES_AT);

function file(relativePath: string, overrides: Partial<ChatChangedFileRead> = {}): ChatChangedFileRead {
  return {
    relativePath,
    previousRelativePath: null,
    status: "modified",
    additions: 3,
    deletions: 1,
    binary: false,
    providerReported: false,
    gitObserved: true,
    ...overrides,
  };
}

function preview(overrides: Partial<ChatRestorePreviewRead> = {}): ChatRestorePreviewRead {
  return {
    previewId: "preview-1",
    checkpointId: "checkpoint-1",
    expiresAt: EXPIRES_AT,
    files: [file("src/main.ts")],
    stagedChanges: false,
    providerRollback: "supported",
    warnings: [],
    ...overrides,
  };
}

describe("checkpoint restore dialog model", () => {
  it("describes each staging and provider history combination", () => {
    const now = EXPIRES_AT_MS - 60_000;
    expect(buildRestoreDialogModel(preview(), now).facts).toEqual([
      { kind: "staging_unchanged", tone: "info" },
      { kind: "provider_rollback", tone: "info" },
    ]);
    expect(buildRestoreDialogModel(preview({ stagedChanges: true, warnings: ["staging_restored"] }), now).facts).toEqual([
      { kind: "staging_restored", tone: "warning" },
      { kind: "provider_rollback", tone: "info" },
    ]);
    expect(buildRestoreDialogModel(preview({ providerRollback: "unsupported", warnings: ["provider_fork_required"] }), now).facts).toEqual([
      { kind: "staging_unchanged", tone: "info" },
      { kind: "provider_fork", tone: "warning" },
    ]);
    expect(buildRestoreDialogModel(preview({
      stagedChanges: true,
      providerRollback: "unsupported",
      warnings: ["staging_restored", "provider_fork_required"],
    }), now).facts.map((fact) => fact.tone)).toEqual(["warning", "warning"]);
  });

  it("keeps a warning when the preview flags disagree with it", () => {
    const model = buildRestoreDialogModel(
      preview({ warnings: ["staging_restored", "provider_fork_required"] }),
      EXPIRES_AT_MS - 60_000,
    );
    expect(model.facts.map((fact) => fact.kind)).toEqual(["staging_restored", "provider_fork"]);
  });

  it("counts minutes up to expiry and marks an expired preview", () => {
    expect(buildRestoreDialogModel(preview(), EXPIRES_AT_MS - 15 * 60_000)).toMatchObject({ remainingMinutes: 15, expired: false });
    expect(buildRestoreDialogModel(preview(), EXPIRES_AT_MS - 61_000)).toMatchObject({ remainingMinutes: 2, expired: false });
    expect(buildRestoreDialogModel(preview(), EXPIRES_AT_MS - 1)).toMatchObject({ remainingMinutes: 1, expired: false });
    expect(buildRestoreDialogModel(preview(), EXPIRES_AT_MS)).toMatchObject({ remainingMinutes: 0, expired: true });
    expect(buildRestoreDialogModel(preview(), EXPIRES_AT_MS + 120_000)).toMatchObject({ remainingMinutes: 0, expired: true });
  });

  it("lists files with totals and handles an empty file list", () => {
    const model = buildRestoreDialogModel(preview({
      files: [
        file("src/main.ts"),
        file("assets/logo.png", { binary: true, additions: null, deletions: null }),
        file("src/new.ts", { previousRelativePath: "src/old.ts", status: "renamed", additions: 2, deletions: 2 }),
      ],
    }), EXPIRES_AT_MS - 60_000);
    expect(model.files.map((row) => row.relativePath)).toEqual(["src/main.ts", "assets/logo.png", "src/new.ts"]);
    expect(model.files[2]).toMatchObject({ previousRelativePath: "src/old.ts", status: "renamed" });
    expect(model).toMatchObject({ additions: 5, deletions: 3 });

    const empty = buildRestoreDialogModel(preview({ files: [] }), EXPIRES_AT_MS - 60_000);
    expect(empty).toMatchObject({ files: [], additions: 0, deletions: 0 });
  });
});
