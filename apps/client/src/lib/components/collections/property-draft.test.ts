import { describe, expect, it } from "vitest";
import { collectionPropertyDraftHeaderBox } from "./property-draft";

const anchorRect = { top: 120.4, bottom: 156.4, left: 300, right: 460 };

describe("collection property draft header placement", () => {
  it("starts on the add cell and keeps the header row height", () => {
    expect(collectionPropertyDraftHeaderBox({ anchorRect, placement: "start", viewportWidth: 1200, preferredWidth: 240 }))
      .toEqual({ left: 300, top: 120, width: 240, height: 36 });
  });

  it("opens at the column boundary on the inserted side", () => {
    expect(collectionPropertyDraftHeaderBox({ anchorRect, placement: "after", viewportWidth: 1200, preferredWidth: 240 }).left).toBe(460);
    expect(collectionPropertyDraftHeaderBox({ anchorRect, placement: "before", viewportWidth: 1200, preferredWidth: 240 }).left).toBe(60);
  });

  it("shifts inside the viewport at either edge", () => {
    expect(collectionPropertyDraftHeaderBox({ anchorRect: { ...anchorRect, left: 100 }, placement: "before", viewportWidth: 1200, preferredWidth: 240 }).left).toBe(8);
    expect(collectionPropertyDraftHeaderBox({ anchorRect, placement: "after", viewportWidth: 600, preferredWidth: 240 }).left).toBe(352);
  });

  it("narrows to the viewport when the preferred width does not fit", () => {
    expect(collectionPropertyDraftHeaderBox({ anchorRect, placement: "start", viewportWidth: 200, preferredWidth: 240 }))
      .toMatchObject({ left: 8, width: 184 });
  });
});
