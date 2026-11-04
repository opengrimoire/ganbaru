import { describe, expect, it } from "vitest";
import { applyProjectMutation } from "./mutations";
import type { ProjectCustomField, ProjectMutation, ProjectsSnapshot, ProjectTask } from "$lib/projects/types";

function emptySnapshot(): ProjectsSnapshot {
  return {
    groups: [], projects: [], sections: [], statuses: [], priorities: [], tasks: [],
    checklistItems: [], tags: [], taskTagLinks: [], customFields: [], customFieldOptions: [],
    customFieldValues: [], customFieldOptionValues: [], dependencies: [], eventLinks: [],
    taskChangeEvents: [], viewPreferences: [], views: [], customEmojis: [],
  };
}

function mutation(changed: Partial<ProjectsSnapshot>, removals: ProjectMutation["removals"] = []): ProjectMutation {
  return {
    changed: { ...emptySnapshot(), ...changed },
    removals,
    calendarEventProjectAssignments: [],
  };
}

describe("applyProjectMutation", () => {
  it("preserves renamed schema rows when an older reorder receipt arrives", () => {
    const field: ProjectCustomField = { id: "field-1", projectId: "project-1", name: "Renamed",
      fieldType: "select", orderKey: "2000", icon: null, revision: 2, createdAt: "same", updatedAt: "same" };
    const option = { id: "option-1", fieldId: field.id, name: "Renamed option", orderKey: "2000",
      revision: 2, createdAt: "same", updatedAt: "same" };
    const initial = { ...emptySnapshot(), customFields: [field], customFieldOptions: [option] };
    const next = applyProjectMutation(initial, mutation({
      customFields: [{ ...field, revision: 1, name: "Old" }],
      customFieldOptions: [{ ...option, revision: 1, name: "Old option" }],
    }));
    expect(next.customFields).toEqual([field]);
    expect(next.customFieldOptions).toEqual([option]);
  });
  it("keeps newer task revisions when an old command receipt arrives with the same timestamp", () => {
    const task: ProjectTask = {
      id: "task-1", projectId: "project-1", sectionId: "section-1", statusId: "status-1",
      title: "Newer", description: "", priority: "normal", taskType: "task", sectionOrderKey: "1000",
      statusOrderKey: "1000", milestone: false, createdAt: "same", updatedAt: "same", revision: 2,
    };
    const snapshot = { ...emptySnapshot(), tasks: [task] };
    const result = applyProjectMutation(snapshot, mutation({ tasks: [{ ...task, title: "Old", revision: 1 }] }));
    expect(result.tasks).toEqual([task]);
    const next = applyProjectMutation(result, mutation({ tasks: [{ ...task, title: "Newest", revision: 3 }] }));
    expect(next.tasks[0].title).toBe("Newest");
  });
  it("immutably upserts authoritative rows for simple and composite collections", () => {
    const initial = emptySnapshot();
    const result = applyProjectMutation(initial, mutation({
      groups: [{
        id: "group-1", name: "Work", icon: "lucide:folder", orderKey: "1000", collapsed: false,
        createdAt: "created", updatedAt: "updated",
      }],
      taskTagLinks: [{ taskId: "task-1", tagId: "tag-1", createdAt: "created" }],
      customFieldValues: [{ taskId: "task-1", fieldId: "field-1", textValue: "value", updatedAt: "updated" }],
      eventLinks: [{ taskId: "task-1", eventId: "event-1", linkKind: "scheduled", createdAt: "created" }],
    }));

    expect(result.groups[0]?.name).toBe("Work");
    expect(result.taskTagLinks).toHaveLength(1);
    expect(result.customFieldValues[0]?.textValue).toBe("value");
    expect(result.eventLinks[0]?.eventId).toBe("event-1");
    expect(initial).toEqual(emptySnapshot());
  });

  it("applies removals and their local cascade before replacement rows", () => {
    const initial = mutation({
      groups: [{ id: "group-1", name: "Work", icon: "lucide:folder", orderKey: "1000", collapsed: false, createdAt: "c", updatedAt: "u" }],
      projects: [{
        id: "project-1", groupId: "group-1", name: "Project", icon: "lucide:folder", orderKey: "1000",
        status: "active", defaultEventName: null, defaultEventTimeMode: "timed",
        defaultEventDurationMinutes: null, defaultPomodoroMode: "preset", defaultIdleSettingsSource: "global",
        defaultIdlePauseEnabled: true, defaultIdleThresholdMinutes: 5, createdAt: "c", updatedAt: "u",
      }],
      tasks: [{
        id: "task-1", projectId: "project-1", sectionId: "section-1", statusId: "status-1",
        title: "Task", description: "", priority: "none", taskType: "task", sectionOrderKey: "1000",
        statusOrderKey: "1000", milestone: false, createdAt: "c", updatedAt: "u",
      }],
      tags: [{ id: "tag-1", projectId: "project-1", name: "Tag", orderKey: "1000", createdAt: "c", updatedAt: "u" }],
      taskTagLinks: [{ taskId: "task-1", tagId: "tag-1", createdAt: "c" }],
      checklistItems: [{ id: "item-1", taskId: "task-1", title: "Item", orderKey: "1000", createdAt: "c", updatedAt: "u" }],
    }).changed;

    const result = applyProjectMutation(initial, mutation({}, [{ kind: "group", id: "group-1" }]));

    expect(result.groups).toEqual([]);
    expect(result.projects).toEqual([]);
    expect(result.tasks).toEqual([]);
    expect(result.tags).toEqual([]);
    expect(result.taskTagLinks).toEqual([]);
    expect(result.checklistItems).toEqual([]);
  });

  it("clears scalar and option values before applying the authoritative replacement", () => {
    const initial = mutation({
      customFieldValues: [{ taskId: "task-1", fieldId: "field-1", textValue: "old", updatedAt: "old" }],
      customFieldOptionValues: [{ taskId: "task-1", fieldId: "field-1", optionId: "option-old" }],
    }).changed;
    const result = applyProjectMutation(initial, mutation({
      customFieldOptionValues: [{ taskId: "task-1", fieldId: "field-1", optionId: "option-new" }],
    }, [{ kind: "custom_field_value", taskId: "task-1", fieldId: "field-1" }]));

    expect(result.customFieldValues).toEqual([]);
    expect(result.customFieldOptionValues.map((value) => value.optionId)).toEqual(["option-new"]);
  });

  it("drops a removed view with its saved preferences and upserts renamed views by id", () => {
    const view = (id: string, name?: string) => ({ id, projectId: "project-1", viewType: "list" as const,
      name, orderKey: "1000", createdAt: "c", updatedAt: "u" });
    const preference = (viewId: string) => ({ projectId: "project-1", viewId, preferenceKey: "columns",
      preferenceValue: "[]", updatedAt: "u" });
    const initial = mutation({
      views: [view("view-a"), view("view-b")],
      viewPreferences: [preference("view-a"), preference("view-b")],
    }).changed;
    const result = applyProjectMutation(initial, mutation(
      { views: [view("view-b", "Backlog")] },
      [{ kind: "view", id: "view-a" }],
    ));

    expect(result.views).toEqual([view("view-b", "Backlog")]);
    expect(result.viewPreferences.map((entry) => entry.viewId)).toEqual(["view-b"]);
  });
});
