import { describe, expect, it } from "vitest";
import type { ChatChannelPageRead } from "$lib/chat/contracts";
import { ChatConversationPageCache } from "./conversation-page-cache";

function page(channelId: string, revision = 1): ChatChannelPageRead {
  return { channelId, messages: [], previousCursor: null, revision };
}

describe("ChatConversationPageCache", () => {
  it("evicts the least recently used conversation at the entry limit", () => {
    const cache = new ChatConversationPageCache<ChatChannelPageRead>(2, 100_000, 8);
    cache.set("first", [page("first")]);
    cache.set("second", [page("second")]);
    expect(cache.get("first")).not.toBeNull();
    cache.set("third", [page("third")]);
    expect(cache.get("second")).toBeNull();
    expect(cache.get("first")).not.toBeNull();
  });

  it("bounds pages and returns copies", () => {
    const cache = new ChatConversationPageCache<ChatChannelPageRead>(2, 100_000, 2);
    cache.set("channel", [page("channel", 1), page("channel", 2), page("channel", 3)]);
    const restored = cache.get("channel");
    expect(restored).toHaveLength(2);
    restored?.splice(0);
    expect(cache.get("channel")).toHaveLength(2);
  });

  it("accepts reactive proxies from the conversation store", () => {
    const cache = new ChatConversationPageCache<ChatChannelPageRead>(2, 100_000, 8);
    const proxiedPage = new Proxy(page("channel"), {});
    const proxiedPages = new Proxy([proxiedPage], {});

    cache.set("channel", proxiedPages);

    expect(cache.get("channel")).toEqual([page("channel")]);
  });
});
