import { emit, listen } from "@tauri-apps/api/event";
import type {
  WindowSyncTransportListener,
  WindowSyncTransportUnlisten,
} from "./sync-transport-contracts";

/** Publish a synchronization payload to the app's desktop windows. */
export function emitWindowSync<T>(eventName: string, payload: T): Promise<void> {
  return emit(eventName, payload);
}

/** Listen for synchronization payloads from the app's other desktop windows. */
export function listenWindowSync<T>(
  eventName: string,
  listener: WindowSyncTransportListener<T>,
): Promise<WindowSyncTransportUnlisten> {
  return listen<T>(eventName, listener);
}
