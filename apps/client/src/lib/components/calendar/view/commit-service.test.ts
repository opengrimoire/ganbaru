import { describe, expect, it, vi } from "vitest";
import type { CalendarEvent } from "$lib/calendar/types";
import type { EditSessionState } from "$lib/components/calendar/edit-session.svelte";
import { CalendarViewCommitService, calendarDataOnly } from "./commit-service";
import { computeViewWindow } from "$lib/calendar/utils";

function setup() {
  const original: CalendarEvent = { id: "event-a", recurrenceDate: "2026-07-11", title: "Event",
    start: "2026-07-12 00:00", end: "2026-07-12 01:00",
    timezone: "America/New_York", calendarId: "calendar-a" };
  let state: EditSessionState = { mode: "edit", sessionKey: 1,
    originalEvent: original, instanceEvent: original, templateId: original.id,
    detailsLoaded: true, anchor: { x: 0, y: 0, width: 0, height: 0 } };
  const commit = vi.fn();
  const directCommit = vi.fn();
  const acceptNativeEdit = vi.fn();
  let context: { vaultId: string; vaultGeneration: number } | null = { vaultId: "vault", vaultGeneration: 1 };
  const service = new CalendarViewCommitService({
    calendarStore: { acceptNativeEdit }, nativeEditor: { commit }, directEditor: { commit: directCommit },
    getSessionState: () => state, getBaseline: () => original, getScope: () => "all",
    getViewWindow: () => computeViewWindow(new Date(2026, 6, 12), "week"),
    getContext: () => context, getRenderZone: () => "Asia/Tokyo",
  });
  return { original, commit, directCommit, acceptNativeEdit, service,
    setState: (value: EditSessionState) => { state = value; },
    clearContext: () => { context = null; } };
}

describe("Calendar semantic persistence", () => {
  it("reviews creation and persists through the native owner without raw event insertion", async () => {
    const test = setup();
    test.setState({ mode: "create", sessionKey: 3, start: "2026-07-12 09:00", end: "2026-07-12 10:00",
      anchor: { x: 0, y: 0, width: 0, height: 0 } });
    await test.service.persist({ title: "New", start: "2026-07-12 09:15", end: "2026-07-12 10:15",
      description: "", linkedTaskIds: ["task-a"] });
    expect(test.commit).toHaveBeenCalledOnce();
    expect(test.acceptNativeEdit).toHaveBeenCalledOnce();
    expect(test.commit.mock.calls[0]?.[0]).toMatchObject({ vaultId: "vault", sessionKey: 3,
      edit: { kind: "create", draft: { timing: { startTime: "2026-07-12T09:15", endTime: "2026-07-12T10:15",
        timezone: "Asia/Tokyo", inputZone: "Asia/Tokyo" } } } });
    expect(JSON.stringify(test.commit.mock.calls[0]?.[0])).not.toContain("linkedTaskIds");
  });

  it("removes project task links from Calendar persistence", () => {
    expect(calendarDataOnly({ title: "Title", start: "2026-07-12 09:00",
      end: "2026-07-12 10:00", description: "", linkedTaskIds: ["task-a"] })).not.toHaveProperty("linkedTaskIds");
  });

  it("sends one semantic edit using the native home identity and current display zone", async () => {
    const test = setup();
    await test.service.persist({ ...test.original, title: "Updated", description: "", linkedTaskIds: ["task-a"] });
    expect(test.commit).toHaveBeenCalledOnce();
    expect(test.acceptNativeEdit).toHaveBeenCalledOnce();
    const draft = test.commit.mock.calls[0]?.[0];
    expect(draft).toMatchObject({ vaultId: "vault", sessionKey: 1,
      edit: { selection: { templateId: "event-a", recurrenceDate: "2026-07-11", scope: "all" } },
      window: { renderZone: "Asia/Tokyo" } });
    expect(JSON.stringify(draft)).not.toContain("linkedTaskIds");
  });

  it("passes End now without a caller cutoff and forces the selected occurrence", async () => {
    const test = setup();
    await test.service.persist({ ...test.original, end: "2026-07-12 00:30", description: "" }, "all", { action: "end_now" });
    expect(test.commit.mock.calls[0]?.[0].edit).toMatchObject({
      action: "end_now", selection: { scope: "this" }, draft: { timing: {} },
    });
  });

  it("sends immediate drag geometry through the same native authority with a distinct editor", async () => {
    const test = setup();
    await test.service.persistDirect({ ...test.original, start: "2026-07-12 00:15" }, test.original);
    expect(test.commit).not.toHaveBeenCalled();
    expect(test.directCommit.mock.calls[0]?.[0]).toMatchObject({
      edit: { selection: { recurrenceDate: "2026-07-11", scope: "this" },
        draft: { timing: { startTime: "2026-07-12T00:15", inputZone: "Asia/Tokyo" } } },
    });
  });

  it("rejects edits before native context is available", async () => {
    const test = setup();
    test.clearContext();
    await expect(test.service.persist({ ...test.original, description: "" })).rejects.toThrow("context");
    expect(test.commit).not.toHaveBeenCalled();
  });

  it("keeps caches valid while a Save has no accepted receipt", async () => {
    const test = setup();
    test.commit.mockRejectedValueOnce(new Error("Response lost"));
    await expect(test.service.persist({ ...test.original, description: "" })).rejects.toThrow("Response lost");
    expect(test.acceptNativeEdit).not.toHaveBeenCalled();
  });

  it("does not persist after the editor closes", async () => {
    const test = setup();
    test.setState({ mode: "closed" });
    await test.service.persist({ ...test.original, description: "" });
    expect(test.commit).not.toHaveBeenCalled();
  });
});
