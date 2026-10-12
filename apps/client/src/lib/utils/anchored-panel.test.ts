// @vitest-environment jsdom
import { describe, expect, it, vi } from "vitest";
import { anchoredPanelContentHeight, anchoredPanelStyle, anchoredPanelWidth, anchoredSidePanelStyle, anchoredSubmenuWidth, type AnchoredPanelInput, type AnchoredSidePanelInput } from "./anchored-panel";

const ANCHOR: AnchoredPanelInput = {
  triggerRect: { top: 40, right: 480, bottom: 72, left: 448 },
  viewportWidth: 800,
  viewportHeight: 600,
  preferredWidth: 320,
  preferredMaxHeight: 240,
};

describe("Anchored floating panel geometry", () => {
  it("preserves start alignment and supports end alignment for narrow settings triggers", () => {
    expect(anchoredPanelStyle(ANCHOR)).toContain("left:448px");
    expect(anchoredPanelStyle({ ...ANCHOR, horizontalAlign: "end" })).toContain("left:160px");
  });

  it("clamps both horizontal alignments inside a narrow viewport", () => {
    const input = { ...ANCHOR, viewportWidth: 280 };
    expect(anchoredPanelStyle(input)).toContain("left:8px");
    expect(anchoredPanelStyle(input)).toContain("width:264px");
    expect(anchoredPanelStyle({ ...input, horizontalAlign: "end" })).toContain("left:8px");
    expect(anchoredPanelWidth(input)).toBe(264);
  });

  it("opens above a low trigger while keeping the whole panel in bounds", () => {
    const style = anchoredPanelStyle({ ...ANCHOR, triggerRect: { top: 550, right: 480, bottom: 582, left: 448 } });
    expect(style).toContain("top:306px");
    expect(style).toContain("max-height:240px");
  });

  it("limits panel height to the available space when both directions are constrained", () => {
    const style = anchoredPanelStyle({ ...ANCHOR, viewportHeight: 160 });
    expect(style).toContain("top:76px");
    expect(style).toContain("max-height:76px");
  });

  it("keeps short content below the anchor without using the height limit as its actual height", () => {
    const style = anchoredPanelStyle({ ...ANCHOR, viewportHeight: 230, contentHeight: 82.75,
      triggerRect: { top: 100, right: 480, bottom: 128, left: 448 } });
    expect(style).toContain("top:132px");
    expect(style).toContain("max-height:90px");
  });

  it("positions short content above a low anchor while retaining room for natural sizing", () => {
    const style = anchoredPanelStyle({ ...ANCHOR, viewportHeight: 230, contentHeight: 82.75,
      triggerRect: { top: 170, right: 480, bottom: 198, left: 448 } });
    expect(style).toContain("top:83px");
    expect(style).toContain("max-height:158px");
  });

  it("measures fractional intrinsic content and borders without including overflowing descendants", () => {
    const panel = document.createElement("div");
    panel.style.borderTopWidth = "1.25px";
    panel.style.borderBottomWidth = "1.25px";
    const header = document.createElement("div");
    const content = document.createElement("div");
    panel.append(header, content);
    document.body.append(panel);
    vi.spyOn(header, "getBoundingClientRect").mockReturnValue(new DOMRect(0, 0, 320, 40.25));
    vi.spyOn(content, "getBoundingClientRect").mockReturnValue(new DOMRect(0, 0, 320, 80.25));
    vi.spyOn(content, "scrollHeight", "get").mockReturnValue(1_000);
    expect(anchoredPanelContentHeight(panel, [header, content])).toBe(123);
    panel.remove();
    vi.restoreAllMocks();
  });
});

const SUBMENU: AnchoredSidePanelInput = {
  triggerRect: { top: 200, right: 380, bottom: 228, left: 140 },
  viewportWidth: 1200,
  viewportHeight: 800,
  preferredWidth: 240,
  preferredMaxHeight: 560,
  contentHeight: 180,
  alignOffset: 7,
};

describe("Anchored submenu geometry", () => {
  it("opens beside its trigger row with its first row level with the trigger", () => {
    expect(anchoredSidePanelStyle(SUBMENU)).toBe("position:fixed; left:384px; top:193px; width:240px; max-height:560px");
  });

  it("flips to the left of its trigger row when the right side lacks room", () => {
    const style = anchoredSidePanelStyle({ ...SUBMENU, triggerRect: { top: 200, right: 1100, bottom: 228, left: 860 } });
    expect(style).toContain("left:616px");
  });

  it("opens below the trigger when neither side of its row has room", () => {
    const narrow = { ...SUBMENU, viewportWidth: 400 };
    expect(anchoredSidePanelStyle(narrow)).toBe(anchoredPanelStyle(narrow));
  });

  it("shifts a tall submenu up to stay inside the viewport and caps it to the viewport height", () => {
    expect(anchoredSidePanelStyle({ ...SUBMENU, triggerRect: { top: 700, right: 380, bottom: 728, left: 140 } })).toContain("top:612px");
    const tall = anchoredSidePanelStyle({ ...SUBMENU, contentHeight: 2_000, viewportHeight: 300 });
    expect(tall).toContain("top:8px");
    expect(tall).toContain("max-height:284px");
  });
});

describe("Anchored submenu width", () => {
  it("keeps a submenu narrower than the menu it opens from", () => {
    expect(anchoredSubmenuWidth(320, 240, 32)).toBe(208);
    expect(anchoredSubmenuWidth(240, 320, 32)).toBe(240);
  });

  it("keeps the preferred width while the parent menu has no layout width", () => {
    expect(anchoredSubmenuWidth(320, 0, 32)).toBe(320);
  });

  it("never returns a negative width for a parent narrower than the inset", () => {
    expect(anchoredSubmenuWidth(320, 20, 32)).toBe(0);
  });
});
