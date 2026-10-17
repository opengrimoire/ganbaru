import { invoke } from "@tauri-apps/api/core";
import { ensureDbUrl } from "$lib/api/db";
import { parseProjectTaskMutation } from "./projects";
import type { ProjectMutation } from "$lib/projects/types";

export interface ProjectDependencyCascadeReason {
  dependencyId: string;
  blockingTaskId: string;
  blockingTitle: string;
  requiredStartDate: string;
}

export interface ProjectDependencyCascadeItem {
  taskId: string;
  title: string;
  shiftDays: number;
  originalStartDate: string | null;
  originalDueDate: string | null;
  originalTargetEndDate: string | null;
  originalRangeStart: string;
  originalRangeEnd: string;
  nextStartDate: string | null;
  nextDueDate: string | null;
  nextTargetEndDate: string | null;
  nextRangeStart: string;
  nextRangeEnd: string;
  reasons: ProjectDependencyCascadeReason[];
}

export type ProjectCascadeConflictReason = "missing_endpoint" | "cycle" | "undated_task" | "invalid_date" | "archived" | "completed" | "scheduled" | "date_overflow";
export interface ProjectDependencyCascadePreview {
  projectId: string;
  digest: string;
  items: ProjectDependencyCascadeItem[];
  conflicts: { dependencyId: string; taskId: string; title: string; reason: ProjectCascadeConflictReason }[];
}
export interface ProjectDependencyCascadeApply {
  operationId: string;
  projectId: string;
  reviewedDigest: string;
}

/** Validate native preview data before displaying or retaining it for review. */
export function parseProjectCascadePreview(value: unknown, projectId: string): ProjectDependencyCascadePreview {
  let textBytes = 0;
  let reasonCount = 0;
  function object(value: unknown): Record<string, unknown> {
    if (typeof value !== "object" || value === null || Array.isArray(value)) throw new Error("Invalid dependency preview object");
    return value as Record<string, unknown>;
  }
  function text(value: unknown, limit = 32 * 1024 * 1024): string {
    if (typeof value !== "string" || value.length > limit) throw new Error("Invalid dependency preview text");
    textBytes += value.length * 3;
    if (textBytes > 32 * 1024 * 1024) throw new Error("Dependency preview exceeds its text limit");
    return value;
  }
  function id(value: unknown): string {
    const result = text(value, 256);
    if (!result.trim()) throw new Error("Invalid dependency preview identity");
    return result;
  }
  function date(value: unknown): string {
    const result = text(value, 10);
    if (!/^\d{4}-\d{2}-\d{2}$/u.test(result) || result.startsWith("0000")) throw new Error("Invalid dependency preview date");
    const parsed = new Date(`${result}T00:00:00Z`);
    if (Number.isNaN(parsed.getTime()) || parsed.toISOString().slice(0, 10) !== result) throw new Error("Invalid dependency preview date");
    return result;
  }
  const nullableDate = (value: unknown): string | null => value === null ? null : date(value);
  function array(value: unknown, limit: number): unknown[] {
    if (!Array.isArray(value) || value.length > limit) throw new Error("Invalid dependency preview array");
    return value;
  }
  const row = object(value);
  if (row.projectId !== projectId) throw new Error("Dependency preview belongs to another project");
  const digest = text(row.digest, 64);
  if (!/^[0-9a-f]{64}$/u.test(digest)) throw new Error("Invalid dependency preview digest");
  const items = array(row.items, 10_000).map((value): ProjectDependencyCascadeItem => {
    const item = object(value);
    if (typeof item.shiftDays !== "number" || !Number.isSafeInteger(item.shiftDays) || item.shiftDays <= 0) throw new Error("Invalid dependency shift");
    return {
      taskId: id(item.taskId), title: text(item.title), shiftDays: item.shiftDays,
      originalStartDate: nullableDate(item.originalStartDate), originalDueDate: nullableDate(item.originalDueDate), originalTargetEndDate: nullableDate(item.originalTargetEndDate),
      originalRangeStart: date(item.originalRangeStart), originalRangeEnd: date(item.originalRangeEnd),
      nextStartDate: nullableDate(item.nextStartDate), nextDueDate: nullableDate(item.nextDueDate), nextTargetEndDate: nullableDate(item.nextTargetEndDate),
      nextRangeStart: date(item.nextRangeStart), nextRangeEnd: date(item.nextRangeEnd),
      reasons: array(item.reasons, 20_000).map((value) => {
        reasonCount += 1;
        if (reasonCount > 20_000) throw new Error("Dependency preview exceeds its reason limit");
        const reason = object(value); return {
        dependencyId: id(reason.dependencyId), blockingTaskId: id(reason.blockingTaskId), blockingTitle: text(reason.blockingTitle), requiredStartDate: date(reason.requiredStartDate),
      }; }),
    };
  });
  if (new Set(items.map((item) => item.taskId)).size !== items.length) throw new Error("Duplicate dependency preview task");
  const conflicts = array(row.conflicts, 20_000).map((value): ProjectDependencyCascadePreview["conflicts"][number] => {
    const conflict = object(value);
    const reason = conflict.reason;
    if (reason !== "missing_endpoint" && reason !== "cycle" && reason !== "undated_task" && reason !== "invalid_date"
      && reason !== "archived" && reason !== "completed" && reason !== "scheduled" && reason !== "date_overflow") throw new Error("Invalid dependency conflict reason");
    return { dependencyId: id(conflict.dependencyId), taskId: id(conflict.taskId), title: text(conflict.title), reason };
  });
  return { projectId, digest, items, conflicts };
}

/** Read a complete proposal without using the filtered frontend task cache. */
export async function previewProjectDependencyCascade(projectId: string): Promise<ProjectDependencyCascadePreview> {
  const value = await invoke<unknown>("projects_preview_dependency_cascade", { dbUrl: await ensureDbUrl(), projectId });
  return parseProjectCascadePreview(value, projectId);
}

/** Apply a reviewed cascade. After an uncertain response, retry with the same digest and operation ID. */
export async function applyProjectDependencyCascade(request: ProjectDependencyCascadeApply): Promise<ProjectMutation> {
  const value = await invoke<unknown>("projects_apply_dependency_cascade", { dbUrl: await ensureDbUrl(), request });
  return parseProjectTaskMutation(value, request.projectId);
}
