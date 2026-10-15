import { describe, expect, it } from "vitest";
import {
  detachedViewWindowLabel,
  detachedViewWindowTitle,
  detachedViewWindowUrl,
  detachableTabViewFromWindowLabel,
  isDetachedViewReattachRequest,
  parseDetachedViewDragPayload,
  parseDetachedViewOpenLocationRequest,
  parseDetachedViewWindowChange,
  parseDetachedViewWindowTab,
  serializeDetachedViewDragPayload,
} from "./detached";

describe("detached view windows", () => {
  it("uses stable labels for app-owned popout windows", () => {
    expect(detachedViewWindowLabel("calendar")).toBe("view-calendar");
    expect(detachedViewWindowLabel("projects")).toBe("view-projects");
    expect(detachedViewWindowLabel("notes")).toBe("view-notes");
    expect(detachedViewWindowLabel("chat")).toBe("view-chat");
  });

  it("opens a local app route with the requested initial view", () => {
    expect(detachedViewWindowUrl("calendar")).toBe("/?view=calendar");
    expect(detachedViewWindowUrl("projects")).toBe("/?view=projects");
    expect(detachedViewWindowUrl("notes")).toBe("/?view=notes");
    expect(detachedViewWindowUrl("chat")).toBe("/?view=chat");
  });

  it("uses view-specific window titles", () => {
    expect(detachedViewWindowTitle("calendar")).toBe("Ganbaru AI: Calendar");
    expect(detachedViewWindowTitle("projects")).toBe("Ganbaru AI: Projects");
    expect(detachedViewWindowTitle("notes")).toBe("Ganbaru AI: Notes");
    expect(detachedViewWindowTitle("chat")).toBe("Ganbaru AI: Chat");
  });

  it("parses detachable views from detached window labels only", () => {
    expect(detachableTabViewFromWindowLabel("view-calendar")).toBe("calendar");
    expect(detachableTabViewFromWindowLabel("view-chat")).toBe("chat");
    expect(detachableTabViewFromWindowLabel("view-music")).toBeUndefined();
    expect(detachableTabViewFromWindowLabel("main")).toBeUndefined();
  });

  it("round-trips detached tab drag payloads", () => {
    const payload = serializeDetachedViewDragPayload("calendar", "view-calendar");
    expect(parseDetachedViewDragPayload(payload)).toEqual({
      view: "calendar",
      sourceLabel: "view-calendar",
    });
  });

  it("rejects mismatched detached tab drag payloads", () => {
    expect(parseDetachedViewDragPayload("not-json")).toBeUndefined();
    expect(parseDetachedViewDragPayload(JSON.stringify({
      view: "calendar",
      sourceLabel: "main",
    }))).toBeUndefined();
    expect(parseDetachedViewDragPayload(JSON.stringify({
      view: "music",
      sourceLabel: "view-music",
    }))).toBeUndefined();
  });

  it("validates reattach requests", () => {
    expect(isDetachedViewReattachRequest({ view: "notes" })).toBe(true);
    expect(isDetachedViewReattachRequest({ view: "music" })).toBe(false);
    expect(isDetachedViewReattachRequest(null)).toBe(false);
  });

  it("carries the moved tab in the window route and reads it back for the same kind only", () => {
    const tab = { tabId: "tab-1", location: { kind: "projects" as const, projectId: "p-1", item: null } };
    const url = detachedViewWindowUrl("projects", tab);
    const search = url.slice(1);

    expect(new URLSearchParams(search).get("view")).toBe("projects");
    expect(parseDetachedViewWindowTab(search, "projects")).toEqual(tab);
    expect(parseDetachedViewWindowTab(search, "notes")).toBeNull();
    expect(parseDetachedViewWindowTab("?view=projects&tab=tab-1&location=%7B", "projects")).toBeNull();
    expect(parseDetachedViewWindowTab("?view=projects", "projects")).toBeNull();
  });

  it("validates window changes and drops tab details from another kind", () => {
    expect(parseDetachedViewWindowChange({ view: "notes", detached: true, tabId: "tab-1" })).toEqual({
      view: "notes",
      detached: true,
      tabId: "tab-1",
    });
    expect(parseDetachedViewWindowChange({
      view: "chat",
      detached: false,
      tabId: "tab-2",
      location: { kind: "chat", projectId: "p-1", item: { type: "chat-channel", id: "c-1" } },
      closed: true,
    })).toEqual({
      view: "chat",
      detached: false,
      tabId: "tab-2",
      location: { kind: "chat", projectId: "p-1", item: { type: "chat-channel", id: "c-1" } },
      closed: true,
    });
    expect(parseDetachedViewWindowChange({
      view: "chat",
      detached: false,
      tabId: "",
      location: { kind: "notes", projectId: null, item: null },
      closed: "yes",
    })).toEqual({ view: "chat", detached: false });
    expect(parseDetachedViewWindowChange({ view: "music", detached: true })).toBeNull();
    expect(parseDetachedViewWindowChange({ view: "notes" })).toBeNull();
  });

  it("validates location requests for detached windows", () => {
    const location = { kind: "notes", projectId: null, item: { type: "notes-page", id: "page-1" } };

    expect(parseDetachedViewOpenLocationRequest({ view: "notes", location })).toEqual({ view: "notes", location });
    expect(parseDetachedViewOpenLocationRequest({ view: "projects", location })).toBeNull();
    expect(parseDetachedViewOpenLocationRequest({ view: "notes", location: null })).toBeNull();
  });
});
