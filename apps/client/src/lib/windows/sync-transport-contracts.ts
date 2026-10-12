export interface WindowSyncTransportEvent<T> {
  payload: T;
}

export type WindowSyncTransportListener<T> = (
  event: WindowSyncTransportEvent<T>,
) => void;

export type WindowSyncTransportUnlisten = () => void;
