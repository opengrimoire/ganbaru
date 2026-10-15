import { describe, expect, it } from "vitest";
import type { View } from "$lib/navigation";
import { createWorkspaceTab, kindLocation, type WorkspaceTab } from "$lib/workspace-tabs/model";
import {
  MAIN_WINDOW_LABEL,
  createTabWindowLabel,
  dragEndedOutsideWindow,
  isTabWindowLabel,
  parseTabWindowDrag,
  parseTabWindowRelease,
  parseTabWindowRequest,
  parseTabWindowSearch,
  parseTabWindowTransfer,
  tabWindowDragData,
  tabWindowUrl,
  tearOffWindowPosition,
} from "./tab-windows";

const KINDS: readonly View[] = ["calendar", "projects", "notes", "chat"];
const SECONDARY = "view-11111111-1111-4111-8111-111111111111";

function tab(id: string, kind: View): WorkspaceTab {
  return createWorkspaceTab(id, kindLocation(kind));
}

describe("tab window labels", () => {
  it("creates labels the desktop capability grants", () => {
    const label = createTabWindowLabel(() => "11111111-1111-4111-8111-111111111111");
    expect(label).toBe(SECONDARY);
    expect(isTabWindowLabel(label)).toBe(true);
  });

  it("rejects the main label and labels outside the granted pattern", () => {
    expect(isTabWindowLabel(MAIN_WINDOW_LABEL)).toBe(false);
    expect(isTabWindowLabel("view-")).toBe(false);
    expect(isTabWindowLabel("view-a/b")).toBe(false);
    expect(isTabWindowLabel(`view-${"a".repeat(65)}`)).toBe(false);
    expect(isTabWindowLabel("pomodoro-overlay-1")).toBe(false);
    expect(isTabWindowLabel(null)).toBe(false);
  });
});

describe("tab window address", () => {
  it("round-trips the tab a window opens with", () => {
    const notes = tab("tab-1", "notes");
    const url = tabWindowUrl(notes);
    const search = url.slice(url.indexOf("?"));
    expect(new URLSearchParams(search).get("view")).toBe("notes");
    expect(parseTabWindowSearch(search, KINDS)).toEqual(notes);
  });

  it("opens an empty address when there is no tab", () => {
    expect(tabWindowUrl(null)).toBe("/");
    expect(parseTabWindowSearch("", KINDS)).toBeNull();
  });

  it("ignores malformed tabs and kinds this window cannot show", () => {
    expect(parseTabWindowSearch("?tab=%7B", KINDS)).toBeNull();
    expect(parseTabWindowSearch("?tab=%7B%7D", KINDS)).toBeNull();
    const chat = tabWindowUrl(tab("tab-1", "chat"));
    expect(parseTabWindowSearch(chat.slice(chat.indexOf("?")), ["calendar", "notes"])).toBeNull();
  });
});

describe("tab window messages", () => {
  const route = { targetLabel: MAIN_WINDOW_LABEL, sourceLabel: SECONDARY, vaultId: "vault-1" };

  it("accepts a transfer between two windows", () => {
    const notes = tab("tab-1", "notes");
    expect(parseTabWindowTransfer({ ...route, tab: notes }, KINDS)).toEqual({ ...route, tab: notes });
  });

  it("rejects messages a window sends to itself or without a vault", () => {
    const notes = tab("tab-1", "notes");
    expect(parseTabWindowTransfer({ ...route, sourceLabel: MAIN_WINDOW_LABEL, tab: notes }, KINDS)).toBeNull();
    expect(parseTabWindowTransfer({ ...route, vaultId: "", tab: notes }, KINDS)).toBeNull();
    expect(parseTabWindowRelease({ ...route, targetLabel: SECONDARY, tabId: "tab-1" })).toBeNull();
  });

  it("rejects messages from windows outside the granted labels", () => {
    expect(parseTabWindowRelease({ ...route, sourceLabel: "settings", tabId: "tab-1" })).toBeNull();
    expect(parseTabWindowRelease({ ...route, targetLabel: "pomodoro-overlay-1", tabId: "tab-1" })).toBeNull();
  });

  it("rejects a transfer whose tab is invalid or unavailable", () => {
    expect(parseTabWindowTransfer({ ...route, tab: { id: "tab-1" } }, KINDS)).toBeNull();
    expect(parseTabWindowTransfer({ ...route, tab: tab("tab-1", "chat") }, ["calendar"])).toBeNull();
  });

  it("requires a tab id on a release", () => {
    expect(parseTabWindowRelease({ ...route, tabId: "tab-1" })).toEqual({ ...route, tabId: "tab-1" });
    expect(parseTabWindowRelease({ ...route, tabId: "" })).toBeNull();
    expect(parseTabWindowRelease({ ...route })).toBeNull();
  });
});

