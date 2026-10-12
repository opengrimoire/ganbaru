import { describe, expect, it } from "vitest";
import {
  applyProjectEmojiSkinTone,
  cleanupProjectIconRecentValues,
  filterProjectEmojiEntries,
  filterProjectLucideIcons,
  projectIconEmojiRecentValues,
  projectIconLucideRecentValues,
  iconPickerAnchoredPanelPlacement,
  iconPickerGroupVirtualWindow,
  iconPickerIsPrimaryLucideCategory,
  iconPickerLucideCategoryIcon,
  iconPickerLucideCategoryOptions,
  iconPickerLucideGroups,
  iconPickerLucideRecentPreviewValue,
  iconPickerMenuPlacement,
  iconPickerPanelPlacement,
  iconPickerPointPlacement,
  iconPickerPrimaryLucideCategoryOptions,
  iconPickerRandomEmojiIcon,
  iconPickerRandomLucideIcon,
  iconPickerVisibleEmojiCategories,
  iconPickerVisibleCustomEmojis,
  iconPickerVirtualWindow,
  prependProjectIconRecentValue,
  projectEmojiSkinToneFromEmoji,
  readProjectIconAskEveryTime,
  readProjectIconDefaultColor,
  readProjectIconRecentValues,
  stripProjectEmojiSkinTone,
  type IconPickerRect,
} from "./picker";
import {
  PROJECT_EMOJI_CATALOG_VERSION,
  PROJECT_EMOJI_ENTRIES,
  PROJECT_EMOJI_SKIN_TONE_BASES,
  type ProjectEmojiEntry,
} from "./emoji-catalog.generated";
import {
  PROJECT_LUCIDE_ICONS,
  type ProjectLucideIconEntry,
} from "./lucide-catalog.generated";
import type { ProjectCustomEmoji } from "$lib/projects/types";

const emojiEntries: readonly ProjectEmojiEntry[] = [
  { emoji: "🚀", name: "rocket", category: "travel", terms: "🚀 rocket" },
  { emoji: "✅", name: "check", category: "symbols", terms: "✅ check done" },
  { emoji: "🏁", name: "chequered flag", category: "flags", terms: "🏁 chequered flag" },
  { emoji: "☕", name: "coffee", category: "food", terms: "☕ coffee drink" },
];

const lucideEntries: readonly ProjectLucideIconEntry[] = [
  { slug: "rocket", label: "Rocket", category: "Travel", terms: "rocket travel", iconNode: [] },
  { slug: "folder", label: "Folder", category: "File icons", terms: "folder file", iconNode: [] },
  { slug: "check", label: "Check", category: "Mathematics", terms: "check done", iconNode: [] },
];

const customEmojis: readonly ProjectCustomEmoji[] = [
  {
    id: "launch",
    name: "Launch badge",
    assetPath: "project-icons/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png",
    sortOrder: 0,
    createdAt: "2026-01-01T00:00:00.000Z",
    updatedAt: "2026-01-01T00:00:00.000Z",
  },
  {
    id: "focus",
    name: "Focus mark",
    assetPath: "project-icons/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.png",
    sortOrder: 1,
    createdAt: "2026-01-02T00:00:00.000Z",
    updatedAt: "2026-01-02T00:00:00.000Z",
  },
];

const pickerBoundary: IconPickerRect = {
  top: 40,
  right: 900,
  bottom: 620,
  left: 0,
  width: 900,
  height: 580,
};

function pickerTrigger(overrides: Partial<IconPickerRect> = {}): IconPickerRect {
  return {
    top: 120,
    right: 640,
    bottom: 152,
    left: 464,
    width: 176,
    height: 32,
    ...overrides,
  };
}

