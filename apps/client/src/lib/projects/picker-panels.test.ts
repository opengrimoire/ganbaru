import { describe, expect, it } from "vitest";
import {
  projectPickerBridgeFrameStyle,
  projectPickerMobileBackAction,
  projectPickerMobilePane,
  projectPickerMenuAimRect,
  projectPickerPanelEstimatedListHeight,
  projectPickerPanelFrameStyle,
  projectPickerPanelHeight,
  projectPickerPointerPoint,
  projectPickerScrollState,
  projectPickerSubpanelAimOrigin,
  projectPickerSubpanelGeometry,
  projectPickerSubpanelSide,
  type ProjectPickerPanelRect,
} from "./picker-panels";

function rect(input: ProjectPickerPanelRect): ProjectPickerPanelRect {
  return input;
}

describe("project picker panel helpers", () => {
  it("uses one compact pane for groups, a selected group's projects, and search", () => {
    expect(projectPickerMobilePane({
      mode: "groups",
      activeGroupId: null,
      searchActive: false,
    })).toBe("groups");
    expect(projectPickerMobilePane({
      mode: "groups",
      activeGroupId: "group-1",
      searchActive: false,
    })).toBe("projects");
    expect(projectPickerMobilePane({
      mode: "projects",
      activeGroupId: null,
      searchActive: false,
    })).toBe("projects");
    expect(projectPickerMobilePane({
      mode: "groups",
      activeGroupId: "group-1",
      searchActive: true,
    })).toBe("search");
  });

  it("closes compact nested layers before the picker surface", () => {
    expect(projectPickerMobileBackAction({
      mode: "groups",
      activeGroupId: "group-1",
      searchActive: false,
      createGroupOpen: false,
      createProjectGroupId: "group-1",
    })).toBe("close-project-creator");
    expect(projectPickerMobileBackAction({
      mode: "groups",
      activeGroupId: null,
      searchActive: false,
      createGroupOpen: true,
      createProjectGroupId: null,
    })).toBe("close-group-creator");
    expect(projectPickerMobileBackAction({
      mode: "groups",
      activeGroupId: "group-1",
      searchActive: true,
      createGroupOpen: false,
      createProjectGroupId: null,
    })).toBe("clear-search");
    expect(projectPickerMobileBackAction({
      mode: "groups",
      activeGroupId: "group-1",
      searchActive: false,
      createGroupOpen: false,
      createProjectGroupId: null,
    })).toBe("show-groups");
    expect(projectPickerMobileBackAction({
      mode: "groups",
      activeGroupId: null,
      searchActive: false,
      createGroupOpen: false,
      createProjectGroupId: null,
    })).toBe("close-picker");
  });

  it("estimates and caps main panel height", () => {
    expect(projectPickerPanelEstimatedListHeight({
      itemCount: 10,
      visibleRows: 6,
      listPadding: 6,
      rowHeight: 30,
    })).toBe(186);

    expect(projectPickerPanelHeight({
      headerHeight: 40,
      footerHeight: 44,
      listHeight: 300,
      maxHeight: null,
      visibleRows: 6,
      listPadding: 6,
      rowHeight: 30,
    })).toBe(270);

    expect(projectPickerPanelHeight({
      headerHeight: 40,
      footerHeight: 44,
      listHeight: 300,
      maxHeight: null,
      visibleRows: null,
      listPadding: 6,
      rowHeight: 30,
    })).toBe(384);

    expect(projectPickerPanelHeight({
      headerHeight: 40,
      footerHeight: 44,
      listHeight: 300,
      maxHeight: 200,
      visibleRows: null,
      listPadding: 6,
      rowHeight: 30,
    })).toBe(200);
  });

  it("places project subpanels to the right when room is available", () => {
    const geometry = projectPickerSubpanelGeometry({
      anchorRect: rect({ left: 100, right: 140, top: 60, bottom: 92, width: 40, height: 32 }),
      panelRect: rect({ left: 20, right: 220, top: 20, bottom: 420, width: 200, height: 400 }),
      bounds: { left: 0, right: 500, top: 0, bottom: 400 },
      gap: 4,
      footerHeight: 44,
      projectCount: 3,
      visibleRows: 4,
      listPadding: 8,
      rowHeight: 32,
    });

    expect(geometry.openRight).toBe(true);
    expect(geometry.panel).toEqual({
      left: 144,
      top: 60,
      width: 200,
      height: 148,
      maxHeight: 180,
    });
    expect(geometry.bridge).toEqual({
      left: 140,
      top: 60,
      width: 4,
      height: 148,
    });
  });

  it("places project subpanels to the left when the right side is tighter", () => {
    const geometry = projectPickerSubpanelGeometry({
      anchorRect: rect({ left: 210, right: 240, top: 80, bottom: 112, width: 30, height: 32 }),
      panelRect: rect({ left: 60, right: 240, top: 20, bottom: 420, width: 180, height: 400 }),
      bounds: { left: 0, right: 300, top: 0, bottom: 400 },
      gap: 4,
      footerHeight: 44,
      projectCount: 2,
      visibleRows: 4,
      listPadding: 8,
      rowHeight: 32,
    });

    expect(geometry.openRight).toBe(false);
    expect(geometry.panel.left).toBe(26);
    expect(geometry.bridge).toEqual({
      left: 206,
      top: 80,
      width: 4,
      height: 116,
    });
  });

  it.each([
    { panelHeight: 97.5, height: 98, top: 302 },
    { panelHeight: 800, height: 400, top: 0 },
  ])("fits a rendered submenu of height $panelHeight within the viewport", ({ panelHeight, height, top }) => {
    const geometry = projectPickerSubpanelGeometry({
      anchorRect: rect({ left: 100, right: 140, top: 310, bottom: 340, width: 40, height: 30 }),
      panelRect: rect({ left: 20, right: 220, top: 20, bottom: 380, width: 200, height: 360 }),
      bounds: { left: 0, right: 500, top: 0, bottom: 400 },
      gap: 4,
      footerHeight: 84,
      projectCount: 3,
      visibleRows: null,
      listPadding: 8,
      rowHeight: 32,
      panelHeight,
    });

    expect(geometry.panel.height).toBe(height);
    expect(geometry.panel.top).toBe(top);
    expect(geometry.panel.top + geometry.panel.height).toBe(400);
    expect(geometry.bridge.top).toBe(top);
    expect(geometry.bridge.height).toBe(height);
  });

  it("formats panel and bridge frames for inline styles", () => {
    expect(projectPickerPanelFrameStyle({
      left: 10.4,
      top: 20.5,
      width: 99.6,
      height: 50.1,
      maxHeight: 80.9,
    })).toBe("left: 10px; top: 21px; width: 100px; height: 50px; max-height: 81px");

    expect(projectPickerBridgeFrameStyle({
      left: 5.4,
      top: 6.5,
      width: -1,
      height: 3.4,
    })).toBe("left: 5px; top: 7px; width: 0px; height: 3px");
  });

  it("derives scroll affordance state from scroll metrics", () => {
    expect(projectPickerScrollState({ scrollHeight: 100, clientHeight: 100, scrollTop: 0 })).toEqual({
      scrollable: false,
      canScrollUp: false,
      canScrollDown: false,
    });
    expect(projectPickerScrollState({ scrollHeight: 200, clientHeight: 100, scrollTop: 0 })).toEqual({
      scrollable: true,
      canScrollUp: false,
      canScrollDown: true,
    });
    expect(projectPickerScrollState({ scrollHeight: 200, clientHeight: 100, scrollTop: 99 })).toEqual({
      scrollable: true,
      canScrollUp: true,
      canScrollDown: false,
    });
  });

  it("derives menu aim geometry from panel rectangles and pointer events", () => {
    const anchor = rect({ left: 10, right: 50, top: 20, bottom: 60, width: 40, height: 40 });
    expect(projectPickerSubpanelAimOrigin(anchor)).toEqual({ x: 30, y: 40 });
    expect(projectPickerMenuAimRect(anchor)).toEqual({ left: 10, right: 50, top: 20, bottom: 60 });
    expect(projectPickerPointerPoint({ clientX: 1, clientY: 2 })).toEqual({ x: 1, y: 2 });
    expect(projectPickerSubpanelSide(anchor, { left: 60, right: 120, top: 0, bottom: 100 })).toBe("right");
    expect(projectPickerSubpanelSide(anchor, { left: -60, right: 5, top: 0, bottom: 100 })).toBe("left");
  });
});
