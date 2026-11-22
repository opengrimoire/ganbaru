import { describe, expect, it } from "vitest";
import {
  CLEAN_DRAFT,
  draftHasUnsavedEdits,
  draftLifecycleFlags,
  nextDraftLifecycle,
  type ChatDraftLifecycle,
  type ChatDraftLifecycleEvent,
} from "./draft-state";

const states: Record<string, ChatDraftLifecycle> = {
  loading: { kind: "loading", error: null },
  loadingWithError: { kind: "loading", error: "Earlier save failed" },
  clean: CLEAN_DRAFT,
  cleanChangedElsewhere: { kind: "clean", changedElsewhere: true, error: null },
  cleanWithError: { kind: "clean", changedElsewhere: false, error: "Read failed" },
  edited: { kind: "edited", error: null },
  editedWithError: { kind: "edited", error: "Save failed" },
  saving: { kind: "saving" },
};

const events: Record<string, ChatDraftLifecycleEvent> = {
  bindStarted: { kind: "bindStarted" },
  loadedClean: { kind: "loaded", dirty: false },
  loadedDirty: { kind: "loaded", dirty: true },
  loadFailed: { kind: "loadFailed", error: "Offline" },
  changed: { kind: "changed" },
  saveStarted: { kind: "saveStarted" },
  saveLanded: { kind: "saveLanded" },
  failed: { kind: "failed", error: "Disk full" },
  adoptedConflict: { kind: "adopted", changedElsewhere: true },
  adoptedRefresh: { kind: "adopted", changedElsewhere: false },
  emptied: { kind: "emptied" },
  noticeDismissed: { kind: "noticeDismissed" },
};

/** The expected next state for every state and event, written out so a new state or event fails until it is decided. */
const expected: Record<string, Record<string, ChatDraftLifecycle>> = {
  loading: {
    bindStarted: states.loading, loadedClean: CLEAN_DRAFT, loadedDirty: states.edited,
    loadFailed: { kind: "clean", changedElsewhere: false, error: "Offline" }, changed: states.loading,
    saveStarted: states.loading, saveLanded: states.loading, failed: { kind: "loading", error: "Disk full" },
    adoptedConflict: states.cleanChangedElsewhere, adoptedRefresh: CLEAN_DRAFT, emptied: CLEAN_DRAFT, noticeDismissed: states.loading,
  },
  loadingWithError: {
    bindStarted: states.loading, loadedClean: CLEAN_DRAFT, loadedDirty: states.edited,
    loadFailed: { kind: "clean", changedElsewhere: false, error: "Offline" }, changed: states.loadingWithError,
    saveStarted: states.loadingWithError, saveLanded: states.loadingWithError, failed: { kind: "loading", error: "Disk full" },
    adoptedConflict: states.cleanChangedElsewhere, adoptedRefresh: CLEAN_DRAFT, emptied: CLEAN_DRAFT, noticeDismissed: states.loadingWithError,
  },
  clean: {
    bindStarted: states.loading, loadedClean: CLEAN_DRAFT, loadedDirty: states.edited,
    loadFailed: { kind: "clean", changedElsewhere: false, error: "Offline" }, changed: states.edited,
    saveStarted: CLEAN_DRAFT, saveLanded: CLEAN_DRAFT, failed: { kind: "clean", changedElsewhere: false, error: "Disk full" },
    adoptedConflict: states.cleanChangedElsewhere, adoptedRefresh: CLEAN_DRAFT, emptied: CLEAN_DRAFT, noticeDismissed: CLEAN_DRAFT,
  },
  cleanChangedElsewhere: {
    bindStarted: states.loading, loadedClean: CLEAN_DRAFT, loadedDirty: states.edited,
    loadFailed: { kind: "clean", changedElsewhere: false, error: "Offline" }, changed: states.edited,
    saveStarted: states.cleanChangedElsewhere, saveLanded: states.cleanChangedElsewhere,
    failed: { kind: "clean", changedElsewhere: true, error: "Disk full" },
    adoptedConflict: states.cleanChangedElsewhere, adoptedRefresh: CLEAN_DRAFT, emptied: CLEAN_DRAFT, noticeDismissed: CLEAN_DRAFT,
  },
  cleanWithError: {
    bindStarted: states.loading, loadedClean: CLEAN_DRAFT, loadedDirty: states.edited,
    loadFailed: { kind: "clean", changedElsewhere: false, error: "Offline" }, changed: states.edited,
    saveStarted: states.cleanWithError, saveLanded: states.cleanWithError,
    failed: { kind: "clean", changedElsewhere: false, error: "Disk full" },
    adoptedConflict: states.cleanChangedElsewhere, adoptedRefresh: CLEAN_DRAFT, emptied: CLEAN_DRAFT, noticeDismissed: states.cleanWithError,
  },
  edited: {
    bindStarted: states.loading, loadedClean: CLEAN_DRAFT, loadedDirty: states.edited,
    loadFailed: { kind: "clean", changedElsewhere: false, error: "Offline" }, changed: states.edited,
    saveStarted: states.saving, saveLanded: CLEAN_DRAFT, failed: { kind: "edited", error: "Disk full" },
    adoptedConflict: states.cleanChangedElsewhere, adoptedRefresh: CLEAN_DRAFT, emptied: CLEAN_DRAFT, noticeDismissed: states.edited,
  },
  editedWithError: {
    bindStarted: states.loading, loadedClean: CLEAN_DRAFT, loadedDirty: states.edited,
    loadFailed: { kind: "clean", changedElsewhere: false, error: "Offline" }, changed: states.edited,
    saveStarted: states.saving, saveLanded: { kind: "clean", changedElsewhere: false, error: "Save failed" },
    failed: { kind: "edited", error: "Disk full" },
    adoptedConflict: states.cleanChangedElsewhere, adoptedRefresh: CLEAN_DRAFT, emptied: CLEAN_DRAFT, noticeDismissed: states.editedWithError,
  },
  saving: {
    bindStarted: states.loading, loadedClean: CLEAN_DRAFT, loadedDirty: states.edited,
    loadFailed: { kind: "clean", changedElsewhere: false, error: "Offline" }, changed: states.saving,
    saveStarted: states.saving, saveLanded: CLEAN_DRAFT, failed: { kind: "edited", error: "Disk full" },
    adoptedConflict: states.cleanChangedElsewhere, adoptedRefresh: CLEAN_DRAFT, emptied: CLEAN_DRAFT, noticeDismissed: states.saving,
  },
};

