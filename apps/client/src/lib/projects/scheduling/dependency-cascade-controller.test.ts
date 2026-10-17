import { describe, expect, it, vi } from "vitest";
import type { ProjectDependencyCascadePreview } from "$lib/api/project-cascade";
import { ProjectDependencyCascadeController } from "./dependency-cascade-controller.svelte";

function proposal(projectId = "project", digest = "1".repeat(64)): ProjectDependencyCascadePreview {
  return { projectId, digest, conflicts: [], items: [{
    taskId: "hidden", title: "Hidden", shiftDays: 1,
    originalStartDate: null, originalDueDate: "2026-06-10", originalTargetEndDate: null,
    originalRangeStart: "2026-06-10", originalRangeEnd: "2026-06-10",
    nextStartDate: null, nextDueDate: "2026-06-11", nextTargetEndDate: null,
    nextRangeStart: "2026-06-11", nextRangeEnd: "2026-06-11", reasons: [],
  }] };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => { resolve = next; });
  return { promise, resolve };
}

describe("Project dependency review lifetime", () => {
  it("discards a late preview from the previously selected project", async () => {
    let projectId = "first";
    const first = deferred<ProjectDependencyCascadePreview>();
    const apply = vi.fn(async () => undefined);
    const controller = new ProjectDependencyCascadeController(() => projectId, apply, async (id) => id === "first" ? first.promise : proposal(id));
    const pending = controller.review();
    projectId = "second";
    controller.reset();
    await controller.review();
    first.resolve(proposal("first"));
    await pending;
    expect(controller.preview?.projectId).toBe("second");
    expect(apply).not.toHaveBeenCalled();
  });

  it("keeps only the latest requested preview for the same project", async () => {
    const first = deferred<ProjectDependencyCascadePreview>();
    const load = vi.fn().mockReturnValueOnce(first.promise).mockResolvedValueOnce(proposal("project", "2".repeat(64)));
    const controller = new ProjectDependencyCascadeController(() => "project", async () => undefined, load);
    const pending = controller.review();
    await controller.review();
    first.resolve(proposal());
    await pending;
    expect(controller.preview?.digest).toBe("2".repeat(64));
  });

  it("retains the reviewed proposal after an uncertain apply without expanding or refreshing it", async () => {
    const apply = vi.fn().mockRejectedValueOnce(new Error("lost response")).mockResolvedValueOnce(undefined);
    const load = vi.fn(async () => proposal());
    const controller = new ProjectDependencyCascadeController(() => "project", apply, load);
    await controller.review();
    await controller.apply();
    expect(controller.error).toBe("lost response");
    expect(controller.open).toBe(true);
    expect(controller.preview?.digest).toBe(proposal().digest);
    await controller.apply();
    expect(apply.mock.calls[0]).toEqual(apply.mock.calls[1]);
    expect(load).toHaveBeenCalledOnce();
    expect(controller.open).toBe(false);
  });

  it("prevents applying unresolved work or a preview from another project", async () => {
    let projectId = "project";
    const apply = vi.fn(async () => undefined);
    const protectedPreview = { ...proposal(), conflicts: [{ dependencyId: "edge", taskId: "hidden", title: "Hidden", reason: "scheduled" as const }] };
    const controller = new ProjectDependencyCascadeController(() => projectId, apply, async () => protectedPreview);
    await controller.review();
    await controller.apply();
    expect(apply).not.toHaveBeenCalled();
    controller.preview = proposal();
    projectId = "other";
    await controller.apply();
    expect(apply).not.toHaveBeenCalled();
  });
});
