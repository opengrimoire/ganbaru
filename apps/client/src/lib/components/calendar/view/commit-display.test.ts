import { describe, expect, it } from "vitest";
import type { CalendarEvent } from "$lib/calendar/types";
import { commitDisplay, NO_COMMIT_DISPLAY, type CommitDisplaySlots } from "./commit-display";

function event(id: string): CalendarEvent {
  return { id, title: id, start: "2026-07-12 09:00", end: "2026-07-12 10:00", timezone: "UTC", calendarId: "calendar-a" };
}

describe("commitDisplay", () => {
  it("shows the live store while nothing commits", () => {
    expect(commitDisplay(NO_COMMIT_DISPLAY)).toEqual({
      panelHidden: false, suppressGlow: false, suppressPreview: false, frozenEvents: null,
    });
  });

  it("freezes the save's projection and hides the glow while a save commits", () => {
    const frozen = [event("saved")];
    expect(commitDisplay({ save: { frozenEvents: frozen }, delete: null })).toEqual({
      panelHidden: false, suppressGlow: true, suppressPreview: true, frozenEvents: frozen,
    });
  });

  it("hides the panel and freezes the deletion's projection while a deletion commits", () => {
    const frozen = [event("kept")];
    expect(commitDisplay({ save: null, delete: { frozenEvents: frozen } })).toEqual({
      panelHidden: true, suppressGlow: false, suppressPreview: true, frozenEvents: frozen,
    });
  });

  it("keeps a running save frozen after a deletion beside it ends", () => {
    const saving = [event("saved")];
    const both: CommitDisplaySlots = { save: { frozenEvents: saving }, delete: { frozenEvents: [event("kept")] } };
    expect(commitDisplay(both).frozenEvents).toEqual([event("kept")]);
    const afterDelete = commitDisplay({ ...both, delete: null });
    expect(afterDelete.frozenEvents).toBe(saving);
    expect(afterDelete.suppressPreview).toBe(true);
  });

  it("keeps a running deletion frozen and hidden after a save beside it ends", () => {
    const deleting = [event("kept")];
    const afterSave = commitDisplay({ save: null, delete: { frozenEvents: deleting } });
    expect(afterSave).toMatchObject({ panelHidden: true, suppressPreview: true, frozenEvents: deleting });
  });
});
