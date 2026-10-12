import { describe, expect, it, vi } from "vitest";
import { NotesPageSessionController } from "./page-session.svelte";

const pageA = "10000000-0000-4000-8000-000000000001";
const pageB = "10000000-0000-4000-8000-000000000002";

function controller() {
  return new NotesPageSessionController({
    initialSelectedPageId: null,
    persistSelectedPageId: vi.fn(),
    recordRecentPage: vi.fn(),
  });
}

describe("notes page session", () => {
  it("rejects stale overlapping page results and recent-page writes", () => {
    const session = controller();
    const requestA = session.open(pageA, "full");
    const requestB = session.open(pageB, "side");

    expect(session.isCurrent(requestA, pageA)).toBe(false);
    expect(session.recordRecentIfCurrent(requestA, pageA)).toBe(false);
    expect(session.isCurrent(requestB, pageB)).toBe(true);
    expect(session.recordRecentIfCurrent(requestB, pageB)).toBe(true);
    expect(session.selectedPageId).toBe(pageB);
    expect(session.pageOpenMode).toBe("side");
  });

  it("does not apply stale breadcrumbs after workspace invalidation", () => {
    const session = controller();
    const request = session.open(pageA, "full");
    session.invalidate();

    expect(session.applyBreadcrumbsIfCurrent(request, pageA, [{
      id: pageA,
      title: "Stale",
      current: true,
      status: "active",
    }])).toBe(false);
    expect(session.breadcrumbs).toEqual([]);
  });

  it("keeps title focus requests monotonic across selection changes", () => {
    const session = controller();
    session.open(pageA, "full");
    session.requestTitleFocus(pageA);
    session.open(pageB, "full");
    session.requestTitleFocus(pageB);

    expect(session.titleFocus).toEqual({ pageId: pageB, requestId: 2 });
  });
});
