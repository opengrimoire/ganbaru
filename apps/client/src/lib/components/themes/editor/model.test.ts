import { describe, expect, it } from "vitest";
import {
  SOURCE_GROUPS,
  localizedThemeNavItems,
  isCalendarGroup,
  isTextActionGroup,
  tokenInfo,
  type SourceGroup,
  type SourceGroupId,
  type ThemeNavTarget,
} from "./model";

function groupIds(groups: readonly SourceGroup[]): SourceGroupId[] {
  return groups.map((group) => group.id);
}

describe("theme editor model", () => {
  it("uses stable unique ids for editable groups", () => {
    const ids = groupIds(SOURCE_GROUPS);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it("keeps section partitions keyed by stable ids", () => {
    const calendarGroups = SOURCE_GROUPS.filter(isCalendarGroup);
    const textActionGroups = SOURCE_GROUPS.filter(isTextActionGroup);
    expect(groupIds(calendarGroups)).toEqual([
      "calendar-surface",
      "calendar-details",
      "event-panel",
    ]);
    expect(groupIds(textActionGroups)).toEqual([
      "ink",
      "primary-action",
      "destructive",
      "confirm",
      "warning",
    ]);

    for (const group of calendarGroups) {
      expect(isCalendarGroup(group)).toBe(true);
      expect(isTextActionGroup(group)).toBe(false);
    }
    for (const group of textActionGroups) {
      expect(isTextActionGroup(group)).toBe(true);
      expect(isCalendarGroup(group)).toBe(false);
    }
  });

  it("keeps navigation targets backed by model entry points", () => {
    const navTargets = new Set<ThemeNavTarget>(
      localizedThemeNavItems((key: string, ..._args: unknown[]) => key)
        .map((item) => item.target),
    );
    expect(navTargets).toEqual(
      new Set<ThemeNavTarget>([
        "general",
        "calendar",
        "signals",
        "json",
      ]),
    );

    const groupTargets = new Set(
      SOURCE_GROUPS.flatMap((group) =>
        group.navTarget === undefined ? [] : [group.navTarget],
      ),
    );
    expect(groupTargets.has("general")).toBe(true);
    expect(groupTargets.has("signals")).toBe(true);
    expect(SOURCE_GROUPS.some(isCalendarGroup)).toBe(true);
  });

  it("places the empty rail picker before the break marker picker", () => {
    const surfaceGroup = SOURCE_GROUPS.find(
      (group) => group.id === "calendar-surface",
    );
    const detailsGroup = SOURCE_GROUPS.find(
      (group) => group.id === "calendar-details",
    );
    expect(surfaceGroup?.rows).not.toContainEqual({
      kind: "single",
      key: "--cal-timeline-rail",
      scope: "cal",
    });
    expect(detailsGroup?.rows).toMatchObject([
      { key: "--cal-current-time" },
      { key: "--cal-timeline-rail" },
      { key: "--cal-timeline-break" },
      { key: "--cal-timeline-focus" },
    ]);
  });

  it("provides visible labels and descriptions for every row", () => {
    for (const group of SOURCE_GROUPS) {
      expect(group.title.length).toBeGreaterThan(0);
      expect(group.description.length).toBeGreaterThan(0);

      for (const row of group.rows) {
        if (row.kind === "single") {
          const info = tokenInfo(row);
          expect(info.title.length).toBeGreaterThan(0);
          expect(info.description.length).toBeGreaterThan(0);
        } else {
          expect(row.title.length).toBeGreaterThan(0);
          expect(row.description.length).toBeGreaterThan(0);
        }
      }
    }
  });
});
