import { describe, expect, it } from "vitest";
import {
  IDLE_PAGE_LOAD,
  notesShellApplied,
  notesWorkspaceError,
  notesWorkspaceLoading,
  transitionPageLoad,
  transitionShellLoad,
  UNLOADED_SHELL,
  type NotesShellLoad,
} from "./workspace-state";

describe("workspace shell load transitions", () => {
  it("loads, applies, and keeps the applied shell across a failed reload", () => {
    const loading = transitionShellLoad(UNLOADED_SHELL, { type: "started" });
    expect(loading).toEqual({ phase: "loading", hasShell: false });
    const loaded = transitionShellLoad(loading, { type: "applied" });
    expect(loaded).toEqual({ phase: "loaded" });
    const reloading = transitionShellLoad(loaded, { type: "started" });
    expect(reloading).toEqual({ phase: "loading", hasShell: true });
    const failed = transitionShellLoad(reloading, { type: "failed", error: "Offline" });
    expect(failed).toEqual({ phase: "failed", hasShell: true, error: "Offline" });
    expect(notesShellApplied(failed)).toBe(true);
  });

  it("reports a failed first load as unapplied until a retry applies it", () => {
    const failed = transitionShellLoad(transitionShellLoad(UNLOADED_SHELL, { type: "started" }), { type: "failed", error: "Offline" });
    expect(notesShellApplied(failed)).toBe(false);
    const retrying = transitionShellLoad(failed, { type: "started" });
    expect(retrying).toEqual({ phase: "loading", hasShell: false });
    expect(transitionShellLoad(retrying, { type: "applied" })).toEqual({ phase: "loaded" });
  });

  it("returns a superseded load to the shell it had", () => {
    const first: NotesShellLoad = { phase: "loading", hasShell: false };
    expect(transitionShellLoad(first, { type: "superseded" })).toBe(UNLOADED_SHELL);
    expect(transitionShellLoad({ phase: "loading", hasShell: true }, { type: "superseded" })).toEqual({ phase: "loaded" });
    const failed: NotesShellLoad = { phase: "failed", hasShell: true, error: "Offline" };
    expect(transitionShellLoad(failed, { type: "superseded" })).toBe(failed);
  });

  it("ignores a repeated apply", () => {
    const loaded: NotesShellLoad = { phase: "loaded" };
    expect(transitionShellLoad(loaded, { type: "applied" })).toBe(loaded);
  });
});

describe("selected page load transitions", () => {
  it("clears a page failure once the same page loads again", () => {
    const failed = transitionPageLoad(IDLE_PAGE_LOAD, { type: "failed", pageId: "a", error: "Outline failed" });
    expect(transitionPageLoad(failed, { type: "loaded", pageId: "a" })).toEqual({ phase: "ready", pageId: "a" });
  });

  it("keeps a failure when the selection request settles after it", () => {
    const failed = transitionPageLoad({ phase: "loading", pageId: "a" }, { type: "failed", pageId: "a", error: "Gone" });
    expect(transitionPageLoad(failed, { type: "settled", pageId: "a" })).toBe(failed);
  });

  it("settles only the page that is loading", () => {
    const loading = transitionPageLoad(IDLE_PAGE_LOAD, { type: "started", pageId: "a" });
    expect(transitionPageLoad(loading, { type: "settled", pageId: "b" })).toBe(loading);
    expect(transitionPageLoad(loading, { type: "settled", pageId: "a" })).toEqual({ phase: "ready", pageId: "a" });
  });

  it("replaces an earlier page's failure when another page starts loading", () => {
    const failed = transitionPageLoad(IDLE_PAGE_LOAD, { type: "failed", pageId: "a", error: "Gone" });
    expect(transitionPageLoad(failed, { type: "started", pageId: "b" })).toEqual({ phase: "loading", pageId: "b" });
  });

  it("returns to idle when no page is selected", () => {
    const ready = transitionPageLoad(IDLE_PAGE_LOAD, { type: "loaded", pageId: "a" });
    expect(transitionPageLoad(ready, { type: "cleared" })).toBe(IDLE_PAGE_LOAD);
    expect(transitionPageLoad(IDLE_PAGE_LOAD, { type: "cleared" })).toBe(IDLE_PAGE_LOAD);
  });
});

describe("workspace error and loading", () => {
  it("shows a shell failure before a page failure", () => {
    const shell: NotesShellLoad = { phase: "failed", hasShell: true, error: "Shell offline" };
    expect(notesWorkspaceError(shell, { phase: "failed", pageId: "a", error: "Gone" }, "a")).toBe("Shell offline");
  });

  it("shows a page failure only while that page is selected", () => {
    const page = { phase: "failed", pageId: "a", error: "Gone" } as const;
    expect(notesWorkspaceError({ phase: "loaded" }, page, "a")).toBe("Gone");
    expect(notesWorkspaceError({ phase: "loaded" }, page, "b")).toBeNull();
    expect(notesWorkspaceError({ phase: "loaded" }, page, null)).toBeNull();
  });

  it("is loading while either the shell or the page loads", () => {
    expect(notesWorkspaceLoading({ phase: "loading", hasShell: false }, IDLE_PAGE_LOAD)).toBe(true);
    expect(notesWorkspaceLoading({ phase: "loaded" }, { phase: "loading", pageId: "a" })).toBe(true);
    expect(notesWorkspaceLoading({ phase: "loaded" }, { phase: "ready", pageId: "a" })).toBe(false);
  });
});
