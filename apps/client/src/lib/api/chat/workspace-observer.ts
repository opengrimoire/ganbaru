import { invoke } from "@tauri-apps/api/core";
import { ensureDbUrl } from "$lib/api/db";
import type {
  ChatWorkspaceObserverStatusRead,
  ProjectWorkingFolderId,
} from "$lib/chat/contracts";
import { parseChatWorkspaceObserverStatus } from "$lib/chat/validation";

/** Starts or reuses the single observer for the selected Chat workspace. */
export async function watchChatWorkspace(
  workingFolderId: ProjectWorkingFolderId,
  executionEnvironmentId: string | null,
): Promise<ChatWorkspaceObserverStatusRead> {
  return parseChatWorkspaceObserverStatus(await invoke<unknown>("chat_watch_workspace", {
    dbUrl: await ensureDbUrl(),
    workingFolderId,
    executionEnvironmentId,
  }));
}

/** Stops an observer only when the supplied generation is still active. */
export async function unwatchChatWorkspace(generation: number): Promise<boolean> {
  const result = await invoke<unknown>("chat_unwatch_workspace", { generation });
  if (typeof result !== "boolean") throw new Error("Chat workspace unwatch response is invalid");
  return result;
}
