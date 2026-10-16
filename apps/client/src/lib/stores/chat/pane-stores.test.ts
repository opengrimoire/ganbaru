import { describe, expect, it, vi } from "vitest";
import type { ChatWindowChanges } from "$lib/chat/window-sync";
import { ChatPaneStores, chatWindowChanges, type ChatPaneStore } from "./pane-stores";

function store(): ChatPaneStore & { received: ChatWindowChanges[] } {
  const received: ChatWindowChanges[] = [];
  return { received, refreshFromOtherPane: vi.fn((changes: ChatWindowChanges) => { received.push(changes); }) };
}

describe("chatWindowChanges", () => {
  it("collects changed ids into sets, dropping duplicates and defaulting missing kinds to empty", () => {
    const changes = chatWindowChanges({ channelIds: ["general", "general"] });
    expect([...changes.channelIds]).toEqual(["general"]);
    expect(changes.threadIds.size).toBe(0);
  });
});

describe("ChatPaneStores", () => {
  it("tells every other pane about a change and never the pane that made it", () => {
    const panes = new ChatPaneStores<ReturnType<typeof store>>();
    const primary = store();
    const secondary = store();
    panes.add(primary);
    panes.add(secondary);
    const changes = chatWindowChanges({ threadIds: ["thread"] });
    panes.notifyOthers(secondary, changes);
    expect(primary.received).toEqual([changes]);
    expect(secondary.received).toEqual([]);
  });

  it("does not notify for empty changes", () => {
    const panes = new ChatPaneStores<ReturnType<typeof store>>();
    const primary = store();
    const secondary = store();
    panes.add(primary);
    panes.add(secondary);
    panes.notifyOthers(primary, chatWindowChanges({}));
    expect(secondary.refreshFromOtherPane).not.toHaveBeenCalled();
  });

  it("lists each pane once in creation order", () => {
    const panes = new ChatPaneStores<ReturnType<typeof store>>();
    const primary = store();
    const secondary = store();
    panes.add(primary);
    panes.add(secondary);
    panes.add(primary);
    expect(panes.all()).toEqual([primary, secondary]);
  });
});
