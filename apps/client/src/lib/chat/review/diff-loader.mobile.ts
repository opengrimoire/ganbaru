import type * as ReviewDiffRuntimeModule from "./diff-runtime";

/** Rejects on mobile so the enhanced desktop diff runtime stays outside mobile bundles. */
export function loadChatReviewDiffRuntime(): Promise<typeof ReviewDiffRuntimeModule> {
  return Promise.reject(new Error("Enhanced Chat diffs are unavailable on mobile"));
}
