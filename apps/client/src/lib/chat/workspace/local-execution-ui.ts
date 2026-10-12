import { listen } from "@tauri-apps/api/event";
import ChatWorkspaceObserver from "$lib/components/chat/workspace/ChatWorkspaceObserver.svelte";
import ChatWorkspacePanel from "$lib/components/chat/workspace/ChatWorkspacePanel.svelte";
import ChatCommandMenu from "$lib/components/chat/composer/ChatCommandMenu.svelte";
import type ChatScratchManager from "$lib/components/settings/chat/ChatScratchManager.svelte";
import { openDetachedViewWindow } from "$lib/windows/detached";
import { loadChatCodeEditorRuntime } from "./code-editor-loader";

export type ChatScratchManagerComponent = typeof ChatScratchManager;
export type StopChatChangeListener = () => void;

export interface ChatLocalExecutionComponents {
  CommandMenu: typeof ChatCommandMenu;
  WorkspaceObserver: typeof ChatWorkspaceObserver;
  WorkspacePanel: typeof ChatWorkspacePanel;
}

/** Desktop-only Chat workspace tools kept in the desktop Chat shell chunk. */
export const CHAT_LOCAL_EXECUTION_COMPONENTS: ChatLocalExecutionComponents = {
  CommandMenu: ChatCommandMenu,
  WorkspaceObserver: ChatWorkspaceObserver,
  WorkspacePanel: ChatWorkspacePanel,
};

/** Warms the desktop code editor after the shared conversation shell loads. */
export function preloadChatLocalExecutionUi(): void {
  void loadChatCodeEditorRuntime().catch(() => undefined);
}

/** Subscribes the desktop shell to native Chat runtime changes. */
export function listenForChatChanges(
  onPayload: (payload: unknown) => void,
): Promise<StopChatChangeListener> {
  return listen<unknown>("chat://change", (event) => onPayload(event.payload));
}

/** Opens Chat in a detached desktop window. */
export function openDetachedChatWindow(): Promise<void> {
  return openDetachedViewWindow("chat");
}

/** Lazily loads the desktop private scratch inspector. */
export function loadChatScratchManager(): Promise<ChatScratchManagerComponent> {
  return import("$lib/components/settings/chat/ChatScratchManager.svelte")
    .then((module) => module.default);
}
