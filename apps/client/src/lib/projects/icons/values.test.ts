import { describe, expect, it } from "vitest";
import {
  parseProjectIcon,
  projectIconColorToEventColor,
  serializeProjectIcon,
} from "./values";

describe("project icon values", () => {
  it("parses explicit empty, emoji, Lucide, custom emoji, and asset values", () => {
    expect(parseProjectIcon("none")).toEqual({ kind: "none" });
    expect(parseProjectIcon("emoji:🚀")).toEqual({ kind: "emoji", emoji: "🚀" });
    expect(parseProjectIcon("lucide:rocket")).toEqual({ kind: "lucide", slug: "rocket", color: "default" });
    expect(parseProjectIcon("lucide:rocket:18")).toEqual({ kind: "lucide", slug: "rocket", color: 18 });
    expect(parseProjectIcon("custom-emoji:emoji-a")).toEqual({ kind: "custom-emoji", id: "emoji-a" });
    expect(parseProjectIcon(`asset:project-icons/${"a".repeat(64)}.webp`)).toEqual({
      kind: "asset",
      relativePath: `project-icons/${"a".repeat(64)}.webp`,
    });
  });

  it("serializes values using stable prefixes", () => {
    expect(serializeProjectIcon({ kind: "none" })).toBe("none");
    expect(serializeProjectIcon({ kind: "emoji", emoji: "📌" })).toBe("emoji:📌");
    expect(serializeProjectIcon({ kind: "lucide", slug: "folder", color: "default" })).toBe("lucide:folder");
    expect(serializeProjectIcon({ kind: "lucide", slug: "folder", color: 18 })).toBe("lucide:folder:18");
    expect(serializeProjectIcon({ kind: "custom-emoji", id: "emoji-a" })).toBe("custom-emoji:emoji-a");
    expect(serializeProjectIcon({
      kind: "asset",
      relativePath: `project-icons/${"b".repeat(64)}.jpg`,
    })).toBe(`asset:project-icons/${"b".repeat(64)}.jpg`);
  });

  it("falls back to the folder icon for malformed values", () => {
    expect(parseProjectIcon("asset:../escape.png")).toEqual({ kind: "lucide", slug: "folder", color: "default" });
    expect(parseProjectIcon(`asset:project-icons/${"a".repeat(64)}.gif`)).toEqual({ kind: "lucide", slug: "folder", color: "default" });
    expect(parseProjectIcon(`asset:project-icons/${"a".repeat(64)}.svg`)).toEqual({ kind: "lucide", slug: "folder", color: "default" });
    expect(parseProjectIcon("lucide:bad slug")).toEqual({ kind: "lucide", slug: "folder", color: "default" });
    expect(parseProjectIcon("folder")).toEqual({ kind: "lucide", slug: "folder", color: "default" });
    expect(parseProjectIcon("lucide:rocket:blue")).toEqual({ kind: "lucide", slug: "folder", color: "default" });
    expect(parseProjectIcon("lucide:rocket:99")).toEqual({ kind: "lucide", slug: "folder", color: "default" });
    expect(parseProjectIcon("emoji:")).toEqual({ kind: "lucide", slug: "folder", color: "default" });
  });

  it("maps canonical icon colors to event palette slots", () => {
    expect(projectIconColorToEventColor("default")).toBeUndefined();
    expect(projectIconColorToEventColor(7)).toBe(7);
  });
});