describe("tab window requests", () => {
  const route = { targetLabel: SECONDARY, sourceLabel: MAIN_WINDOW_LABEL, vaultId: "vault-1" };

  it("asks for a dragged tab by id", () => {
    expect(parseTabWindowRequest({ ...route, tabId: "tab-1" })).toEqual({ ...route, tabId: "tab-1" });
    expect(parseTabWindowRequest({ ...route, tabId: 1 })).toBeNull();
    expect(parseTabWindowRequest({ ...route, targetLabel: MAIN_WINDOW_LABEL, tabId: "tab-1" })).toBeNull();
  });
});

describe("tab window drags", () => {
  const drag = { sourceLabel: SECONDARY, vaultId: "vault-1", tabId: "tab-1" };

  it("reads a tab dragged in from another window", () => {
    expect(parseTabWindowDrag(tabWindowDragData(drag), MAIN_WINDOW_LABEL)).toEqual(drag);
  });

  it("ignores drags from the same window", () => {
    expect(parseTabWindowDrag(tabWindowDragData(drag), SECONDARY)).toBeNull();
  });

  it("rejects malformed drag data", () => {
    expect(parseTabWindowDrag("", MAIN_WINDOW_LABEL)).toBeNull();
    expect(parseTabWindowDrag("{", MAIN_WINDOW_LABEL)).toBeNull();
    expect(parseTabWindowDrag(tabWindowDragData({ ...drag, sourceLabel: "settings" }), MAIN_WINDOW_LABEL)).toBeNull();
    expect(parseTabWindowDrag(tabWindowDragData({ ...drag, vaultId: "" }), MAIN_WINDOW_LABEL)).toBeNull();
    expect(parseTabWindowDrag(tabWindowDragData({ ...drag, tabId: "" }), MAIN_WINDOW_LABEL)).toBeNull();
  });
});

describe("tab tear-off", () => {
  const viewport = { width: 800, height: 600 };
  const inside = { clientX: 120, clientY: 14, screenX: 420, screenY: 214 };

  it("treats a drag whose end point lies outside the viewport as having left the window", () => {
    expect(dragEndedOutsideWindow(inside, viewport, 1)).toBe(false);
    expect(dragEndedOutsideWindow({ ...inside, clientY: 640, screenY: 840 }, viewport, 1)).toBe(true);
    expect(dragEndedOutsideWindow({ ...inside, clientX: -30, screenX: 270 }, viewport, 1)).toBe(true);
    expect(dragEndedOutsideWindow({ ...inside, clientX: 800 }, viewport, 1)).toBe(true);
  });

  it("trusts the drag entry count even when the end point looks inside", () => {
    expect(dragEndedOutsideWindow(inside, viewport, 0)).toBe(true);
  });

  it("uses the drag entry count alone when the engine reports no end point", () => {
    const unreported = { clientX: 0, clientY: 0, screenX: 0, screenY: 0 };
    expect(dragEndedOutsideWindow(unreported, viewport, 0)).toBe(true);
    expect(dragEndedOutsideWindow(unreported, viewport, 2)).toBe(false);
  });

  it("places a torn-off window so the pointer sits over its tab strip", () => {
    expect(tearOffWindowPosition(500.4, 300.6)).toEqual({ x: 404, y: 285 });
  });

  it("opens centered when the drag end has no screen position", () => {
    expect(tearOffWindowPosition(0, 0)).toBeNull();
    expect(tearOffWindowPosition(Number.NaN, 10)).toBeNull();
  });
});
