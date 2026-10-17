export interface WindowSyncEnvelope<TPayload> {
  sourceId: string;
  payload: TPayload;
}

const windowSyncSourceId =
  globalThis.crypto?.randomUUID?.() ??
  `window-${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

export function ownWindowSyncSourceId(): string {
  return windowSyncSourceId;
}

export function createWindowSyncEnvelope<TPayload>(
  payload: TPayload,
): WindowSyncEnvelope<TPayload> {
  return {
    sourceId: windowSyncSourceId,
    payload,
  };
}

export function isWindowSyncEnvelope<TPayload>(
  value: unknown,
  isPayload: (payload: unknown) => payload is TPayload,
): value is WindowSyncEnvelope<TPayload> {
  if (!isRecord(value)) return false;
  return (
    typeof value.sourceId === "string" &&
    isPayload(value.payload)
  );
}

export function isForeignWindowSyncEnvelope(
  envelope: WindowSyncEnvelope<unknown>,
): boolean {
  return envelope.sourceId !== windowSyncSourceId;
}
