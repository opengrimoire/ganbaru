import { describe, expect, it } from "vitest";
import { ChatPanelVisibilityState, chatPanelScope } from "./panel-visibility.svelte";

describe("ChatPanelVisibilityState", () => {
  it("starts every channel with both panels closed", () => {
    const visibility = new ChatPanelVisibilityState();

    expect(visibility.read("channel-a")).toEqual({ inspector: false, bottom: false });
    expect(visibility.read(null)).toEqual({ inspector: false, bottom: false });
  });

  it("keeps each channel's open panels independent and restores them when switching back", () => {
    const visibility = new ChatPanelVisibilityState();

    visibility.setOpen("channel-a", "bottom", true);
    visibility.setOpen("channel-b", "inspector", true);

    expect(visibility.read("channel-a")).toEqual({ inspector: false, bottom: true });
    expect(visibility.read("channel-b")).toEqual({ inspector: true, bottom: false });
    expect(visibility.read("channel-c")).toEqual({ inspector: false, bottom: false });

    visibility.toggle("channel-b", "inspector");
    expect(visibility.isOpen("channel-b", "inspector")).toBe(false);
    expect(visibility.isOpen("channel-a", "bottom")).toBe(true);
  });

  it("changes one panel without touching the other panel of the same channel", () => {
    const visibility = new ChatPanelVisibilityState();

    visibility.setOpen("channel-a", "inspector", true);
    visibility.setOpen("channel-a", "bottom", true);
    visibility.setOpen("channel-a", "inspector", false);

    expect(visibility.read("channel-a")).toEqual({ inspector: false, bottom: true });
  });

  it("keeps the most recently opened channels when the bound is reached", () => {
    const visibility = new ChatPanelVisibilityState();

    for (let index = 0; index < 65; index += 1) visibility.setOpen(`channel-${index}`, "bottom", true);

    expect(visibility.isOpen("channel-0", "bottom")).toBe(false);
    expect(visibility.isOpen("channel-1", "bottom")).toBe(true);
    expect(visibility.isOpen("channel-64", "bottom")).toBe(true);
  });

  it("closes every channel on clear", () => {
    const visibility = new ChatPanelVisibilityState();

    visibility.setOpen("channel-a", "inspector", true);
    visibility.setOpen(null, "bottom", true);
    visibility.clear();

    expect(visibility.read("channel-a")).toEqual({ inspector: false, bottom: false });
    expect(visibility.read(null)).toEqual({ inspector: false, bottom: false });
  });

  it("keeps a reply thread tab's panels apart from its channel's", () => {
    const visibility = new ChatPanelVisibilityState();
    const thread = chatPanelScope("channel-a", "reply-1");

    visibility.setOpen(thread, "bottom", true);

    expect(thread).not.toBe("channel-a");
    expect(chatPanelScope("channel-a", null)).toBe("channel-a");
    expect(chatPanelScope(null, "reply-1")).toBeNull();
    expect(chatPanelScope("channel-a", "reply-2")).not.toBe(thread);
    expect(visibility.isOpen(thread, "bottom")).toBe(true);
    expect(visibility.isOpen("channel-a", "bottom")).toBe(false);
  });
});
