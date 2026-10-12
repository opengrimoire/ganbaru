import type * as CodeEditorRuntimeModule from "./code-editor-runtime";

let runtimeModulePromise: Promise<typeof CodeEditorRuntimeModule> | null = null;

/** Loads the shared CodeMirror runtime once without selecting a language grammar. */
export function loadChatCodeEditorRuntime(): Promise<typeof CodeEditorRuntimeModule> {
  runtimeModulePromise ??= import("./code-editor-runtime");
  return runtimeModulePromise;
}
