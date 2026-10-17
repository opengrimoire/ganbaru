import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { parseProjectCascadePreview, previewProjectDependencyCascade } from "./project-cascade";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("$lib/api/db", () => ({ ensureDbUrl: vi.fn(async () => "sqlite:test") }));

function preview() {
  return { projectId: "project", digest: "1".repeat(64), conflicts: [], items: [{
    taskId: "unloaded", title: "Unloaded task", shiftDays: 2,
    originalStartDate: "2026-06-11", originalDueDate: null, originalTargetEndDate: "2026-06-13",
    originalRangeStart: "2026-06-11", originalRangeEnd: "2026-06-13",
    nextStartDate: "2026-06-13", nextDueDate: null, nextTargetEndDate: "2026-06-15",
    nextRangeStart: "2026-06-13", nextRangeEnd: "2026-06-15",
    reasons: [{ dependencyId: "edge", blockingTaskId: "unloaded-predecessor", blockingTitle: "Predecessor", requiredStartDate: "2026-06-13" }],
  }] };
}

describe("native Project cascade preview boundary", () => {
  beforeEach(() => vi.clearAllMocks());

  it("requests the canonical project without sending filtered frontend tasks", async () => {
    vi.mocked(invoke).mockResolvedValue(preview());
    expect(await previewProjectDependencyCascade("project")).toEqual(preview());
    expect(invoke).toHaveBeenCalledExactlyOnceWith("projects_preview_dependency_cascade", { dbUrl: "sqlite:test", projectId: "project" });
  });

  it.each([
    (value: ReturnType<typeof preview>) => ({ ...value, projectId: "other" }),
    (value: ReturnType<typeof preview>) => ({ ...value, digest: "invalid" }),
    (value: ReturnType<typeof preview>) => ({ ...value, items: [value.items[0], value.items[0]] }),
    (value: ReturnType<typeof preview>) => ({ ...value, items: [{ ...value.items[0], nextStartDate: "2026-02-30" }] }),
    (value: ReturnType<typeof preview>) => ({ ...value, items: [{ ...value.items[0], shiftDays: Infinity }] }),
    (value: ReturnType<typeof preview>) => ({ ...value, conflicts: [{ dependencyId: "edge", taskId: "task", title: "Task", reason: "guess" }] }),
  ])("rejects malformed preview fields before they can be reviewed", (corrupt) => {
    expect(() => parseProjectCascadePreview(corrupt(preview()), "project")).toThrow();
  });
});
