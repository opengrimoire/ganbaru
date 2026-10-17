import type * as ReviewDiffRuntimeModule from "./diff-runtime";

let runtimeModulePromise: Promise<typeof ReviewDiffRuntimeModule> | null = null;

/** Loads the enhanced Review renderer once and shares it across intent and panel activation. */
export function loadChatReviewDiffRuntime(): Promise<typeof ReviewDiffRuntimeModule> {
  runtimeModulePromise ??= import("./diff-runtime");
  return runtimeModulePromise;
}
