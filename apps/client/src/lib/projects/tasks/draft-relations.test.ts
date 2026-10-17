import { describe, expect, it } from "vitest";
import {
  applyProjectTaskDraftRelations,
  emptyProjectTaskDraftRelations,
  moveProjectTaskDraftEntry,
  projectTaskDraftChecklistItems,
  projectTaskDraftDependencies,
  projectTaskDraftHasTagName,
  projectTaskDraftPreview,
  projectTaskDraftRelationsChanged,
  projectTaskDraftSubtaskTasks,
  projectTaskDraftTags,
  removeProjectTaskDraftTag,
  type ProjectTaskDraftRelations,
  type ProjectTaskDraftRelationWriter,
} from "./draft-relations";
import { projectTaskNewDetailDraft } from "./detail";
import type { ProjectTag, ProjectTask } from "$lib/projects/types";

const TIMESTAMP = "2026-10-07T12:00:00Z";

function relations(overrides: Partial<ProjectTaskDraftRelations> = {}): ProjectTaskDraftRelations {
  return { ...emptyProjectTaskDraftRelations(), ...overrides };
}

function tag(id: string, name: string): ProjectTag {
  return {
    id,
    projectId: "project-a",
    name,
    sortOrder: 0,
    createdAt: TIMESTAMP,
    updatedAt: TIMESTAMP,
  };
}

function draftTask(parentTaskId: string | null = null): ProjectTask {
  return projectTaskDraftPreview({
    id: "draft",
    projectId: "project-a",
    draft: {
      ...projectTaskNewDetailDraft({ sectionId: "section-a", statusId: "todo" }),
      title: "Write report",
      priority: "high",
    },
    parentTaskId,
    timestamp: TIMESTAMP,
  });
}

describe("projectTaskDraftRelationsChanged", () => {
  it("reports an empty draft as unchanged", () => {
    expect(projectTaskDraftRelationsChanged(emptyProjectTaskDraftRelations())).toBe(false);
  });

  it("reports any attached relation as a change", () => {
    expect(projectTaskDraftRelationsChanged(relations({ parentTaskId: "parent" }))).toBe(true);
    expect(projectTaskDraftRelationsChanged(relations({ newTagNames: ["urgent"] }))).toBe(true);
    expect(projectTaskDraftRelationsChanged(relations({ eventIds: ["event-a"] }))).toBe(true);
  });
});

describe("moveProjectTaskDraftEntry", () => {
  const entries = [{ id: "a" }, { id: "b" }, { id: "c" }];

  it("swaps an entry with its neighbor", () => {
    expect(moveProjectTaskDraftEntry(entries, "b", -1).map((entry) => entry.id)).toEqual(["b", "a", "c"]);
    expect(moveProjectTaskDraftEntry(entries, "b", 1).map((entry) => entry.id)).toEqual(["a", "c", "b"]);
  });

  it("leaves the order unchanged at the edges or for unknown entries", () => {
    expect(moveProjectTaskDraftEntry(entries, "a", -1).map((entry) => entry.id)).toEqual(["a", "b", "c"]);
    expect(moveProjectTaskDraftEntry(entries, "c", 1).map((entry) => entry.id)).toEqual(["a", "b", "c"]);
    expect(moveProjectTaskDraftEntry(entries, "missing", 1).map((entry) => entry.id)).toEqual(["a", "b", "c"]);
  });

  it("does not mutate the input list", () => {
    moveProjectTaskDraftEntry(entries, "b", 1);
    expect(entries.map((entry) => entry.id)).toEqual(["a", "b", "c"]);
  });
});

describe("draft projections", () => {
  it("builds a preview task from the draft fields and parent", () => {
    const task = draftTask("parent");
    expect(task).toMatchObject({
      id: "draft",
      projectId: "project-a",
      sectionId: "section-a",
      statusId: "todo",
      parentTaskId: "parent",
      title: "Write report",
      priority: "high",
    });
    expect(draftTask().parentTaskId).toBeUndefined();
  });

  it("projects checklist entries in order with completion", () => {
    const items = projectTaskDraftChecklistItems(relations({
      checklist: [
        { id: "one", title: "Outline", completed: true },
        { id: "two", title: "Draft", completed: false },
      ],
    }), "draft", TIMESTAMP);
    expect(items.map((item) => [item.id, item.taskId, item.sortOrder, item.completedAt])).toEqual([
      ["one", "draft", 0, TIMESTAMP],
      ["two", "draft", 1, undefined],
    ]);
  });

  it("projects subtasks under the draft with their own status", () => {
    const parent = draftTask();
    const subtasks = projectTaskDraftSubtaskTasks(relations({
      subtasks: [{ id: "sub", title: "Collect data", statusId: "done" }],
    }), parent);
    expect(subtasks).toHaveLength(1);
    expect(subtasks[0]).toMatchObject({
      id: "sub",
      parentTaskId: "draft",
      sectionId: "section-a",
      statusId: "done",
      title: "Collect data",
      priority: "normal",
    });
  });

  it("identifies draft dependencies by their blocking task", () => {
    const dependencies = projectTaskDraftDependencies(relations({ blockedByTaskIds: ["blocker"] }), "draft", TIMESTAMP);
    expect(dependencies).toEqual([{
      id: "blocker",
      blockingTaskId: "blocker",
      blockedTaskId: "draft",
      dependencyType: "blocks",
      createdAt: TIMESTAMP,
    }]);
  });
});

