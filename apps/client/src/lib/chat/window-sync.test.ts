import { describe, expect, it } from "vitest";
import { chatChangeKeys, parseChatChangeKeys } from "./window-sync";

describe("Chat window change keys", () => {
  it("round-trips thread and channel ids, including ids that contain the separator", () => {
    const keys = chatChangeKeys({ threadIds: ["thread-1", "a:b"], channelIds: ["channel-1"] });

    const changes = parseChatChangeKeys(keys);

    expect([...changes.threadIds]).toEqual(["thread-1", "a:b"]);
    expect([...changes.channelIds]).toEqual(["channel-1"]);
  });

  it("collapses repeated ids and ignores unknown or empty keys", () => {
    const changes = parseChatChangeKeys([
      "thread:thread-1",
      "thread:thread-1",
      "thread:",
      "channel:",
      "draft:draft-1",
      "channel-1",
    ]);

    expect([...changes.threadIds]).toEqual(["thread-1"]);
    expect([...changes.channelIds]).toEqual([]);
  });

  it("produces no keys when nothing changed", () => {
    expect(chatChangeKeys({})).toEqual([]);
  });
});
