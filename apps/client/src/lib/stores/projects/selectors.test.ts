import { describe, expect, it } from "vitest";
import type { ProjectsSnapshot } from "$lib/projects/types";
import { createProjectStoreSelectors } from "./selectors";

function emptySnapshot(): ProjectsSnapshot {
  return {
    groups: [],
    projects: [],
    sections: [],
    statuses: [],
    priorities: [],
    tasks: [],
    checklistItems: [],
    tags: [],
    taskTagLinks: [],
    customFields: [],
    customFieldOptions: [],
    customFieldValues: [],
    customFieldOptionValues: [],
    dependencies: [],
    eventLinks: [],
    taskChangeEvents: [],
    viewPreferences: [],
    views: [],
    customEmojis: [],
  };
}

describe("createProjectStoreSelectors", () => {
  it("reads the current snapshot each time", () => {
    let snapshot = emptySnapshot();
    const selectors = createProjectStoreSelectors(() => snapshot);

    expect(selectors.groupById("group-1")).toBeUndefined();

    snapshot = {
      ...snapshot,
      groups: [{
        id: "group-1",
        name: "Inbox",
        icon: "lucide:folder",
        orderKey: "1000",
        collapsed: false,
        createdAt: "2026-01-01T00:00:00.000Z",
        updatedAt: "2026-01-01T00:00:00.000Z",
      }],
    };

    expect(selectors.groupById("group-1")?.name).toBe("Inbox");
  });
});
