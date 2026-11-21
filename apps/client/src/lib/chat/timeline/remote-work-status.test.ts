import { describe, expect, it } from "vitest";
import { remoteWorkStatus, remoteWorkStatusIcon } from "./remote-work-status";

const noRows = { rowsShown: false, claimExpected: false };

describe("remoteWorkStatus", () => {
  it("shows no status while the run's rows already show the work progressing", () => {
    expect(remoteWorkStatus("working", { rowsShown: true, claimExpected: false })).toBeNull();
    expect(remoteWorkStatus("queued", { rowsShown: true, claimExpected: true })).toBeNull();
  });

  it("reads queued work as starting while a linked desktop is about to claim it", () => {
    expect(remoteWorkStatus("queued", { rowsShown: false, claimExpected: true })).toBe("starting");
    expect(remoteWorkStatus("queued", noRows)).toBe("queued");
  });

  it("keeps the replicated state when no rows show", () => {
    expect(remoteWorkStatus("working", noRows)).toBe("working");
    expect(remoteWorkStatus("working", { rowsShown: false, claimExpected: true })).toBe("working");
  });

  it("keeps a waiting state even while rows show, since the rows do not say what the work waits for", () => {
    expect(remoteWorkStatus("waiting_for_answer", { rowsShown: true, claimExpected: false })).toBe("waiting_for_answer");
    expect(remoteWorkStatus("waiting_for_approval", { rowsShown: true, claimExpected: false })).toBe("waiting_for_approval");
  });
});

describe("remoteWorkStatusIcon", () => {
  it("spins only while the work runs", () => {
    expect(remoteWorkStatusIcon("working")).toBe("progress");
  });

  it("shows work about to run as text alone, like a thinking step", () => {
    expect(remoteWorkStatusIcon("starting")).toBe("none");
    expect(remoteWorkStatusIcon("queued")).toBe("none");
  });

  it("names what waiting work waits for and leaves finished work without an icon", () => {
    expect(remoteWorkStatusIcon("waiting_for_answer")).toBe("question");
    expect(remoteWorkStatusIcon("waiting_for_approval")).toBe("approval");
    expect(remoteWorkStatusIcon("completed")).toBe("none");
    expect(remoteWorkStatusIcon("ready_for_review")).toBe("none");
  });
});
