// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

const backend = vi.hoisted(() => {
  const promise = new Promise<never>(() => undefined);
  let notesCalls = 0;
  let projectsCalls = 0;
  let historyCalls = 0;
  const componentCalls: string[] = [];
  return {
    promise,
    get notesCalls() {
      return notesCalls;
    },
    get projectsCalls() {
      return projectsCalls;
    },
    get historyCalls() {
      return historyCalls;
    },
    componentCalls,
    recordNotesCall() {
      notesCalls += 1;
    },
    recordProjectsCall() {
      projectsCalls += 1;
    },
    recordHistoryCall() {
      historyCalls += 1;
    },
  };
});

vi.mock("./component-registry", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./component-registry")>();
  return {
    ...actual,
    loadNotesSurface: (kind: string) => {
      backend.componentCalls.push(`surface:${kind}`);
      return backend.promise;
    },
    retryNotesSurface: (kind: string) => {
      backend.componentCalls.push(`surface-retry:${kind}`);
      return backend.promise;
    },
    loadNotesOptionalComponent: (kind: string) => {
      backend.componentCalls.push(`optional:${kind}`);
      return backend.promise;
    },
    retryNotesOptionalComponent: (kind: string) => {
      backend.componentCalls.push(`optional-retry:${kind}`);
      return backend.promise;
    },
  };
});

vi.mock("@tauri-apps/api/window", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@tauri-apps/api/window")>();
  return {
    ...actual,
    getCurrentWindow: () => ({ label: "main" }),
  };
});

vi.mock("$lib/api/notes", async (importOriginal) => {
  const actual = await importOriginal<typeof import("$lib/api/notes")>();
  const unresolved = () => {
    backend.recordNotesCall();
    return backend.promise;
  };
  return {
    ...actual,
    loadNotesWorkspaceShell: unresolved,
  };
});

vi.mock("$lib/api/projects", async (importOriginal) => {
  const actual = await importOriginal<typeof import("$lib/api/projects")>();
  return {
    ...actual,
    loadProjectsWorkspace: () => {
      backend.recordProjectsCall();
      return backend.promise;
    },
  };
});

vi.mock("$lib/api/notes/project-history", async (importOriginal) => {
  const actual = await importOriginal<typeof import("$lib/api/notes/project-history")>();
  return {
    ...actual,
    flushDueNotesProjectHistory: () => {
      backend.recordHistoryCall();
      return Promise.resolve();
    },
    initializeNotesProjectHistory: () => {
      backend.recordHistoryCall();
      return Promise.resolve();
    },
  };
});

describe("NotesView first use", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    vi.unstubAllGlobals();
    component = undefined;
    target = undefined;
  });

  it("renders its workspace shell while initial backend promises are unresolved", async () => {
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      callback(0);
      return 1;
    });
    target = document.createElement("div");
    document.body.append(target);
    const { default: NotesView } = await import("./NotesView.svelte");

    component = mount(NotesView, { target });
    await tick();

    expect(backend.notesCalls).toBe(1);
    expect(backend.projectsCalls).toBe(1);
    expect(backend.historyCalls).toBe(0);
    expect(target.querySelector('[data-first-use-shell="notes"]')).not.toBeNull();
    expect(target.querySelector("[data-notes-workspace-header]")).not.toBeNull();
    expect(target.querySelector("[data-notes-first-use-state]")?.getAttribute("aria-busy"))
      .toBeNull();
    expect(target.querySelector("[data-notes-first-use-state]")?.textContent).not.toContain("Loading");
    expect(backend.componentCalls).toEqual([]);
  }, 15_000);

  it("keeps the sidebar choice when leaving Notes and returning", async () => {
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      callback(0);
      return 1;
    });
    target = document.createElement("div");
    document.body.append(target);
    const { default: NotesView } = await import("./NotesView.svelte");
    const { getNotes } = await import("$lib/stores/notes.svelte");
    const notes = getNotes();
    notes.explorerCollapsed = false;
    component = mount(NotesView, { target });
    await tick();

    const collapse = target.querySelector<HTMLButtonElement>('button[aria-label="Collapse sidebar"]');
    expect(collapse).not.toBeNull();
    collapse?.click();
    await tick();
    expect(target.querySelector("[data-notes-explorer]")?.classList.contains("notes-project-explorer-collapsed")).toBe(true);

    await unmount(component);
    component = mount(NotesView, { target });
    await tick();
    expect(target.querySelector("[data-notes-explorer]")?.classList.contains("notes-project-explorer-collapsed")).toBe(true);

    const expand = target.querySelector<HTMLButtonElement>('button[aria-label="Expand sidebar"]');
    expect(expand).not.toBeNull();
    expand?.click();
    await tick();
    await unmount(component);
    component = mount(NotesView, { target });
    await tick();
    expect(target.querySelector("[data-notes-explorer]")?.classList.contains("notes-project-explorer-collapsed")).toBe(false);
  });

});
