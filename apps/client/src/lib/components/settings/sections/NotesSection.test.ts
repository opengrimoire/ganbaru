// @vitest-environment jsdom

import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import NotesSection from "./NotesSection.svelte";

interface RetentionImpact {
  versionCount: number;
  storedBytes: number;
}

const mocks = vi.hoisted(() => ({
  notes: {
    pageHistorySettings: { retention_days: 90 } as { retention_days: number } | null,
    pageHistorySettingsLoading: false,
    pageHistorySettingsError: null as string | null,
    loadPageHistorySettings: (): Promise<void> => Promise.resolve(),
    updatePageHistoryRetention: vi.fn((_days: number): Promise<void> => Promise.resolve()),
  },
  impact: vi.fn((_days: number): Promise<RetentionImpact> => Promise.resolve({ versionCount: 0, storedBytes: 0 })),
}));

vi.mock("$lib/stores/notes.svelte", () => ({ getNotes: () => mocks.notes }));
vi.mock("$lib/api/notes/project-history", () => ({ getNotesHistoryRetentionImpact: mocks.impact }));
vi.mock("$lib/stores/preferences.svelte", () => ({
  getPreferences: () => ({
    notesDefaultOpenMode: "center",
    notesSidebarOrder: "manual",
    notesShowPropertyIcons: true,
    notesMentionNotificationsEnabled: true,
    notesReminderNotificationsEnabled: true,
    notesUserMentionNotificationsEnabled: true,
    notesTaskMentionNotificationsEnabled: true,
    notesNotificationIncludeContent: true,
  }),
}));

let target: HTMLDivElement;
let component: ReturnType<typeof mount> | undefined;

async function settle(): Promise<void> {
  for (let index = 0; index < 5; index += 1) {
    await Promise.resolve();
    flushSync();
  }
}

/** Opens the history retention menu and picks the option with `label`. */
function chooseRetention(label: string): void {
  const trigger = [...target.querySelectorAll<HTMLButtonElement>("button")]
    .find((button) => button.textContent?.includes("days") || button.textContent?.trim() === "Off");
  if (!trigger) throw new Error("No history retention menu");
  trigger.click();
  flushSync();
  const option = [...document.body.querySelectorAll<HTMLButtonElement>('[role="option"]')]
    .find((button) => button.textContent?.trim() === label);
  if (!option) throw new Error(`No ${label} option`);
  option.click();
  flushSync();
}

describe("Notes settings section", () => {
  beforeEach(() => {
    target = document.createElement("div");
    document.body.append(target);
    mocks.notes.pageHistorySettings = { retention_days: 90 };
    mocks.notes.updatePageHistoryRetention.mockClear();
    mocks.impact.mockReset();
  });

  afterEach(() => {
    if (component) unmount(component);
    component = undefined;
    target.remove();
  });

  it("does not apply a shorter retention whose impact read finishes after a newer choice", async () => {
    let finishImpact = (_impact: RetentionImpact): void => {};
    mocks.impact.mockReturnValueOnce(new Promise<RetentionImpact>((resolve) => {
      finishImpact = resolve;
    }));
    component = mount(NotesSection, { target, props: { transfersAvailable: false, notificationsAvailable: false } });
    flushSync();

    chooseRetention("30 days");
    chooseRetention("365 days");
    await settle();
    expect(mocks.notes.updatePageHistoryRetention.mock.calls).toEqual([[365]]);

    finishImpact({ versionCount: 0, storedBytes: 0 });
    await settle();
    expect(mocks.notes.updatePageHistoryRetention.mock.calls).toEqual([[365]]);
  });
});
