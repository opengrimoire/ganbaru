import { describe, expect, it } from "vitest";
import {
  priorityDraftDirty,
  prioritySaveDrafts,
  statusDraftDirty,
  statusSaveDrafts,
  tagNameTaken,
  tagSaveDrafts,
  type PriorityDraftState,
  type StatusDraftState,
  type TagDraftState,
} from "./collection-drafts";
import type {
  ProjectPriorityConfig,
  ProjectStatus,
  ProjectTag,
} from "$lib/projects/types";

function status(overrides: Partial<ProjectStatus> = {}): ProjectStatus {
  return {
    id: "status-1",
    projectId: "project-1",
    name: "Active",
    category: "active",
    color: 8,
    sortOrder: 1000,
    terminal: false,
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

function priority(overrides: Partial<ProjectPriorityConfig> = {}): ProjectPriorityConfig {
  return {
    id: "priority-1",
    projectId: "project-1",
    name: "Normal",
    color: 13,
    sortOrder: 1000,
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

function tag(overrides: Partial<ProjectTag> = {}): ProjectTag {
  return {
    id: "tag-1",
    projectId: "project-1",
    name: "Research",
    color: 10,
    sortOrder: 1000,
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

function statusState(
  overrides: Partial<StatusDraftState> = {},
): StatusDraftState {
  return {
    nameDrafts: {},
    categoryDrafts: {},
    colorDrafts: {},
    fallbackColor: 1,
    ...overrides,
  };
}

function priorityState(
  overrides: Partial<PriorityDraftState> = {},
): PriorityDraftState {
  return {
    nameDrafts: {},
    colorDrafts: {},
    fallbackColor: 1,
    ...overrides,
  };
}

function tagState(
  overrides: Partial<TagDraftState> = {},
): TagDraftState {
  return {
    nameDrafts: {},
    colorDrafts: {},
    fallbackColor: 1,
    ...overrides,
  };
}

describe("statusSaveDrafts", () => {
  it("returns changed status drafts with trimmed names", () => {
    const active = status();
    const result = statusSaveDrafts([
      active,
      status({ id: "status-2", name: "Done", category: "done", color: 9 }),
    ], statusState({
      nameDrafts: { "status-1": "  Started  " },
      categoryDrafts: { "status-1": "blocked" },
      colorDrafts: { "status-1": 11 },
    }));

    expect(result).toEqual({
      ok: true,
      drafts: [{ status: active, name: "Started", category: "blocked", color: 11 }],
    });
  });

  it("rejects a dirty status with a blank name", () => {
    expect(statusSaveDrafts([
      status(),
    ], statusState({ nameDrafts: { "status-1": " " } }))).toEqual({
      ok: false,
      error: "name_required",
    });
  });

  it("detects changed status fields", () => {
    expect(statusDraftDirty(status(), statusState())).toBe(false);
    expect(statusDraftDirty(
      status(),
      statusState({ categoryDrafts: { "status-1": "done" } }),
    )).toBe(true);
  });
});

describe("prioritySaveDrafts", () => {
  it("returns changed priority drafts with trimmed names", () => {
    const normal = priority();
    const result = prioritySaveDrafts([
      normal,
      priority({ id: "priority-2", name: "Urgent", color: 14 }),
    ], priorityState({
      nameDrafts: { "priority-1": "  Focus  " },
      colorDrafts: { "priority-1": 12 },
    }));

    expect(result).toEqual({
      ok: true,
      drafts: [{ priority: normal, name: "Focus", color: 12 }],
    });
  });

  it("rejects a dirty priority with a blank name", () => {
    expect(prioritySaveDrafts([
      priority(),
    ], priorityState({ nameDrafts: { "priority-1": "" } }))).toEqual({
      ok: false,
      error: "name_required",
    });
  });

  it("detects changed priority fields", () => {
    expect(priorityDraftDirty(priority(), priorityState())).toBe(false);
    expect(priorityDraftDirty(
      priority(),
      priorityState({ colorDrafts: { "priority-1": 12 } }),
    )).toBe(true);
  });
});

describe("tagSaveDrafts", () => {
  it("returns changed tag drafts and trims names", () => {
    const research = tag();
    const result = tagSaveDrafts([
      research,
      tag({ id: "tag-2", name: "Writing", color: 11 }),
    ], tagState({
      nameDrafts: { "tag-1": "  Reading  " },
      colorDrafts: { "tag-1": 12 },
    }));

    expect(result).toEqual({
      ok: true,
      drafts: [{ tag: research, name: "Reading", color: 12 }],
    });
  });

  it("rejects blank and duplicate tag draft names", () => {
    expect(tagSaveDrafts([
      tag(),
    ], tagState({ nameDrafts: { "tag-1": " " } }))).toEqual({
      ok: false,
      error: "name_required",
    });
    expect(tagSaveDrafts([
      tag(),
      tag({ id: "tag-2", name: "Writing" }),
    ], tagState({ nameDrafts: { "tag-2": " research " } }))).toEqual({
      ok: false,
      error: "name_exists",
    });
  });
});

describe("tagNameTaken", () => {
  it("matches existing tag names case-insensitively", () => {
    expect(tagNameTaken({
      tags: [tag()],
      name: " research ",
    })).toBe(true);
  });

  it("ignores blank names and the optional ignored tag", () => {
    expect(tagNameTaken({
      tags: [tag()],
      name: " ",
    })).toBe(false);
    expect(tagNameTaken({
      tags: [tag()],
      name: "Research",
      ignoredTagId: "tag-1",
    })).toBe(false);
  });
});
