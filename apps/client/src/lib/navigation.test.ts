import { describe, expect, it } from "vitest";
import {
  APP_VIEWS,
  availableAppViews,
  isView,
  isViewAvailable,
  parseInitialViewSearch,
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
  });
});
