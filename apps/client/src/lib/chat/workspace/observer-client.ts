import type {
  ChatWorkspaceChangeBatch,
  ChatWorkspaceObserverStatusRead,
} from "$lib/chat/contracts";

type ChangeListener = (batch: ChatWorkspaceChangeBatch) => void;
type StatusListener = (status: ChatWorkspaceObserverStatusRead | null) => void;

const changeListeners = new Set<ChangeListener>();
const statusListeners = new Set<StatusListener>();
let currentStatus: ChatWorkspaceObserverStatusRead | null = null;

/** Subscribes to generation-filtered changes for the active Chat workspace. */
export function subscribeChatWorkspaceChanges(listener: ChangeListener): () => void {
  changeListeners.add(listener);
  return () => changeListeners.delete(listener);
}

/** Subscribes to the active workspace observer mode and degradation state. */
export function subscribeChatWorkspaceObserverStatus(listener: StatusListener): () => void {
  statusListeners.add(listener);
  listener(currentStatus);
  return () => statusListeners.delete(listener);
}

export function publishChatWorkspaceChange(batch: ChatWorkspaceChangeBatch): void {
  for (const listener of changeListeners) listener(batch);
}

export function publishChatWorkspaceObserverStatus(
  status: ChatWorkspaceObserverStatusRead | null,
): void {
  currentStatus = status;
  for (const listener of statusListeners) listener(status);
}
