/**
 * Bridge for Projects sync conflicts: names, task titles and descriptions, checklist item titles, and
 * dependencies hidden to break a cycle.
 */

import { invoke } from "@tauri-apps/api/core";
import { ensureDbUrl } from "$lib/api/db";
import { mapMutation, type ProjectsMutationRows } from "$lib/api/projects";
import { mapSyncDeviceRef, type SyncDeviceRef } from "$lib/api/sync";
import type { ProjectMutation } from "$lib/projects/types";
import { withOwnerRequiredPrompt } from "$lib/vault/owner-required";
import {
  readArray,
  readBoolean,
  readEnum,
  readNonNegativeSafeInteger,
  readRecord,
  readString,
} from "$lib/utils/readers";

/** Row kinds that can hold a text conflict. */
export const PROJECT_CONFLICT_TARGETS = ["project", "task", "checklist_item"] as const;
export type ProjectConflictTarget = (typeof PROJECT_CONFLICT_TARGETS)[number];

/** Text fields that can hold a conflict. */
export const PROJECT_CONFLICT_FIELDS = ["name", "title", "description"] as const;
export type ProjectConflictField = (typeof PROJECT_CONFLICT_FIELDS)[number];

/** One concurrent version of a text field. */
export interface ProjectConflictVersion {
  /** Opaque id for choosing this version. */
  version: string;
  device: SyncDeviceRef;
  editedAtMs: number;
  /** Whether every device shows this version now. */
  displayed: boolean;
  text: string;
}

/** The versions of one text field in conflict, newest first. */
export interface ProjectFieldConflict {
  target: ProjectConflictTarget;
  id: string;
  field: ProjectConflictField;
  versions: ProjectConflictVersion[];
}

/** A dependency hidden to break a cycle. */
export interface ProjectDependencyConflict {
  /** Dependency key, the id the dependency has while it exists. */
  key: string;
  blockingTaskId: string;
  blockedTaskId: string;
  blockingTaskTitle: string;
  blockedTaskTitle: string;
  /** Whether adding the dependency again would close a cycle now. */
  wouldCycle: boolean;
  /** Device that added the dependency. */
  device: SyncDeviceRef;
  addedAtMs: number;
}

/** The conflicts of a project; both lists are empty when it has none. */
export interface ProjectConflicts {
  projectId: string;
  fields: ProjectFieldConflict[];
  dependencies: ProjectDependencyConflict[];
}

/** Keep the version every device shows, or use another one. */
export type ProjectFieldChoice = { kind: "displayed" } | { kind: "version"; version: string };

/** Keep a hidden dependency removed, or add it again. */
export type ProjectDependencyChoice = "keep_removed" | "restore";

export type ProjectConflictResolution =
  | {
    kind: "field";
    target: ProjectConflictTarget;
    id: string;
    field: ProjectConflictField;
    choice: ProjectFieldChoice;
  }
  | {
    kind: "dependency";
    blockingTaskId: string;
    blockedTaskId: string;
    choice: ProjectDependencyChoice;
  };

/**
 * A conflict read or resolution failure. `resolved` means the conflict or version is gone, and `would_cycle` means
 * restoring a dependency would recreate the cycle it was hidden to break.
 */
export class ProjectConflictError extends Error {
  constructor(readonly code: "resolved" | "would_cycle" | "failed", message: string) {
    super(message);
    this.name = "ProjectConflictError";
  }
}

function mapVersion(value: unknown, label: string): ProjectConflictVersion {
  const row = readRecord(value, label);
  return {
    version: readString(row.version, `${label}.version`),
    device: mapSyncDeviceRef(row.device, `${label}.device`),
    editedAtMs: readNonNegativeSafeInteger(row.editedAtMs, `${label}.editedAtMs`),
    displayed: readBoolean(row.displayed, `${label}.displayed`),
    text: readString(row.text, `${label}.text`),
  };
}

function mapFieldConflict(value: unknown, label: string): ProjectFieldConflict {
  const row = readRecord(value, label);
  return {
    target: readEnum(row.target, PROJECT_CONFLICT_TARGETS, `${label}.target`),
    id: readString(row.id, `${label}.id`),
    field: readEnum(row.field, PROJECT_CONFLICT_FIELDS, `${label}.field`),
    versions: readArray(row.versions, `${label}.versions`, mapVersion),
  };
}

function mapDependencyConflict(value: unknown, label: string): ProjectDependencyConflict {
  const row = readRecord(value, label);
  return {
    key: readString(row.key, `${label}.key`),
    blockingTaskId: readString(row.blockingTaskId, `${label}.blockingTaskId`),
    blockedTaskId: readString(row.blockedTaskId, `${label}.blockedTaskId`),
    blockingTaskTitle: readString(row.blockingTaskTitle, `${label}.blockingTaskTitle`),
    blockedTaskTitle: readString(row.blockedTaskTitle, `${label}.blockedTaskTitle`),
    wouldCycle: readBoolean(row.wouldCycle, `${label}.wouldCycle`),
    device: mapSyncDeviceRef(row.device, `${label}.device`),
    addedAtMs: readNonNegativeSafeInteger(row.addedAtMs, `${label}.addedAtMs`),
  };
}

/** Validates the conflicts of a project from the native side. */
export function mapProjectConflicts(value: unknown): ProjectConflicts {
  const row = readRecord(value, "project conflicts");
  return {
    projectId: readString(row.projectId, "project conflicts.projectId"),
    fields: readArray(row.fields, "project conflicts.fields", mapFieldConflict),
    dependencies: readArray(row.dependencies, "project conflicts.dependencies", mapDependencyConflict),
  };
}

const CONFLICT_ERROR_CODES = ["resolved", "would_cycle", "failed"] as const;

/** Turns a native `{ code, message }` rejection into a {@link ProjectConflictError}, and passes other errors through. */
export function toProjectConflictError(error: unknown): unknown {
  if (typeof error !== "object" || error === null || !("code" in error) || !("message" in error)) return error;
  const { code, message } = error;
  if (typeof message !== "string") return error;
  const known = CONFLICT_ERROR_CODES.find((candidate) => candidate === code);
  return known ? new ProjectConflictError(known, message) : error;
}

/** Reads the conflicts of a project. */
export async function loadProjectConflicts(projectId: string): Promise<ProjectConflicts> {
  const dbUrl = await ensureDbUrl();
  try {
    return mapProjectConflicts(await invoke<unknown>("projects_conflicts", { dbUrl, projectId }));
  } catch (error: unknown) {
    throw toProjectConflictError(error);
  }
}

/** Resolves a conflict on every linked device and returns the rows it changed. */
export async function resolveProjectConflict(resolution: ProjectConflictResolution): Promise<ProjectMutation> {
  const dbUrl = await ensureDbUrl();
  try {
    return mapMutation(await withOwnerRequiredPrompt(() =>
      invoke<ProjectsMutationRows>("projects_resolve_conflict", { dbUrl, resolution })));
  } catch (error: unknown) {
    throw toProjectConflictError(error);
  }
}
