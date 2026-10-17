import { describe, expect, it } from "vitest";
import { reviewSourceFallback } from "./session.svelte";
import type { ReviewDiffSource } from "$lib/chat/contracts";

const checkpoint: ReviewDiffSource = { kind: "checkpoint", range: "turn", turnId: "turn-1" };

describe("review source recovery", () => {
  it("uses structured reasons even when diagnostic wording changes", () => {
    expect(reviewSourceFallback({ code: "not_found", details: { reason: "checkpoint_pair" }, message: "Changed wording" }, checkpoint))
      .toEqual({ kind: "provider_turn", turnId: "turn-1" });
    expect(reviewSourceFallback({ code: "not_found", details: { reason: "checkpoint_pair" } }, { ...checkpoint, turnId: null }))
      .toEqual({ kind: "working_tree", mode: "all" });
    expect(reviewSourceFallback({ code: "not_found", details: { reason: "provider_turn" } }, { kind: "provider_turn", turnId: "turn-1" }))
      .toEqual({ kind: "working_tree", mode: "all" });
  });

  it.each([
    null, "A settled checkpoint pair is not available for this review",
    { code: "not_found", message: "A settled checkpoint pair is not available for this review" },
    { code: "permission", details: { reason: "checkpoint_pair" } },
    { code: "not_found", details: { reason: "provider_turn" } },
    { code: "not_found", details: null },
    { code: "not_found", details: "checkpoint_pair" },
  ])("does not hide unrelated or malformed errors: %j", (error) => {
    expect(reviewSourceFallback(error, checkpoint)).toBeNull();
  });
});
