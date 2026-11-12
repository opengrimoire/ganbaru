import { describe, expect, it } from "vitest";
import type { ChatProjectionKey } from "$lib/chat/contracts";
import {
  fullNativeChange,
  mergeNativeChanges,
  nativeChangeFromNotification,
  planNativeRefresh,
  type ChatNativeChange,
} from "./native-change-routing";

function change(keys: ChatProjectionKey[], sinceSequence = 5, sequence = sinceSequence): ChatNativeChange {
  return nativeChangeFromNotification({
    threadId: "thread-1",
    sequence,
    sinceSequence,
    revision: 1,
    changedProjectionKeys: keys,
  });
}

describe("planNativeRefresh", () => {
  it("never rereads the timeline for an organizational key alone", () => {
    expect(planNativeRefresh(change(["organizational"]), true, 4)).toEqual({
      shell: false,
      organizational: true,
      timeline: false,
      interaction: false,
    });
  });

  it("rereads nothing for a messages key on an unselected thread", () => {
    expect(planNativeRefresh(change(["messages"]), false, 4)).toEqual({
      shell: false,
      organizational: false,
      timeline: false,
      interaction: false,
    });
  });

  it("rereads the timeline for row keys and the interaction for pending requests on the selected thread", () => {
    expect(planNativeRefresh(change(["activities"]), true, 4)).toMatchObject({ timeline: true, interaction: false });
    expect(planNativeRefresh(change(["pending_requests"]), true, 4)).toMatchObject({ timeline: false, interaction: true });
    expect(planNativeRefresh(change(["turns", "thread"]), true, 4)).toEqual({
      shell: true,
      organizational: false,
      timeline: true,
      interaction: true,
    });
  });

  it("refreshes the interaction for a change that names no projection", () => {
    expect(planNativeRefresh(change([]), true, 4)).toEqual({
      shell: false,
      organizational: false,
      timeline: false,
      interaction: true,
    });
  });

  it("rereads everything for an unknown key or a skipped sequence", () => {
    const everything = { shell: true, organizational: true, timeline: true, interaction: true };
    expect(planNativeRefresh(change(["unknown"]), true, 4)).toEqual(everything);
    expect(planNativeRefresh(change(["messages"], 7), true, 4)).toEqual(everything);
    expect(planNativeRefresh(fullNativeChange(), true, 4)).toEqual(everything);
  });

  it("does not treat a repeated or first sequence as skipped", () => {
    expect(planNativeRefresh(change(["messages"], 4), true, 4).shell).toBe(false);
    expect(planNativeRefresh(change(["messages"], 9), true, null).shell).toBe(false);
  });
});

describe("mergeNativeChanges", () => {
  it("unions keys and spans the sequence range of both changes", () => {
    const merged = mergeNativeChanges(change(["messages"], 6, 6), change(["thread"], 5, 8));

    expect([...merged.keys].sort()).toEqual(["messages", "thread"]);
    expect(merged.sinceSequence).toBe(5);
    expect(merged.sequence).toBe(8);
  });

  it("keeps the known sequences when one change has none", () => {
    const merged = mergeNativeChanges(fullNativeChange(), change(["messages"], 6, 7));

    expect(merged.keys.has("unknown")).toBe(true);
    expect(merged.sinceSequence).toBe(6);
    expect(merged.sequence).toBe(7);
  });
});
