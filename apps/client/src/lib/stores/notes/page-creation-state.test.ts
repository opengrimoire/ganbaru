import { describe, expect, it } from "vitest";
import type { NotesLoadedPage, NotesPageCreate } from "$lib/notes/types";
import {
  notesPageCreationError,
  notesPageCreationSaved,
  notesPageCreationStarted,
  transitionPageCreation,
} from "./page-creation-state";

const request = { id: "page" } as NotesPageCreate;
const loaded = { page: { id: "page" } } as NotesLoadedPage;

describe("page creation transitions", () => {
  it("retries a failed create request from creating", () => {
    const creating = notesPageCreationStarted(request);
    const failed = transitionPageCreation(creating, { type: "create-failed", error: "Disk full" });
    expect(failed).toEqual({ phase: "create-failed", request, error: "Disk full" });
    expect(notesPageCreationError(failed)).toBe("Disk full");
    expect(notesPageCreationSaved(failed)).toBe(false);
    expect(transitionPageCreation(failed, { type: "create-started" })).toEqual(creating);
  });

  it("keeps the created page through a failed reconcile and its retry", () => {
    const reconciling = transitionPageCreation(notesPageCreationStarted(request), { type: "created", loaded });
    expect(reconciling).toEqual({ phase: "reconciling", request, loaded });
    const failed = transitionPageCreation(reconciling, { type: "reconcile-failed", error: "Hydrate failed" });
    expect(failed).toEqual({ phase: "reconcile-failed", request, loaded, error: "Hydrate failed" });
    expect(notesPageCreationSaved(failed)).toBe(true);
    expect(notesPageCreationError(failed)).toBe("Hydrate failed");
    expect(transitionPageCreation(failed, { type: "reconcile-started" })).toEqual(reconciling);
  });

  it("ignores events that do not fit the phase", () => {
    const creating = notesPageCreationStarted(request);
    expect(transitionPageCreation(creating, { type: "reconcile-failed", error: "Late" })).toBe(creating);
    expect(transitionPageCreation(creating, { type: "create-started" })).toBe(creating);
    const reconciling = transitionPageCreation(creating, { type: "created", loaded });
    expect(transitionPageCreation(reconciling, { type: "create-failed", error: "Late" })).toBe(reconciling);
    expect(transitionPageCreation(reconciling, { type: "created", loaded })).toBe(reconciling);
    expect(notesPageCreationError(undefined)).toBeNull();
    expect(notesPageCreationSaved(undefined)).toBe(false);
  });
});
