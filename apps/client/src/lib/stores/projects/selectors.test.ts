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
        sortOrder: 1000,
        collapsed: false,
        createdAt: "2026-01-01T00:00:00.000Z",
        updatedAt: "2026-01-01T00:00:00.000Z",
      }],
    };

    expect(selectors.groupById("group-1")?.name).toBe("Inbox");
  });

  it("derives the next custom emoji sort order", () => {
    const snapshot = {
      ...emptySnapshot(),
      customEmojis: [
        {
          id: "emoji-1",
          name: "Focus",
          assetPath: "assets/project-icons/focus.png",
          sortOrder: 1000,
          createdAt: "2026-01-01T00:00:00.000Z",
          updatedAt: "2026-01-01T00:00:00.000Z",
        },
        {
          id: "emoji-2",
          name: "Break",
          assetPath: "assets/project-icons/break.png",
          sortOrder: 3000,
          createdAt: "2026-01-01T00:00:00.000Z",
          updatedAt: "2026-01-01T00:00:00.000Z",
        },
      ],
    } satisfies ProjectsSnapshot;
    const selectors = createProjectStoreSelectors(() => snapshot);

    expect(selectors.nextCustomEmojiSortOrder()).toBe(4000);
  });
});