describe("icon picker helpers", () => {
  it("filters emoji by category and search terms", () => {
    expect(filterProjectEmojiEntries(emojiEntries, "coffee", "all").map((entry) => entry.emoji)).toEqual(["☕"]);
    expect(filterProjectEmojiEntries(emojiEntries, "", "symbols").map((entry) => entry.emoji)).toEqual(["✅", "🏁"]);
  });

  it("filters Lucide icons by category and search terms", () => {
    expect(filterProjectLucideIcons(lucideEntries, "file", "all").map((entry) => entry.slug)).toEqual(["folder"]);
    expect(filterProjectLucideIcons(lucideEntries, "", "Travel").map((entry) => entry.slug)).toEqual(["rocket"]);
  });

  it("searches official English Lucide tags", () => {
    expect(filterProjectLucideIcons(PROJECT_LUCIDE_ICONS, "strong", "all").map((entry) => entry.slug))
      .toContain("biceps-flexed");
  });

  it("includes the app-supplied sport shoe icon in search", () => {
    expect(filterProjectLucideIcons(PROJECT_LUCIDE_ICONS, "sneaker", "Sports").map((entry) => entry.slug))
      .toContain("sport-shoe");
  });

  it("builds the visible emoji category tabs with the localized symbols label", () => {
    expect(iconPickerVisibleEmojiCategories("Symbols plus flags")).toEqual([
      { id: "smileys", label: "Smileys" },
      { id: "people", label: "People" },
      { id: "nature", label: "Nature" },
      { id: "food", label: "Food" },
      { id: "activity", label: "Activity" },
      { id: "travel", label: "Travel" },
      { id: "objects", label: "Objects" },
      { id: "symbols", label: "Symbols plus flags" },
    ]);
  });

  it("builds Lucide category options with preferred icons and primary ordering", () => {
    const categoryIconEntries: readonly ProjectLucideIconEntry[] = [
      { slug: "rocket", label: "Rocket", category: "Travel", terms: "rocket travel", iconNode: [] },
      { slug: "luggage", label: "Luggage", category: "Travel", terms: "luggage travel", iconNode: [] },
      { slug: "wrench", label: "Wrench", category: "Tools", terms: "wrench tools", iconNode: [] },
      { slug: "folder", label: "Folder", category: "File icons", terms: "folder file", iconNode: [] },
    ];
    const options = iconPickerLucideCategoryOptions([
      "Travel",
      "Tools",
      "File icons",
    ], categoryIconEntries);

    expect(iconPickerLucideCategoryIcon("Travel", categoryIconEntries)?.slug).toBe("luggage");
    expect(options.map((option) => [option.category, option.icon?.slug])).toEqual([
      ["Travel", "luggage"],
      ["Tools", "wrench"],
      ["File icons", "folder"],
    ]);
    expect(iconPickerPrimaryLucideCategoryOptions(options).map((option) => option.category)).toEqual([
      "File icons",
      "Tools",
    ]);
    expect(iconPickerIsPrimaryLucideCategory("Tools")).toBe(true);
    expect(iconPickerIsPrimaryLucideCategory("Travel")).toBe(false);
  });

  it("reads recent values from unknown config data", () => {
    expect(readProjectIconRecentValues(["emoji:🚀", 4, "lucide:folder", null])).toEqual([
      "emoji:🚀",
      "lucide:folder",
    ]);
    expect(readProjectIconRecentValues("emoji:🚀")).toEqual([]);
  });

  it("splits recent values by picker tab", () => {
    const values = ["emoji:🚀", "custom-emoji:launch", "lucide:folder", "none"];

    expect(projectIconEmojiRecentValues(values)).toEqual(["emoji:🚀", "custom-emoji:launch"]);
    expect(projectIconLucideRecentValues(values)).toEqual(["lucide:folder"]);
  });

  it("cleans recent values by deduplicating and dropping missing custom emoji", () => {
    expect(cleanupProjectIconRecentValues([
      "emoji:🚀",
      "emoji:🚀",
      "custom-emoji:missing",
      "custom-emoji:kept",
      "none",
    ], new Set(["kept"]))).toEqual(["emoji:🚀", "custom-emoji:kept"]);
  });

  it("prepends recent values without keeping stale duplicates", () => {
    expect(prependProjectIconRecentValue(
      ["emoji:🚀", "lucide:folder"],
      { kind: "emoji", emoji: "🚀" },
      new Set(),
    )).toEqual(["emoji:🚀", "lucide:folder"]);
  });

  it("filters custom emoji by name without changing the empty-query order", () => {
    expect(iconPickerVisibleCustomEmojis(customEmojis, "").map((emoji) => emoji.id)).toEqual([
      "launch",
      "focus",
    ]);
    expect(iconPickerVisibleCustomEmojis(customEmojis, "FOCUS").map((emoji) => emoji.id)).toEqual([
      "focus",
    ]);
  });

  it("groups Lucide icons in category order and drops empty categories", () => {
    expect(iconPickerLucideGroups(lucideEntries, [
      "Mathematics",
      "Animals",
      "Travel",
    ])).toEqual([
      { category: "Mathematics", entries: [lucideEntries[2]] },
      { category: "Travel", entries: [lucideEntries[0]] },
    ]);
  });

  it("previews recent Lucide icons with the active color policy", () => {
    expect(iconPickerLucideRecentPreviewValue({
      rawValue: "lucide:folder",
      iconColor: 3,
    })).toBe("lucide:folder:3");
    expect(iconPickerLucideRecentPreviewValue({
      rawValue: "lucide:folder:9",
      iconColor: 3,
    })).toBe("lucide:folder:3");
    expect(iconPickerLucideRecentPreviewValue({
      rawValue: "lucide:folder:9",
      iconColor: "default",
    })).toBe("lucide:folder");
    expect(iconPickerLucideRecentPreviewValue({
      rawValue: "emoji:🚀",
      iconColor: 3,
    })).toBe("emoji:🚀");
  });

  it("accepts only valid stored default icon colors", () => {
    expect(readProjectIconDefaultColor("default")).toBe("default");
    expect(readProjectIconDefaultColor(0)).toBe(0);
    expect(readProjectIconDefaultColor(31)).toBe(31);
    expect(readProjectIconDefaultColor(32)).toBe("default");
    expect(readProjectIconDefaultColor(-1)).toBe("default");
    expect(readProjectIconDefaultColor("3")).toBe("default");
    expect(readProjectIconDefaultColor(undefined)).toBe("default");
  });

  it("enables Ask every time only for an explicit stored true value", () => {
    expect(readProjectIconAskEveryTime(true)).toBe(true);
    expect(readProjectIconAskEveryTime(false)).toBe(false);
    expect(readProjectIconAskEveryTime("true")).toBe(false);
    expect(readProjectIconAskEveryTime(undefined)).toBe(false);
  });

  it("selects random emoji and Lucide values from injected randomness", () => {
    const thumbs = [{ emoji: "👍", name: "thumbs up", category: "people", terms: "👍 thumbs up" }] as const;

    expect(iconPickerRandomEmojiIcon(thumbs, "medium", () => 0)).toEqual({
      kind: "emoji",
      emoji: "👍🏽",
    });
    expect(iconPickerRandomLucideIcon(lucideEntries, {
      iconColor: 5,
      random: () => 0.9,
    })).toEqual({
      kind: "lucide",
      slug: "check",
      color: 5,
    });
    expect(iconPickerRandomLucideIcon(lucideEntries, {
      iconColor: "default",
      random: () => 0.9,
    })).toEqual({
      kind: "lucide",
      slug: "check",
      color: "default",
    });
    expect(iconPickerRandomEmojiIcon([], "default", () => 0)).toBeNull();
  });

  it("computes a bounded virtual grid window", () => {
    expect(iconPickerVirtualWindow(100, 5, 36, 72, 72, 1)).toEqual({
      startIndex: 5,
      endIndex: 30,
      beforeHeight: 36,
      afterHeight: 504,
    });
  });

  it("anchors the icon picker panel from the trigger bottom trailing edge", () => {
    expect(iconPickerPanelPlacement({
      triggerRect: pickerTrigger(),
      boundaryRect: pickerBoundary,
      preferredWidth: 360,
      preferredHeight: 440,
    })).toEqual({
      left: 280,
      top: 156,
      width: 360,
      height: 440,
    });
  });

  it("aligns the panel with the trigger's left edge when requested", () => {
    expect(iconPickerPanelPlacement({
      triggerRect: pickerTrigger(),
      boundaryRect: pickerBoundary,
      preferredWidth: 360,
      preferredHeight: 440,
      align: "start",
    })).toEqual({
      left: 464,
      top: 156,
      width: 360,
      height: 440,
    });

    expect(iconPickerPanelPlacement({
      triggerRect: pickerTrigger({ left: 840, right: 960, width: 120 }),
      boundaryRect: pickerBoundary,
      preferredWidth: 360,
      preferredHeight: 160,
      align: "start",
    }).left).toBe(532);
  });

  it("caps the icon picker panel before the lower boundary", () => {
    const placement = iconPickerPanelPlacement({
      triggerRect: pickerTrigger({ top: 280, bottom: 312 }),
      boundaryRect: pickerBoundary,
      preferredWidth: 360,
      preferredHeight: 440,
    });

    expect(placement.top).toBe(316);
    expect(placement.top + placement.height).toBeLessThanOrEqual(pickerBoundary.bottom - 8);
  });

  it("keeps the icon picker panel inside the viewport width", () => {
    const placement = iconPickerPanelPlacement({
      triggerRect: pickerTrigger({ left: 840, right: 960, width: 120 }),
      boundaryRect: pickerBoundary,
      preferredWidth: 360,
      preferredHeight: 160,
    });

    expect(placement.left + placement.width).toBeLessThanOrEqual(pickerBoundary.right - 8);
    expect(placement.left).toBe(532);
  });

  it("places anchored custom panels above when there is more useful space above", () => {
    expect(iconPickerAnchoredPanelPlacement({
      anchorRect: pickerTrigger({ top: 500, bottom: 532 }),
      viewportRect: pickerBoundary,
      panelWidth: 330,
      preferredHeight: 360,
    })).toEqual({
      left: 310,
      top: 136,
      maxHeight: 360,
    });
  });

  it("places anchored custom panels below when there is enough space below", () => {
    expect(iconPickerAnchoredPanelPlacement({
      anchorRect: pickerTrigger(),
      viewportRect: pickerBoundary,
      panelWidth: 330,
      preferredHeight: 360,
    })).toEqual({
      left: 310,
      top: 156,
      maxHeight: 360,
    });
  });

  it("places category menus on the side with more available space", () => {
    expect(iconPickerMenuPlacement({
      anchorRect: pickerTrigger({ top: 520, bottom: 552 }),
      viewportRect: pickerBoundary,
      menuWidth: 240,
      menuMaxHeight: 280,
    })).toEqual({
      left: 400,
      top: 236,
      maxHeight: 280,
    });
  });

  it("centers point panels on their anchor while staying in the viewport", () => {
    expect(iconPickerPointPlacement({
      anchorRect: pickerTrigger({ left: 0, right: 32, width: 32 }),
      viewportRect: pickerBoundary,
      panelWidth: 260,
      panelHeight: 144,
    })).toEqual({
      left: 8,
      top: 156,
    });
  });

  it("virtualizes grouped icon rows without dropping group spacing", () => {
    const groups = [
      { category: "A", entries: Array.from({ length: 12 }, (_, index) => `a-${index}`) },
      { category: "B", entries: Array.from({ length: 8 }, (_, index) => `b-${index}`) },
      { category: "C", entries: Array.from({ length: 4 }, (_, index) => `c-${index}`) },
    ];

    const result = iconPickerGroupVirtualWindow({
      groups,
      columnCount: 4,
      viewportHeight: 72,
      scrollTop: 160,
      rowHeight: 36,
      groupHeaderHeight: 24,
      groupGapHeight: 12,
      overscanRows: 1,
    });

    expect(result.beforeHeight).toBe(0);
    expect(result.afterHeight).toBe(0);
    expect(result.groups.map((group) => group.category)).toEqual(["A", "B", "C"]);
    expect(result.groups[0]?.entries).toEqual(groups[0]?.entries.slice(8, 12));
    expect(result.groups[0]?.beforeRowsHeight).toBe(72);
    expect(result.groups[1]?.entries).toEqual(groups[1]?.entries);
    expect(result.groups[2]?.entries).toEqual(groups[2]?.entries);
  });

  it("applies skin tone only to supported emoji", () => {
    expect(applyProjectEmojiSkinTone("👍", "medium")).toBe("👍🏽");
    expect(applyProjectEmojiSkinTone("👍🏽", "default")).toBe("👍");
    expect(applyProjectEmojiSkinTone("👨‍💻", "medium")).toBe("👨🏽‍💻");
    expect(applyProjectEmojiSkinTone("🚀", "dark")).toBe("🚀");
    expect(stripProjectEmojiSkinTone("🙏🏿")).toBe("🙏");
    expect(projectEmojiSkinToneFromEmoji("👏🏼")).toBe("medium-light");
  });

  it("uses the generated Unicode emoji catalog", () => {
    expect(PROJECT_EMOJI_CATALOG_VERSION).toBe("17.0");
    expect(PROJECT_EMOJI_ENTRIES.length).toBeGreaterThan(1_500);
    expect(PROJECT_EMOJI_ENTRIES.filter((entry) => entry.category === "flags").length).toBeGreaterThan(250);
    expect(PROJECT_EMOJI_ENTRIES.some((entry) => entry.emoji === "🇿🇼" && entry.name === "flag: Zimbabwe")).toBe(true);
    expect(PROJECT_EMOJI_SKIN_TONE_BASES).toContain("👨‍💻");
  });
});
