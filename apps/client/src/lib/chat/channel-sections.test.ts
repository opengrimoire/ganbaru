// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { setActiveVaultIdentity } from "$lib/vault/active-vault";
import {
  moveChatChannelToSection,
  normalizeChatSidebarSections,
  orderChatSidebarSections,
  readLastChatChannelId,
  readChatSidebarSections,
  reorderChatSidebarSections,
  saveLastChatChannelId,
  saveChatSidebarSections,
  withChatSidebarSectionStarred,
} from "./channel-sections";

describe("Chat channel sections", () => {
  beforeEach(() => {
    localStorage.clear();
    setActiveVaultIdentity("vault-a");
  });

  afterEach(() => setActiveVaultIdentity(null));

  it("round trips validated custom sections", () => {
    saveChatSidebarSections("project", [{ id: "section", name: "Work", collapsed: true, channelIds: ["a"] }]);
    expect(localStorage.getItem("ganbaru.chat.channel-sections.v1:vault-a:project")).not.toBeNull();
    expect(readChatSidebarSections("project")).toEqual([
      { id: "section", name: "Work", collapsed: true, channelIds: ["a"] },
    ]);
  });

  it("removes stale and duplicate assignments", () => {
    expect(normalizeChatSidebarSections([
      { id: "a", name: "A", collapsed: false, channelIds: ["one", "missing"] },
      { id: "b", name: "B", collapsed: false, channelIds: ["one", "two"] },
    ], ["one", "two"])).toEqual([
      { id: "a", name: "A", collapsed: false, channelIds: ["one"] },
      { id: "b", name: "B", collapsed: false, channelIds: ["two"] },
    ]);
  });

  it("moves a channel between custom and fixed sections", () => {
    const sections = [{ id: "a", name: "A", collapsed: false, channelIds: ["one"] }];
    expect(moveChatChannelToSection(sections, "two", "a")[0]?.channelIds).toEqual(["one", "two"]);
    expect(moveChatChannelToSection(sections, "one", null)[0]?.channelIds).toEqual([]);
  });

  it("keeps a starred section through storage and drops a non-boolean star", () => {
    localStorage.setItem("ganbaru.chat.channel-sections.v1:vault-a:project", JSON.stringify([
      { id: "a", name: "A", collapsed: false, channelIds: [], starred: true },
      { id: "b", name: "B", collapsed: false, channelIds: [], starred: "yes" },
    ]));
    expect(readChatSidebarSections("project")).toEqual([
      { id: "a", name: "A", collapsed: false, channelIds: [], starred: true },
      { id: "b", name: "B", collapsed: false, channelIds: [] },
    ]);
  });

  it("stars and unstars one section without touching the others", () => {
    const sections = [
      { id: "a", name: "A", collapsed: false, channelIds: ["one"] },
      { id: "b", name: "B", collapsed: true, channelIds: [], starred: true },
    ];
    expect(withChatSidebarSectionStarred(sections, "a", true)[0]).toEqual({ id: "a", name: "A", collapsed: false, channelIds: ["one"], starred: true });
    expect(withChatSidebarSectionStarred(sections, "b", false)[1]).toEqual({ id: "b", name: "B", collapsed: true, channelIds: [] });
    expect(withChatSidebarSectionStarred(sections, "b", false)[0]).toBe(sections[0]);
  });

  it("orders sections by name or creation with starred sections first", () => {
    const sections = [
      { id: "1", name: "research", collapsed: false, channelIds: [] },
      { id: "2", name: "Design 10", collapsed: false, channelIds: [], starred: true },
      { id: "3", name: "Design 9", collapsed: false, channelIds: [] },
      { id: "4", name: "Admin", collapsed: false, channelIds: [] },
    ];
    const names = (order: Parameters<typeof orderChatSidebarSections>[1]) =>
      orderChatSidebarSections(sections, order).map((section) => section.name);
    expect(names("name-asc")).toEqual(["Design 10", "Admin", "Design 9", "research"]);
    expect(names("name-desc")).toEqual(["Design 10", "research", "Design 9", "Admin"]);
    expect(names("created-asc")).toEqual(["Design 10", "research", "Design 9", "Admin"]);
    expect(names("modified-desc")).toEqual(["Design 10", "Admin", "Design 9", "research"]);
  });

  it("reorders some sections and keeps the others in their slots", () => {
    const sections = ["a", "b", "c", "d"].map((id) => ({ id, name: id, collapsed: false, channelIds: [] }));
    const ids = (ordered: string[]) => reorderChatSidebarSections(sections, ordered).map((section) => section.id);
    expect(ids(["d", "a"])).toEqual(["d", "b", "c", "a"]);
    expect(ids(["c", "b", "a", "d"])).toEqual(["c", "b", "a", "d"]);
    expect(orderChatSidebarSections(reorderChatSidebarSections(sections, ["b", "a"]), "custom").map((section) => section.id))
      .toEqual(["b", "a", "c", "d"]);
  });

  it("remembers the last selected channel per project", () => {
    saveLastChatChannelId("one", "channel:a");
    saveLastChatChannelId("two", "channel:b");
    expect(localStorage.getItem("ganbaru.chat.last-channel.v1:vault-a:one")).toBe("channel:a");
    expect(readLastChatChannelId("one")).toBe("channel:a");
    expect(readLastChatChannelId("two")).toBe("channel:b");
  });

  it("isolates stable project IDs between vaults", () => {
    saveChatSidebarSections("project-routine-learning", [
      { id: "section-a", name: "Vault A", collapsed: false, channelIds: [] },
    ]);
    saveLastChatChannelId("project-routine-learning", "channel:a");

    setActiveVaultIdentity("vault-b");
    expect(readChatSidebarSections("project-routine-learning")).toEqual([]);
    expect(readLastChatChannelId("project-routine-learning")).toBeNull();

    setActiveVaultIdentity("vault-a");
    expect(readChatSidebarSections("project-routine-learning")).toEqual([
      { id: "section-a", name: "Vault A", collapsed: false, channelIds: [] },
    ]);
    expect(readLastChatChannelId("project-routine-learning")).toBe("channel:a");
  });

});
