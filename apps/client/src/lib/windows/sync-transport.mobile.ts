import type {
  WindowSyncTransportListener,
  WindowSyncTransportUnlisten,
} from "./sync-transport-contracts";

/** The mobile shell owns one webview, so there are no sibling windows to notify. */
export function emitWindowSync<T>(_eventName: string, _payload: T): Promise<void> {
  return Promise.resolve();
}

/** The mobile shell owns one webview, so it has no cross-window events to receive. */
export function listenWindowSync<T>(
  _eventName: string,
  _listener: WindowSyncTransportListener<T>,
): Promise<WindowSyncTransportUnlisten> {
  return Promise.resolve((): void => undefined);
}
