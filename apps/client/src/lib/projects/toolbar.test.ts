import { describe, expect, it } from "vitest";
import {
  deriveProjectFilterChips,
  deriveProjectListColumnControls,
  pickProjectTaskModalLayout,
  projectNavigatorPanelGeometry,
  projectToolbarPanelGeometry,
  toggleProjectListColumn,
} from "./toolbar";
import type { ProjectCustomFieldFilter, ProjectTaskListColumn } from "./types";

describe("project toolbar", () => {
  it("derives active filter chips from non-default filters", () => {
    const customFieldFilter: ProjectCustomFieldFilter = {
      fieldId: "field-risk",
      mode: "filled",
    };

    expect(deriveProjectFilterChips({
      search: "  Example  ",
      statusLabel: "Open",
      sectionLabel: undefined,
      priorityLabel: "High",
      dueLabel: undefined,
      scheduleLabel: "Scheduled",
      dependencyLabel: undefined,
      tagFilterLabel: "Design",
      customFieldFilters: [customFieldFilter],
      customFieldFilterLabel: (filter) => `Field ${filter.fieldId}`,
    })).toEqual([
      { id: "search", label: "Example", clearTarget: "search" },
      { id: "status", label: "Open", clearTarget: "status" },
      { id: "priority", label: "High", clearTarget: "priority" },
      { id: "schedule", label: "Scheduled", clearTarget: "schedule" },
      { id: "tag", label: "Design", clearTarget: "tag" },
      { id: "custom:field-risk", label: "Field field-risk", clearTarget: "custom:field-risk" },
    ]);
  });

  it("derives customize column controls and toggles visibility", () => {
    const columns: ProjectTaskListColumn[] = ["status", "priority", "due"];

    expect(deriveProjectListColumnControls(columns, ["priority"], (column) => column)).toEqual([
      { column: "status", label: "status", visible: false },
      { column: "priority", label: "priority", visible: true },
      { column: "due", label: "due", visible: false },
    ]);
    expect(toggleProjectListColumn(["priority"], "due")).toEqual(["priority", "due"]);
    expect(toggleProjectListColumn(["priority", "due"], "priority")).toEqual(["due"]);
  });

  it("picks a centered task modal when desktop space is available", () => {
    expect(pickProjectTaskModalLayout({
      viewportWidth: 1440,
      viewportHeight: 900,
    })).toBe("modal");
  });

  it("uses sheet and fullscreen layouts when the modal cannot fit", () => {
    expect(pickProjectTaskModalLayout({
      viewportWidth: 600,
      viewportHeight: 800,
    })).toBe("sheet");
    expect(pickProjectTaskModalLayout({
      viewportWidth: 360,
      viewportHeight: 800,
    })).toBe("fullscreen");
    expect(pickProjectTaskModalLayout({
      viewportWidth: 900,
      viewportHeight: 440,
    })).toBe("fullscreen");
  });

  it("positions the project navigator below the breadcrumb without overflowing desktop viewports", () => {
    expect(projectNavigatorPanelGeometry({
      anchorLeft: 24,
      anchorBottom: 42,
      viewportWidth: 1200,
      viewportHeight: 800,
    })).toEqual({
      left: 24,
      top: 46,
      width: 259,
      height: 746,
    });

    expect(projectNavigatorPanelGeometry({
      anchorLeft: 1100,
      anchorBottom: 42,
      viewportWidth: 1200,
      viewportHeight: 800,
    }).left).toBe(933);
  });

  it("keeps the project navigator inside explicit tab bounds", () => {
    expect(projectNavigatorPanelGeometry({
      anchorLeft: 24,
      anchorBottom: 42,
      viewportWidth: 1200,
      viewportHeight: 800,
      boundsLeft: 10,
      boundsRight: 610,
      boundsTop: 42,
      boundsBottom: 500,
    })).toEqual({
      left: 24,
      top: 50,
      width: 259,
      height: 442,
    });
  });

  it("uses a compact project navigator layout on narrow viewports", () => {
    expect(projectNavigatorPanelGeometry({
      anchorLeft: 24,
      anchorBottom: 42,
      viewportWidth: 360,
      viewportHeight: 500,
    })).toEqual({
      left: 8,
      top: 48,
      width: 344,
      height: 444,
    });
  });

  it("positions toolbar panels from the trigger without overflowing the viewport", () => {
    expect(projectToolbarPanelGeometry({
      anchorLeft: 1080,
      anchorRight: 1112,
      anchorTop: 8,
      anchorBottom: 36,
      viewportWidth: 1200,
      viewportHeight: 800,
    })).toEqual({
      left: 732,
      top: 42,
      width: 380,
      maxHeight: 560,
    });

    expect(projectToolbarPanelGeometry({
      anchorLeft: 1170,
      anchorRight: 1198,
      anchorTop: 8,
      anchorBottom: 36,
      viewportWidth: 1200,
      viewportHeight: 800,
    }).left).toBe(812);
  });

  it("flips toolbar panels above the trigger when bottom space is constrained", () => {
    expect(projectToolbarPanelGeometry({
      anchorLeft: 420,
      anchorRight: 452,
      anchorTop: 510,
      anchorBottom: 538,
      viewportWidth: 900,
      viewportHeight: 560,
      preferredHeight: 320,
    })).toEqual({
      left: 72,
      top: 184,
      width: 380,
      maxHeight: 320,
    });
  });
});