describe("draft tags", () => {
  const projectTags = [tag("tag-a", "Writing"), tag("tag-b", "Research")];

  it("lists linked tags followed by pending new tags and skips missing tags", () => {
    const tags = projectTaskDraftTags({
      relations: relations({ tagIds: ["tag-b", "deleted"], newTagNames: ["Urgent"] }),
      projectTags,
      projectId: "project-a",
      timestamp: TIMESTAMP,
    });
    expect(tags.map((entry) => entry.name)).toEqual(["Research", "Urgent"]);
  });

  it("removes linked and pending tags by their displayed identity", () => {
    const draft = relations({ tagIds: ["tag-a"], newTagNames: ["Urgent"] });
    const [linked, pending] = projectTaskDraftTags({
      relations: draft,
      projectTags,
      projectId: "project-a",
      timestamp: TIMESTAMP,
    });
    expect(removeProjectTaskDraftTag(draft, linked.id)).toMatchObject({ tagIds: [], newTagNames: ["Urgent"] });
    expect(removeProjectTaskDraftTag(draft, pending.id)).toMatchObject({ tagIds: ["tag-a"], newTagNames: [] });
  });

  it("matches tag names case-insensitively against linked and pending tags only", () => {
    const draft = relations({ tagIds: ["tag-a"], newTagNames: ["Urgent"] });
    expect(projectTaskDraftHasTagName(draft, projectTags, " writing ")).toBe(true);
    expect(projectTaskDraftHasTagName(draft, projectTags, "URGENT")).toBe(true);
    expect(projectTaskDraftHasTagName(draft, projectTags, "Research")).toBe(false);
  });
});

describe("applyProjectTaskDraftRelations", () => {
  function recordingWriter(failing: Set<string> = new Set()): {
    writer: ProjectTaskDraftRelationWriter;
    calls: string[];
  } {
    const calls: string[] = [];
    const record = async (call: string): Promise<void> => {
      calls.push(call);
      if (failing.has(call)) throw new Error(`${call} failed`);
    };
    return {
      calls,
      writer: {
        addChecklistItem: (taskId, title, completed) => record(`checklist:${taskId}:${title}:${completed}`),
        addSubtask: (parent, title, statusId) => record(`subtask:${parent.id}:${title}:${statusId}`),
        linkTag: (taskId, tagId) => record(`tag:${taskId}:${tagId}`),
        addAndLinkTag: (task, name) => record(`new-tag:${task.id}:${name}`),
        addDependency: (blockingTaskId, blockedTaskId) => record(`dependency:${blockingTaskId}:${blockedTaskId}`),
        linkEvent: (taskId, eventId) => record(`event:${taskId}:${eventId}`),
        saveCustomFieldValue: (value) => record(`field:${value.taskId}:${value.fieldId}:${value.textValue}`),
      },
    };
  }

  const draft = relations({
    checklist: [{ id: "c", title: "Outline", completed: true }],
    subtasks: [{ id: "s", title: "Collect data", statusId: "todo" }],
    tagIds: ["tag-a"],
    newTagNames: ["Urgent"],
    blockedByTaskIds: ["blocker"],
    eventIds: ["event-a"],
  });
  const created = { ...draftTask(), id: "task-1" };
  const customFieldValues = [{
    fieldId: "notes",
    textValue: "Draft value",
    numberValue: null,
    dateValue: null,
    checkboxValue: null,
    optionIds: [],
  }];

  it("writes every relation against the created task", async () => {
    const { writer, calls } = recordingWriter();
    const failures = await applyProjectTaskDraftRelations({ task: created, relations: draft, customFieldValues, writer });
    expect(failures).toEqual([]);
    expect(calls).toEqual([
      "checklist:task-1:Outline:true",
      "subtask:task-1:Collect data:todo",
      "tag:task-1:tag-a",
      "new-tag:task-1:Urgent",
      "dependency:blocker:task-1",
      "event:task-1:event-a",
      "field:task-1:notes:Draft value",
    ]);
  });

  it("continues after a failed write and reports each failure", async () => {
    const { writer, calls } = recordingWriter(new Set(["tag:task-1:tag-a", "event:task-1:event-a"]));
    const failures = await applyProjectTaskDraftRelations({ task: created, relations: draft, customFieldValues, writer });
    expect(failures).toEqual(["tag:task-1:tag-a failed", "event:task-1:event-a failed"]);
    expect(calls).toHaveLength(7);
  });

  it("writes nothing for an empty draft", async () => {
    const { writer, calls } = recordingWriter();
    const failures = await applyProjectTaskDraftRelations({
      task: created,
      relations: emptyProjectTaskDraftRelations(),
      customFieldValues: [],
      writer,
    });
    expect(failures).toEqual([]);
    expect(calls).toEqual([]);
  });
});
