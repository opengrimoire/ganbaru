import { beforeEach, describe, expect, it, vi } from "vitest";
import { prepareDesktopWorkspace } from "./desktop-workspace-readiness";

const dependencies = vi.hoisted(() => ({
  database: vi.fn<() => Promise<string>>(),
  projects: vi.fn<() => Promise<void>>(),
  notes: vi.fn<() => Promise<void>>(),
  chat: vi.fn<(projectId: string | null) => Promise<void>>(),
}));
vi.mock("$lib/api/db", () => ({ ensureDbUrl: dependencies.database }));
vi.mock("$lib/stores/projects.svelte", () => ({
  getProjects: () => ({ ensureLoaded: dependencies.projects, selectedProjectId: "project" }),
}));
vi.mock("$lib/stores/notes.svelte", () => ({ getNotes: () => ({ ensureLoaded: dependencies.notes }) }));
vi.mock("$lib/stores/chat.svelte", () => ({ getChat: () => ({ prewarmForProject: dependencies.chat }) }));

describe("desktop workspace readiness", () => {
  beforeEach(() => {
    dependencies.database.mockReset().mockResolvedValue("sqlite:ganbaru-ai.sqlite");
    dependencies.projects.mockReset().mockResolvedValue();
    dependencies.notes.mockReset().mockResolvedValue();
    dependencies.chat.mockReset().mockResolvedValue();
  });

  it("shares startup work and waits for workspace data before allowing entry", async () => {
    let finishProjects!: () => void;
    let finishNotes!: () => void;
    dependencies.projects.mockReturnValue(new Promise((resolve) => { finishProjects = resolve; }));
    dependencies.notes.mockReturnValue(new Promise((resolve) => { finishNotes = resolve; }));
    const first = prepareDesktopWorkspace();
    expect(prepareDesktopWorkspace()).toBe(first);
    await vi.waitFor(() => expect(dependencies.projects).toHaveBeenCalledOnce());
    expect(dependencies.notes).not.toHaveBeenCalled();
    finishProjects();
    await vi.waitFor(() => expect(dependencies.notes).toHaveBeenCalledOnce());
    expect(dependencies.chat).toHaveBeenCalledExactlyOnceWith("project");
    const ready = vi.fn();
    void first.then(ready);
    await Promise.resolve();
    expect(ready).not.toHaveBeenCalled();
    finishNotes();
    await first;
    expect(ready).toHaveBeenCalledOnce();
  });

  it("allows a failed preparation to be retried", async () => {
    dependencies.notes.mockRejectedValueOnce(new Error("workspace unavailable"));
    await expect(prepareDesktopWorkspace()).rejects.toThrow("workspace unavailable");
    await prepareDesktopWorkspace();
    expect(dependencies.notes).toHaveBeenCalledTimes(2);
  });
});