describe("nextDraftLifecycle", () => {
  for (const [stateName, state] of Object.entries(states)) {
    for (const [eventName, event] of Object.entries(events)) {
      it(`moves ${stateName} on ${eventName}`, () => {
        expect(nextDraftLifecycle(state, event)).toEqual(expected[stateName]?.[eventName]);
      });
    }
  }

  it("covers every state and event in the expectation table", () => {
    expect(Object.keys(expected).sort()).toEqual(Object.keys(states).sort());
    for (const row of Object.values(expected)) expect(Object.keys(row).sort()).toEqual(Object.keys(events).sort());
  });
});

describe("draftLifecycleFlags", () => {
  it("presents each state as the composer flags it always meant", () => {
    expect(draftLifecycleFlags(states.loadingWithError)).toEqual({
      loading: true, saving: false, dirty: false, changedElsewhere: false, error: "Earlier save failed",
    });
    expect(draftLifecycleFlags(states.cleanChangedElsewhere)).toEqual({
      loading: false, saving: false, dirty: false, changedElsewhere: true, error: null,
    });
    expect(draftLifecycleFlags(states.editedWithError)).toEqual({
      loading: false, saving: false, dirty: true, changedElsewhere: false, error: "Save failed",
    });
    expect(draftLifecycleFlags(states.saving)).toEqual({
      loading: false, saving: true, dirty: true, changedElsewhere: false, error: null,
    });
  });

  it("never reports a save without unsaved edits or a notice beside unsaved edits", () => {
    for (const state of Object.values(states)) {
      const flags = draftLifecycleFlags(state);
      if (flags.saving) expect(flags.dirty).toBe(true);
      if (flags.dirty) expect(flags.changedElsewhere).toBe(false);
      if (flags.loading) expect(flags.dirty || flags.saving || flags.changedElsewhere).toBe(false);
      expect(draftHasUnsavedEdits(state)).toBe(flags.dirty);
    }
  });
});
