import { describe, expect, it, vi } from "vitest";
import { translateFromPartialCatalog, type Translate } from "$lib/i18n/translator.svelte";
import { SyncPresentationRegistry, type SyncDomainPresentation } from "./presentation-registry";

const t: Translate = ((key, ...args) => translateFromPartialCatalog({}, key, ...args)) as Translate;

function notesDomain(changed: () => void = () => {}): SyncDomainPresentation {
  return {
    label: () => "Notes",
    tables: {
      notes: {
        conflicts: (_t, count) => `${count} notes in conflict`,
        recovery: {
          untitled: () => "Untitled note",
          restored: () => "Note restored",
          restoreFailed: () => "Note not restored",
        },
      },
      note_tags: {},
    },
    changed,
  };
}

describe("sync presentation registry", () => {
  it("presents a registered table through its domain", () => {
    const registry = new SyncPresentationRegistry().register(notesDomain());
    expect(registry.conflictLine(t, "notes", 2)).toBe("2 notes in conflict");
    expect(registry.recoveryUntitled(t, "notes")).toBe("Untitled note");
    expect(registry.recoveryRestored(t, "notes")).toBe("Note restored");
    expect(registry.recoveryRestoreFailed(t, "notes")).toBe("Note not restored");
  });

  it("falls back to neutral sync copy for unknown tables and missing renderers", () => {
    const registry = new SyncPresentationRegistry().register(notesDomain());
    for (const table of ["note_tags", "calendar_events"]) {
      expect(registry.conflictLine(t, table, 1)).toBe(t("sync.conflicts", 1));
      expect(registry.recoveryUntitled(t, table)).toBe(t("sync.recovery.untitled"));
      expect(registry.recoveryRestored(t, table)).toBe(t("sync.recovery.restored"));
      expect(registry.recoveryRestoreFailed(t, table)).toBe(t("sync.recovery.failed"));
    }
    expect(registry.conflictLine(t, "note_tags", 3)).not.toBe(registry.conflictLine(t, "note_tags", 1));
  });

  it("refuses a second domain for a table and keeps the first registration", () => {
    const registry = new SyncPresentationRegistry().register(notesDomain());
    const intruder: SyncDomainPresentation = {
      label: () => "Other",
      tables: { other_rows: {}, note_tags: {} },
      changed: () => {},
    };
    expect(() => registry.register(intruder)).toThrow("note_tags");
    expect(registry.labels(t)).toEqual(["Notes"]);
    expect(registry.changedNotices(["other_rows"])).toEqual([]);
  });

  it("lists domain labels in registration order", () => {
    const registry = new SyncPresentationRegistry()
      .register(notesDomain())
      .register({ label: () => "Calendar", tables: { calendar_events: {} }, changed: () => {} });
    expect(registry.labels(t)).toEqual(["Notes", "Calendar"]);
  });

  it("runs each owning domain's changed notice once and skips unknown tables", () => {
    const notesChanged = vi.fn();
    const calendarChanged = vi.fn();
    const registry = new SyncPresentationRegistry()
      .register(notesDomain(notesChanged))
      .register({ label: () => "Calendar", tables: { calendar_events: {} }, changed: calendarChanged });

    expect(registry.changedNotices(["notes", "note_tags", "notes"])).toEqual([notesChanged]);
    registry.notifyChanged(["notes", "note_tags", "unknown", "calendar_events"]);
    expect(notesChanged).toHaveBeenCalledTimes(1);
    expect(calendarChanged).toHaveBeenCalledTimes(1);

    registry.notifyChanged(["unknown"]);
    expect(notesChanged).toHaveBeenCalledTimes(1);
  });
});
