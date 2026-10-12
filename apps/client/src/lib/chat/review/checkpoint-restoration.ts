import * as chatApi from "$lib/api/chat";
import type {
  ChatRestorePreviewRead,
  ChatThreadId,
  ChatThreadShellRead,
} from "$lib/chat/contracts";

export interface ChatCheckpointRestoreRequest {
  threadId: ChatThreadId;
  checkpointId: string;
}

export interface RestoreChatCheckpointOptions {
  thread: Pick<ChatThreadShellRead, "id" | "revision">;
  checkpointId: string;
  confirm: (preview: ChatRestorePreviewRead) => boolean;
  onRestored: (threadId: ChatThreadId) => Promise<void>;
}

export function isChatCheckpointRestoreRequest(value: unknown): value is ChatCheckpointRestoreRequest {
  if (typeof value !== "object" || value === null) return false;
  const record = value as Record<string, unknown>;
  return typeof record.threadId === "string" && typeof record.checkpointId === "string";
}

/** Runs the preview, confirmation, and execution stages of checkpoint restoration. */
export async function restoreChatCheckpoint(options: RestoreChatCheckpointOptions): Promise<void> {
  const preview = await chatApi.previewChatCheckpointRestore(
    options.thread.id,
    options.checkpointId,
  );
  if (!options.confirm(preview)) return;
  await chatApi.executeChatCheckpointRestore({
    command: {
      clientCommandId: crypto.randomUUID(),
      expectedThreadRevision: options.thread.revision,
    },
    threadId: options.thread.id,
    previewId: preview.previewId,
    confirmed: true,
  });
  await options.onRestored(options.thread.id);
}
