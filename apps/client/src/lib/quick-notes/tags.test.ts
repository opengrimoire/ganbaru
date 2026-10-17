import { describe, expect, it } from "vitest";
import {
  compareQuickNoteTags,
  pickQuickNoteTagColor,
  quickNoteRelativeViewTagId,
  quickNoteTagMoveRequest,
  quickNoteTagVisibleCount,
  quickNoteViewIndexForKey,
  quickNoteViewShortcut,
} from "$lib/quick-notes/tags";

describe("Quick notes tag shortcuts", () => {
  it("maps All and nine tag views to 1 through 9 and then 0", () => {
    expect(Array.from({ length: 10 }, (_, index) => quickNoteViewShortcut(index)))
      .toEqual(["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"]);
  });

  it("maps numeric keys back to view positions", () => {
    expect(quickNoteViewIndexForKey("1")).toBe(0);
    expect(quickNoteViewIndexForKey("9")).toBe(8);
    expect(quickNoteViewIndexForKey("0")).toBe(9);
  });

  it("rejects unsupported positions and keys", () => {
    expect(quickNoteViewShortcut(-1)).toBeNull();
    expect(quickNoteViewShortcut(10)).toBeNull();
    expect(quickNoteViewShortcut(1.5)).toBeNull();
    expect(quickNoteViewIndexForKey("Digit1")).toBeNull();
    expect(quickNoteViewIndexForKey("a")).toBeNull();
  });
});

describe("Quick notes view stepping", () => {
  const tagIds = ["work", "home", "ideas"];

  it("steps from All through the tags and wraps at both ends", () => {
    expect(quickNoteRelativeViewTagId(tagIds, null, 1)).toBe("work");
    expect(quickNoteRelativeViewTagId(tagIds, "home", 1)).toBe("ideas");
    expect(quickNoteRelativeViewTagId(tagIds, "ideas", 1)).toBeNull();
    expect(quickNoteRelativeViewTagId(tagIds, null, -1)).toBe("ideas");
    expect(quickNoteRelativeViewTagId(tagIds, "work", -1)).toBeNull();
  });

  it("enters the views from Archive or Trash at All going forward and at the last view going back", () => {
    expect(quickNoteRelativeViewTagId(tagIds, undefined, 1)).toBeNull();
    expect(quickNoteRelativeViewTagId(tagIds, undefined, -1)).toBe("ideas");
  });

  it("treats a tag that no longer exists like being outside the views", () => {
    expect(quickNoteRelativeViewTagId(tagIds, "deleted", 1)).toBeNull();
    expect(quickNoteRelativeViewTagId(tagIds, "deleted", -1)).toBe("ideas");
  });

  it("stays on All when there are no tags", () => {
    expect(quickNoteRelativeViewTagId([], null, 1)).toBeNull();
    expect(quickNoteRelativeViewTagId([], null, -1)).toBeNull();
  });
});

describe("Quick notes tag order", () => {
  it("sorts by order key in byte order, then by id", () => {
    const tags = [
      { id: "b", orderKey: "a1" },
      { id: "c", orderKey: "a0" },
      { id: "a", orderKey: "a0" },
      { id: "d", orderKey: "Zz" },
      { id: "e", orderKey: "a0V" },
    ];
    expect(tags.sort(compareQuickNoteTags).map((tag) => tag.id)).toEqual(["d", "a", "c", "e", "b"]);
  });
});

describe("Quick notes tag colors", () => {
  const allSlots = Array.from({ length: 32 }, (_, index) => index);

  it("never picks the default color while another slot is free", () => {
    for (const roll of [0, 0.25, 0.5, 0.999]) {
      expect(pickQuickNoteTagColor([], 30, () => roll)).not.toBe(30);
    }
  });

  it("picks only colors no other tag uses", () => {
    const used = allSlots.filter((color) => color !== 3 && color !== 7 && color !== 30);
    expect(pickQuickNoteTagColor(used, 30, () => 0)).toBe(3);
    expect(pickQuickNoteTagColor(used, 30, () => 0.999)).toBe(7);
  });

  it("reuses colors but still avoids the default once every slot is taken", () => {
    const picked = new Set([0, 0.3, 0.6, 0.999].map((roll) => pickQuickNoteTagColor(allSlots, 30, () => roll)));
    expect(picked.has(30)).toBe(false);
    expect(picked.size).toBeGreaterThan(1);
  });

  it("follows a changed default color", () => {
    expect(pickQuickNoteTagColor([], 0, () => 0)).toBe(1);
  });
});

describe("Quick notes tag moves", () => {
  const ids = ["a", "b", "c", "d"];

  it("names the new neighbors of a moved tag", () => {
    expect(quickNoteTagMoveRequest(ids, "a", 2)).toEqual({ id: "a", previousId: "c", nextId: "d" });
    expect(quickNoteTagMoveRequest(ids, "d", 0)).toEqual({ id: "d", previousId: null, nextId: "a" });
    expect(quickNoteTagMoveRequest(ids, "b", 3)).toEqual({ id: "b", previousId: "d", nextId: null });
  });

  it("returns null when the tag stays or is missing", () => {
    expect(quickNoteTagMoveRequest(ids, "b", 1)).toBeNull();
    expect(quickNoteTagMoveRequest(ids, "z", 0)).toBeNull();
  });
});

describe("Quick notes tag row overflow", () => {
  it("shows every tag when the row fits", () => {
    expect(quickNoteTagVisibleCount([40, 40, 40], 128, 4, 30)).toBe(3);
    expect(quickNoteTagVisibleCount([], 0, 4, 30)).toBe(0);
  });

  it("keeps room for the overflow button once a tag does not fit", () => {
    expect(quickNoteTagVisibleCount([40, 40, 40], 127, 4, 30)).toBe(2);
    expect(quickNoteTagVisibleCount([40, 40, 40], 77, 4, 30)).toBe(1);
    expect(quickNoteTagVisibleCount([40, 40, 40], 73, 4, 30)).toBe(0);
  });

  it("stops at the first tag that does not fit, so shown tags stay a prefix", () => {
    expect(quickNoteTagVisibleCount([40, 200, 10], 100, 4, 30)).toBe(1);
  });
});
