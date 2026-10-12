// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ProjectsWorkspaceSnapshot } from "$lib/projects/types";

const backend = vi.hoisted(() => {
  let calls = 0;
  const promise = new Promise<ProjectsWorkspaceSnapshot>(() => undefined);
  return {
    promise,
    get calls() {
      return calls;
    },
    recordCall() {
      calls += 1;
    },
  };
});

const deferredComponents = vi.hoisted(() => {
  const promise = new Promise<never>(() => undefined);
  return {
    promise,
    viewCalls: 0,
    optionalCalls: 0,
  };
});

vi.mock("$lib/components/projects/component-registry", () => ({
  loadProjectView: () => {
    deferredComponents.viewCalls += 1;
    return deferredComponents.promise;
  },
  retryProjectView: () => deferredComponents.promise,
  loadProjectOptionalComponent: () => {
    deferredComponents.optionalCalls += 1;
    return deferredComponents.promise;
  },
  retryProjectOptionalComponent: () => deferredComponents.promise,
}));

vi.mock("@tauri-apps/api/window", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@tauri-apps/api/window")>();
  return {
    ...actual,
    getCurrentWindow: () => ({ label: "main" }),
  };
});

vi.mock("@tauri-apps/api/event", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@tauri-apps/api/event")>();
  return {
    ...actual,
    listen: () => Promise.resolve(() => undefined),
  };
});

vi.mock("$lib/api/projects", async (importOriginal) => {
  const actual = await importOriginal<typeof import("$lib/api/projects")>();
  return {
    ...actual,
    loadProjectsWorkspace: () => {
      backend.recordCall();
      return backend.promise;
    },
  };
});

describe("ProjectsView first use", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = undefined;
    target = undefined;
  });

  it("renders its header and stable frame while data is unresolved", async () => {
    target = document.createElement("div");
    document.body.append(target);
    const { default: ProjectsView } = await import("./ProjectsView.svelte");

    component = mount(ProjectsView, { target });
    await tick();

    expect(backend.calls).toBe(1);
    expect(target.querySelector('[data-first-use-shell="projects"]')).not.toBeNull();
    expect(target.querySelector("[data-projects-shell-header]")?.textContent).toContain("Projects");
    expect(target.querySelector("[data-projects-content-frame]")).not.toBeNull();
    expect(target.querySelector("[data-projects-first-use-state]")?.getAttribute("aria-busy"))
      .toBeNull();
    expect(target.querySelector("[data-projects-first-use-state]")?.textContent)
      .not.toContain("Loading");
    expect(target.querySelector("[data-projects-first-use-state]")?.textContent)
      .not.toContain("No projects");
    expect(deferredComponents.viewCalls).toBe(0);
    expect(deferredComponents.optionalCalls).toBe(0);
  }, 15_000);
});
