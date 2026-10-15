import { describe, expect, it } from "vitest";
import {
  APP_VIEWS,
  availableAppViews,
  isDetachableTabView,
  isView,
  isViewAvailable,
  mainTabViews,
  parseInitialViewSearch,
  viewLabel,
} from "./navigation";
import { platformProfileFor } from "./platform";

describe("navigation helpers", () => {
  it("accepts registered views only", () => {
    expect(isView("calendar")).toBe(true);
    expect(isView("chat")).toBe(true);
    expect(isView("music")).toBe(false);
    expect(isView("settings")).toBe(false);
    expect(isView(null)).toBe(false);
  });

  it("limits detached windows to title bar tabs", () => {
    expect(isDetachableTabView("calendar")).toBe(true);
    expect(isDetachableTabView("projects")).toBe(true);
    expect(isDetachableTabView("notes")).toBe(true);
    expect(isDetachableTabView("chat")).toBe(true);
    expect(isDetachableTabView("music")).toBe(false);
  });

  it("parses an initial view from the window search string", () => {
    for (const view of APP_VIEWS) {
      expect(parseInitialViewSearch(`?view=${view}`)).toBe(view);
    }
    expect(parseInitialViewSearch("?view=music")).toBeUndefined();
    expect(parseInitialViewSearch("?view=settings")).toBeUndefined();
    expect(parseInitialViewSearch("")).toBeUndefined();
  });

  it("exposes portable Android views without desktop window controls", () => {
    const android = platformProfileFor("android");

    expect(availableAppViews(android)).toEqual(["calendar", "projects", "notes", "chat"]);
    expect(isViewAvailable("notes", android)).toBe(true);
    expect(isViewAvailable("chat", android)).toBe(true);
    expect(parseInitialViewSearch("?view=notes", android)).toBe("notes");
    expect(parseInitialViewSearch("?view=chat", android)).toBe("chat");
    expect(mainTabViews(new Set(), android)).toEqual(["calendar", "projects", "notes", "chat"]);
  });

  it("provides user-facing labels for every view", () => {
    expect(viewLabel("calendar")).toBe("Calendar");
    expect(viewLabel("projects")).toBe("Projects");
    expect(viewLabel("notes")).toBe("Notes");
    expect(viewLabel("chat")).toBe("Chat");
  });

  it("removes detached tabs from the main tab list", () => {
    expect(mainTabViews(new Set(["calendar", "notes"]))).toEqual(["projects", "chat"]);
  });
});
