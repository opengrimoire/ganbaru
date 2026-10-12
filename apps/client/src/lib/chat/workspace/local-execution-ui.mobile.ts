import type ChatWorkspaceObserver from "$lib/components/chat/workspace/ChatWorkspaceObserver.svelte";
import type ChatWorkspacePanel from "$lib/components/chat/workspace/ChatWorkspacePanel.svelte";
import type ChatCommandMenu from "$lib/components/chat/composer/ChatCommandMenu.svelte";
import type ChatScratchManager from "$lib/components/settings/chat/ChatScratchManager.svelte";

export type ChatScratchManagerComponent = typeof ChatScratchManager;
export type StopChatChangeListener = () => void;

export interface ChatLocalExecutionComponents {
  CommandMenu: typeof ChatCommandMenu;
  WorkspaceObserver: typeof ChatWorkspaceObserver;
  WorkspacePanel: typeof ChatWorkspacePanel;
}

/** Keeps desktop execution surfaces outside mobile bundles. */
export const CHAT_LOCAL_EXECUTION_COMPONENTS: ChatLocalExecutionComponents | null = null;

/** Mobile Chat has no local code editor runtime to preload. */
export function preloadChatLocalExecutionUi(): void {}

/** Mobile Chat has no native provider process changes to observe. */
export function listenForChatChanges(
  _onPayload: (payload: unknown) => void,
): Promise<StopChatChangeListener> {
  return Promise.resolve(() => {});
}

/** Detached windows are unavailable on mobile. */
export function openDetachedChatWindow(): Promise<void> {
  return Promise.reject(new Error("Detached Chat windows are unavailable on mobile"));
}

/** Private local scratch inspection is unavailable on mobile. */
export function loadChatScratchManager(): Promise<ChatScratchManagerComponent> {
  return Promise.reject(new Error("Private Chat scratch inspection is unavailable on mobile"));
}
