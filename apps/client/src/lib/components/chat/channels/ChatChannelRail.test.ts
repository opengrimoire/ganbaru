// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatChannelRead } from "$lib/chat/contracts";
import { readChatSidebarSections, saveChatSidebarSections } from "$lib/chat/channel-sections";
import { getChat } from "$lib/stores/chat.svelte";
import { getSettingsLauncher } from "$lib/stores/settings-launcher.svelte";
import { setActiveVaultIdentity } from "$lib/vault/active-vault";
import { SUBMENU_CLOSE_DELAY_MS } from "$lib/utils/menu-aim";
import ChatChannelRail from "./ChatChannelRail.svelte";

const projectState = vi.hoisted(() => ({
  store: {
    selectedProjectId: "project-1" as string | null,
    projects: [{ id: "project-1", name: "Ganbaru" }],
  },
}));

vi.mock("$lib/stores/projects.svelte", () => ({
  getProjects: () => projectState.store,
}));

describe("ChatChannelRail", () => {
  let target: HTMLDivElement;
  let component: ReturnType<typeof mount> | null;

  beforeEach(() => {
    localStorage.clear();
    setActiveVaultIdentity("vault-a");
    projectState.store.selectedProjectId = "project-1";
    const chat = getChat();
    chat.activeChannels = [channel("channel-general", "general"), channel("channel-design", "design")];
    chat.archivedChannels = [];
    chat.channelsLoading = false;
    chat.selectedChannelId = "channel-general";
    chat.channelArchiveOpen = false;
    saveChatSidebarSections("project-1", [{
      id: "section-design",
      name: "Design",
      collapsed: false,
      channelIds: ["channel-design"],
    }]);
    target = document.createElement("div");
    document.body.appendChild(target);
    component = null;
  });

  afterEach(async () => {
    if (component) await unmount(component);
    target.remove();
    setActiveVaultIdentity(null);
    vi.restoreAllMocks();
  });

  it("uses right-side disclosure chevrons and collapses every channel section", async () => {
    component = mount(ChatChannelRail, {
      target,
      props: {
        expanded: true,
        showCollapsedStrip: true,
        onExpand: vi.fn(),
        onCollapse: vi.fn(),
      },
    });
    await tick();

    const toggles = [...target.querySelectorAll<HTMLButtonElement>(".section-toggle")];
    expect(toggles.map((toggle) => toggle.textContent?.trim())).toEqual([
      "Channels",
      "Design",
      "Direct messages",
    ]);
    for (const toggle of toggles) {
      expect(toggle.firstElementChild?.tagName).toBe("SPAN");
      expect(toggle.lastElementChild?.classList.contains("section-chevron")).toBe(true);
      expect(toggle.getAttribute("aria-expanded")).toBe("true");
    }

    expect(target.querySelector("[data-chat-direct-messages-hint]")).not.toBeNull();

    toggles[0]?.click();
    toggles[1]?.click();
    toggles[2]?.click();
    await tick();

    expect(toggles.map((toggle) => toggle.getAttribute("aria-expanded"))).toEqual([
      "false",
      "false",
      "false",
    ]);
    expect(toggles.every((toggle) => toggle.classList.contains("collapsed"))).toBe(true);
    expect(target.querySelectorAll(".channel-row")).toHaveLength(0);
    expect(target.querySelector("[data-chat-direct-messages-hint]")).toBeNull();
  });

  it("keeps search in the toolbar and clears filtering without hiding the field", async () => {
    component = mount(ChatChannelRail, {
      target,
      props: { expanded: true, showCollapsedStrip: true, onExpand: vi.fn(), onCollapse: vi.fn() },
    });
    await tick();
    const input = target.querySelector<HTMLInputElement>('.explorer-toolbar input[type="search"]');
    expect(input).not.toBeNull();
    expect(document.activeElement).not.toBe(input);
    expect(target.querySelectorAll(".explorer-toolbar button")).toHaveLength(1);
    window.dispatchEvent(new Event("ganbaru-ai:chat-focus-search"));
    await tick();
    await vi.waitFor(() => expect(document.activeElement).toBe(input));
    if (!input) throw new Error("Search input is missing");
    input.value = "g";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await tick();
    expect([...target.querySelectorAll(".channel-row")].map((row) => row.textContent?.trim())).toEqual(["general", "design"]);
    input.value = "z";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await tick();
    expect(target.querySelectorAll(".channel-row")).toHaveLength(0);
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await tick();
    expect(input.value).toBe("");
    expect(target.querySelectorAll(".channel-row")).toHaveLength(2);
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await tick();
    expect(target.querySelector('input[type="search"]')).toBe(input);
    expect(document.activeElement).toBe(input);
  });

  it("shows a channel name tooltip only while the label is clipped", async () => {
    component = mount(ChatChannelRail, {
      target,
      props: { expanded: true, showCollapsedStrip: true, onExpand: vi.fn(), onCollapse: vi.fn() },
    });
    await tick();
    const label = target.querySelector<HTMLElement>(".channel-row .truncate");
    if (!label) throw new Error("Channel label is missing");
    let renderedWidth = 80;
    Object.defineProperties(label, {
      scrollWidth: { configurable: true, get: () => renderedWidth },
      clientWidth: { configurable: true, value: 100 },
    });

    expect(label.hasAttribute("title")).toBe(false);
    label.dispatchEvent(new PointerEvent("pointerover", { bubbles: true }));
    expect(label.dataset.appTooltip).toBeUndefined();

    renderedWidth = 150;
    label.dispatchEvent(new PointerEvent("pointerover", { bubbles: true }));
    expect(label.dataset.appTooltip).toBe("general");

    renderedWidth = 100;
    label.dispatchEvent(new PointerEvent("pointerover", { bubbles: true }));
    expect(label.dataset.appTooltip).toBeUndefined();
  });

  it("shows section name tooltips only while their labels are clipped", async () => {
    component = mount(ChatChannelRail, {
      target,
      props: { expanded: true, showCollapsedStrip: true, onExpand: vi.fn(), onCollapse: vi.fn() },
    });
    await tick();
    const labels = [...target.querySelectorAll<HTMLElement>(".section-toggle > span")];
    expect(labels.map((label) => label.textContent)).toEqual(["Channels", "Design", "Direct messages"]);

    for (const label of labels) {
      let renderedWidth = 80;
      Object.defineProperties(label, {
        scrollWidth: { configurable: true, get: () => renderedWidth },
        clientWidth: { configurable: true, value: 100 },
      });
      label.dispatchEvent(new PointerEvent("pointerover", { bubbles: true }));
      expect(label.dataset.appTooltip).toBeUndefined();

      renderedWidth = 150;
      label.dispatchEvent(new PointerEvent("pointerover", { bubbles: true }));
      expect(label.dataset.appTooltip).toBe(label.textContent);

      renderedWidth = 100;
      label.dispatchEvent(new PointerEvent("pointerover", { bubbles: true }));
      expect(label.dataset.appTooltip).toBeUndefined();
    }
  });

  it("focuses new section drafts and cancels them without changing navigation", async () => {
    component = mount(ChatChannelRail, {
      target,
      props: { expanded: true, showCollapsedStrip: true, onExpand: vi.fn(), onCollapse: vi.fn() },
    });
    await tick();
    const trigger = target.querySelector<HTMLButtonElement>(".section-heading .explorer-row-action");
    trigger?.click();
    await tick();
    target.querySelector<HTMLButtonElement>(".section-context-menu button")?.click();
    await tick();
    const input = target.querySelector<HTMLInputElement>('form input');
    expect(input).not.toBeNull();
    await vi.waitFor(() => expect(document.activeElement).toBe(input));
    if (!input) throw new Error("Section name input is missing");
    input.value = "Unfinished section";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await tick();
    expect(target.querySelector("form")).toBeNull();
    expect(document.activeElement).toBe(trigger);
    expect([...target.querySelectorAll(".section-toggle")].map((toggle) => toggle.textContent?.trim())).toEqual(["Channels", "Design", "Direct messages"]);
    trigger?.click();
    await tick();
    target.querySelector<HTMLButtonElement>(".section-context-menu button")?.click();
    await tick();
    expect(target.querySelector<HTMLInputElement>('form input')?.value).toBe("");
  });

  it("toggles section actions, clamps the menu, and closes it from outside or Escape", async () => {
    component = mount(ChatChannelRail, {
      target,
      props: { expanded: true, showCollapsedStrip: true, onExpand: vi.fn(), onCollapse: vi.fn() },
    });
    await tick();
    const trigger = target.querySelector<HTMLButtonElement>(".section-heading .explorer-row-action");
    expect(trigger).not.toBeNull();
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
      if (this.classList.contains("section-context-menu")) return DOMRect.fromRect({ width: 180, height: 80 });
      if (this === trigger) return DOMRect.fromRect({ x: window.innerWidth - 20, y: 30, width: 28, height: 28 });
      return DOMRect.fromRect();
    });

    trigger?.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    trigger?.click();
    await tick();
    const menu = target.querySelector<HTMLElement>(".section-context-menu");
    await vi.waitFor(() => expect(menu?.style.left).toBe(`${window.innerWidth - 180 - 8}px`));
    expect(menu?.querySelectorAll('[role="menuitem"]')).toHaveLength(1);
    expect(menu?.querySelector("svg")).not.toBeNull();
    expect(trigger?.getAttribute("aria-expanded")).toBe("true");

    trigger?.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    trigger?.click();
    await tick();
    expect(target.querySelector(".section-context-menu")).toBeNull();
    expect(trigger?.getAttribute("aria-expanded")).toBe("false");

    trigger?.click();
    await tick();
    target.querySelector<HTMLElement>(".section-context-menu")?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await tick();
    expect(target.querySelector(".section-context-menu")).toBeNull();
    expect(document.activeElement).toBe(trigger);

    trigger?.click();
    await tick();
    document.body.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    await tick();
    expect(target.querySelector(".section-context-menu")).toBeNull();
  });

  it("opens contacts and invitations from the direct messages header menu", async () => {
    const open = vi.spyOn(getSettingsLauncher(), "open").mockImplementation(() => undefined);
    component = mount(ChatChannelRail, {
      target,
      props: { expanded: true, showCollapsedStrip: true, onExpand: vi.fn(), onCollapse: vi.fn() },
    });
    await tick();
    const trigger = target.querySelector<HTMLButtonElement>("[data-chat-direct-messages] .section-heading .explorer-row-action");
    expect(trigger).not.toBeNull();
    trigger?.click();
    await tick();
    const menu = target.querySelector<HTMLElement>(".section-context-menu");
    expect([...menu?.querySelectorAll('[role="menuitem"]') ?? []].map((item) => item.textContent?.trim())).toEqual(["Contacts", "Invitations"]);
    expect(trigger?.getAttribute("aria-expanded")).toBe("true");
    menu?.querySelector<HTMLButtonElement>("[data-chat-invitations]")?.click();
    await tick();
    expect(open).toHaveBeenCalledWith("contacts", { contactsTab: "invitations" });
    expect(target.querySelector(".section-context-menu")).toBeNull();
  });

  it("shows section edit actions and closes the menu before confirmation", async () => {
    component = mount(ChatChannelRail, {
      target,
      props: { expanded: true, showCollapsedStrip: true, onExpand: vi.fn(), onCollapse: vi.fn() },
    });
    await tick();
    const trigger = target.querySelectorAll<HTMLButtonElement>(".section-heading .explorer-row-action")[1];
    trigger?.click();
    await tick();
    const menu = target.querySelector<HTMLElement>(".section-context-menu");
    expect([...menu?.querySelectorAll('[role="menuitem"]') ?? []].map((item) => item.textContent?.trim())).toEqual(["Rename", "Delete section"]);
    expect(menu?.querySelectorAll("svg")).toHaveLength(2);
    menu?.querySelectorAll<HTMLButtonElement>("button")[1]?.click();
    await tick();
    expect(target.querySelector(".section-context-menu")).toBeNull();
    expect(trigger?.getAttribute("aria-expanded")).toBe("false");
    expect(target.textContent).toContain("Delete section");
  });

  it("renames a section in its heading and keeps its navigation state", async () => {
    const prompt = vi.spyOn(window, "prompt");
    component = mount(ChatChannelRail, {
      target,
      props: { expanded: true, showCollapsedStrip: true, onExpand: vi.fn(), onCollapse: vi.fn() },
    });
    await tick();
    const section = target.querySelectorAll<HTMLElement>(".channel-section")[1];
    const toggle = section?.querySelector<HTMLButtonElement>(".section-toggle");
    toggle?.click();
    await tick();
    section?.querySelector<HTMLButtonElement>(".section-heading .explorer-row-action")?.click();
    await tick();
    target.querySelector<HTMLButtonElement>(".section-context-menu button")?.click();
    await tick();

    const input = section?.querySelector<HTMLInputElement>(".section-heading input");
    expect(prompt).not.toHaveBeenCalled();
    expect(input?.value).toBe("Design");
    expect(input?.parentElement?.classList.contains("section-renaming")).toBe(true);
    expect([...section?.querySelectorAll<HTMLButtonElement>(".section-rename-control") ?? []].map((button) => button.getAttribute("aria-label"))).toEqual(["Save", "Cancel"]);
    await vi.waitFor(() => expect(document.activeElement).toBe(input));
    expect(input?.selectionStart).toBe(0);
    expect(input?.selectionEnd).toBe("Design".length);
    if (!input) throw new Error("Section rename input is missing");
    input.value = "  Product planning  ";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    await tick();

    expect(section?.querySelector(".section-heading input")).toBeNull();
    expect(section?.querySelector(".section-toggle")?.textContent?.trim()).toBe("Product planning");
    expect(section?.querySelector(".section-toggle")?.getAttribute("aria-expanded")).toBe("false");
    expect(readChatSidebarSections("project-1")[0]?.name).toBe("Product planning");
    expect(document.activeElement).toBe(section?.querySelector(".section-toggle"));
  });

  it("cancels section renaming on Escape and saves valid edits on blur", async () => {
    component = mount(ChatChannelRail, {
      target,
      props: { expanded: true, showCollapsedStrip: true, onExpand: vi.fn(), onCollapse: vi.fn() },
    });
    await tick();
    const section = target.querySelectorAll<HTMLElement>(".channel-section")[1];
    async function openRename(): Promise<HTMLInputElement> {
      section?.querySelector<HTMLButtonElement>(".section-heading .explorer-row-action")?.click();
      await tick();
      target.querySelector<HTMLButtonElement>(".section-context-menu button")?.click();
      await tick();
      const input = section?.querySelector<HTMLInputElement>(".section-heading input");
      if (!input) throw new Error("Section rename input is missing");
      await vi.waitFor(() => expect(document.activeElement).toBe(input));
      return input;
    }

    const cancelled = await openRename();
    cancelled.value = "Discarded";
    cancelled.dispatchEvent(new Event("input", { bubbles: true }));
    cancelled.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await tick();
    expect(readChatSidebarSections("project-1")[0]?.name).toBe("Design");
    expect(document.activeElement).toBe(section?.querySelector(".section-toggle"));

    const saved = await openRename();
    saved.value = "Research";
    saved.dispatchEvent(new Event("input", { bubbles: true }));
    saved.blur();
    await tick();
    expect(readChatSidebarSections("project-1")[0]?.name).toBe("Research");

    const empty = await openRename();
    empty.value = "   ";
    empty.dispatchEvent(new Event("input", { bubbles: true }));
    empty.blur();
    await tick();
    expect(readChatSidebarSections("project-1")[0]?.name).toBe("Research");

    const tabbed = await openRename();
    tabbed.value = "Planning";
    tabbed.dispatchEvent(new Event("input", { bubbles: true }));
    section?.querySelector<HTMLButtonElement>('.section-rename-control[aria-label="Save"]')?.focus();
    expect(section?.querySelector(".section-heading input")).toBe(tabbed);
    target.querySelector<HTMLButtonElement>(".channel-section .section-toggle")?.focus();
    await tick();
    expect(readChatSidebarSections("project-1")[0]?.name).toBe("Planning");

    const keyboardCancelled = await openRename();
    keyboardCancelled.value = "Discarded again";
    keyboardCancelled.dispatchEvent(new Event("input", { bubbles: true }));
    const cancel = section?.querySelector<HTMLButtonElement>('.section-rename-control[aria-label="Cancel"]');
    cancel?.focus();
    cancel?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await tick();
    expect(readChatSidebarSections("project-1")[0]?.name).toBe("Planning");
    expect(document.activeElement).toBe(section?.querySelector(".section-toggle"));
  });

  it("keeps mobile rename controls tappable without saving before Cancel", async () => {
    component = mount(ChatChannelRail, {
      target,
      props: { presentation: "surface", expanded: true, showCollapsedStrip: true, onExpand: vi.fn(), onCollapse: vi.fn() },
    });
    await tick();
    const section = target.querySelectorAll<HTMLElement>(".channel-section")[1];
    async function openRename(): Promise<HTMLInputElement> {
      section?.querySelector<HTMLButtonElement>(".section-heading .explorer-row-action")?.click();
      await tick();
      target.querySelector<HTMLButtonElement>(".section-context-menu button")?.click();
      await tick();
      const input = section?.querySelector<HTMLInputElement>(".section-heading input");
      if (!input) throw new Error("Section rename input is missing");
      await vi.waitFor(() => expect(document.activeElement).toBe(input));
      return input;
    }

    const cancelled = await openRename();
    cancelled.value = "Do not save";
    cancelled.dispatchEvent(new Event("input", { bubbles: true }));
    const cancel = section?.querySelector<HTMLButtonElement>('.section-rename-control[aria-label="Cancel"]');
    expect(cancel).not.toBeNull();
    cancel?.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, pointerType: "touch" }));
    cancelled.blur();
    expect(section?.querySelector(".section-heading input")).toBe(cancelled);
    cancel?.dispatchEvent(new PointerEvent("pointerup", { bubbles: true, pointerType: "touch" }));
    cancel?.click();
    await tick();
    expect(readChatSidebarSections("project-1")[0]?.name).toBe("Design");

    const saved = await openRename();
    saved.value = "Research";
    saved.dispatchEvent(new Event("input", { bubbles: true }));
    const save = section?.querySelector<HTMLButtonElement>('.section-rename-control[aria-label="Save"]');
    expect(save?.disabled).toBe(false);
    save?.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, pointerType: "touch" }));
    saved.blur();
    save?.dispatchEvent(new PointerEvent("pointerup", { bubbles: true, pointerType: "touch" }));
    save?.click();
    await tick();
    expect(readChatSidebarSections("project-1")[0]?.name).toBe("Research");

    const empty = await openRename();
    empty.value = "   ";
    empty.dispatchEvent(new Event("input", { bubbles: true }));
    await tick();
    expect(section?.querySelector<HTMLButtonElement>('.section-rename-control[aria-label="Save"]')?.disabled).toBe(true);
  });

  it("opens channel actions without selecting the channel and restores focus on Escape", async () => {
    const selectChannel = vi.spyOn(getChat(), "selectChannel").mockResolvedValue();
    component = mount(ChatChannelRail, {
      target,
      props: { expanded: true, showCollapsedStrip: true, onExpand: vi.fn(), onCollapse: vi.fn() },
    });
    await tick();
    const trigger = target.querySelector<HTMLButtonElement>('.channel-row-group .explorer-row-action');
    trigger?.click();
    await tick();
    expect(selectChannel).not.toHaveBeenCalled();
    const menu = target.querySelector<HTMLElement>('[role="menu"]');
    expect(menu).not.toBeNull();
    await vi.waitFor(() => expect(menu?.contains(document.activeElement)).toBe(true));
    menu?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await tick();
    expect(target.querySelector('[role="menu"]')).toBeNull();
    expect(document.activeElement).toBe(trigger);
  });

  it("closes a channel menu when its action button is clicked again", async () => {
    component = mount(ChatChannelRail, {
      target,
      props: { expanded: true, showCollapsedStrip: true, onExpand: vi.fn(), onCollapse: vi.fn() },
    });
    await tick();
    const trigger = target.querySelector<HTMLButtonElement>('.channel-row-group .explorer-row-action');
    expect(trigger).not.toBeNull();

    trigger?.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    trigger?.click();
    await tick();
    expect(target.querySelector('[role="menu"]')).not.toBeNull();
    expect(trigger?.getAttribute("aria-expanded")).toBe("true");

    trigger?.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    trigger?.click();
    await tick();
    expect(target.querySelector('[role="menu"]')).toBeNull();
    expect(trigger?.getAttribute("aria-expanded")).toBe("false");

    trigger?.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    trigger?.click();
    await tick();
    expect(target.querySelector('[role="menu"]')).not.toBeNull();

    document.body.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    await tick();
    expect(target.querySelector('[role="menu"]')).toBeNull();
  });

  it("reveals move destinations on hover and keeps them available by keyboard focus", async () => {
    component = mount(ChatChannelRail, {
      target,
      props: { expanded: true, showCollapsedStrip: true, onExpand: vi.fn(), onCollapse: vi.fn() },
    });
    await tick();
    target.querySelector<HTMLButtonElement>(".channel-row-group .explorer-row-action")?.click();
    await tick();
    const group = target.querySelector<HTMLElement>('.channel-context-menu [role="group"]');
    const moveTrigger = group?.querySelector<HTMLButtonElement>('button[aria-haspopup="menu"]');
    expect(group).not.toBeNull();
    expect(target.querySelector(".channel-move-menu")).toBeNull();

    group?.dispatchEvent(new PointerEvent("pointerenter", { pointerType: "mouse" }));
    await vi.waitFor(() => expect(target.querySelector(".channel-move-menu")).not.toBeNull());
    expect(moveTrigger?.getAttribute("aria-expanded")).toBe("true");
    group?.dispatchEvent(new PointerEvent("pointerleave", { pointerType: "mouse" }));
    group?.dispatchEvent(new PointerEvent("pointerenter", { pointerType: "mouse" }));
    await new Promise((resolve) => setTimeout(resolve, SUBMENU_CLOSE_DELAY_MS + 20));
    expect(target.querySelector(".channel-move-menu")).not.toBeNull();
    group?.dispatchEvent(new PointerEvent("pointerleave", { pointerType: "mouse" }));
    await vi.waitFor(() => expect(target.querySelector(".channel-move-menu")).toBeNull());

    moveTrigger?.focus();
    await vi.waitFor(() => expect(target.querySelector(".channel-move-menu")).not.toBeNull());
    const destinations = target.querySelectorAll<HTMLButtonElement>('.channel-move-menu [role="menuitem"]');
    expect([...destinations].map((button) => button.textContent?.trim())).toEqual(["Channels", "Design"]);
    destinations[1]?.click();
    await tick();
    expect(target.querySelector(".channel-context-menu")).toBeNull();
    expect(readChatSidebarSections("project-1")[0]?.channelIds).toContain("channel-general");
  });

  it("does not mark a channel as current while the archive is open", async () => {
    getChat().channelArchiveOpen = true;
    component = mount(ChatChannelRail, {
      target,
      props: { expanded: true, showCollapsedStrip: true, onExpand: vi.fn(), onCollapse: vi.fn() },
    });
    await tick();
    expect(target.querySelector('.channel-row[aria-current]')).toBeNull();
    expect(target.querySelector('.archive-button')).toBeNull();
  });

  it("closes the mobile surface after navigating to a channel", async () => {
    const chat = getChat();
    const selectChannel = vi.spyOn(chat, "selectChannel").mockResolvedValue();
    const onCollapse = vi.fn();
    component = mount(ChatChannelRail, {
      target,
      props: {
        presentation: "surface",
        expanded: true,
        showCollapsedStrip: false,
        onExpand: vi.fn(),
        onCollapse,
      },
    });
    await tick();

    const designChannel = [...target.querySelectorAll<HTMLButtonElement>(".channel-row")]
      .find((button) => button.textContent?.trim() === "design");
    designChannel?.click();

    await vi.waitFor(() => {
      expect(selectChannel).toHaveBeenCalledWith("channel-design");
      expect(onCollapse).toHaveBeenCalledOnce();
    });
  });
});

function channel(id: string, name: string): ChatChannelRead {
  const timestamp = "2026-08-05T12:00:00.000Z";
  return {
    id,
    conversationId: `conversation-${id}`,
    projectId: "project-1",
    name,
    topic: `${name} channel`,
    isDefault: name === "general",
    memberships: [],
    messageCount: 0,
    unreadCount: 0,
    latestPreview: null,
    lastActivityAt: timestamp,
    attentionState: null,
    revision: 1,
    archivedAt: null,
    createdAt: timestamp,
    updatedAt: timestamp,
  };
}
