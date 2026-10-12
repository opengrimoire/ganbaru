import { describe, expect, it, vi } from "vitest";
import { EventPanelDateTimeController } from "./date-time-controller.svelte";

describe("EventPanelDateTimeController", () => {
  it("restores timed values after toggling all-day and keeps cross-midnight dates", () => {
    const draft = {
      allDay: false,
      startDate: "2026-07-12",
      startTime: "23:30",
      endDate: "2026-07-13",
      endTime: "00:30",
    };
    const emitChange = vi.fn();
    const controller = new EventPanelDateTimeController({
      draft,
      controlsDisabled: () => false,
      lockStartControls: () => false,
      timeFormat: () => "24h",
      emitChange,
    });

    controller.toggleAllDay();
    expect(draft).toMatchObject({ allDay: true, startTime: "00:00", endTime: "00:00" });
    controller.toggleAllDay();
    expect(draft).toMatchObject({
      allDay: false,
      startTime: "23:30",
      endTime: "00:30",
      endDate: "2026-07-13",
    });
    expect(emitChange).toHaveBeenCalledTimes(2);
  });

  it("does not mutate the locked start time", () => {
    const draft = {
      allDay: false,
      startDate: "2026-07-12",
      startTime: "09:00",
      endDate: "2026-07-12",
      endTime: "10:00",
    };
    const controller = new EventPanelDateTimeController({
      draft,
      controlsDisabled: () => false,
      lockStartControls: () => true,
      timeFormat: () => "24h",
      emitChange: vi.fn(),
    });
    controller.startTimeDraft = "11:00";
    controller.startTimeDraftEdited = true;

    expect(controller.commitTimeInput("start")).toBe(false);
    expect(draft.startTime).toBe("09:00");
  });
});
