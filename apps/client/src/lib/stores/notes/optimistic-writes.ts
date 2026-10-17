export interface NotesOptimisticWriteTracker {
  track: (blockIds: readonly string[], persistence: Promise<void>) => void;
  pending: (blockId: string) => Promise<void> | null;
  has: (blockId: string) => boolean;
  flush: () => Promise<void>;
}

/** Tracks the latest optimistic persistence operation for each affected block. */
export function createNotesOptimisticWriteTracker(): NotesOptimisticWriteTracker {
  const pendingWrites = new Map<string, Promise<void>>();

  function track(blockIds: readonly string[], persistence: Promise<void>): void {
    for (const blockId of blockIds) pendingWrites.set(blockId, persistence);
    const cleanup = () => {
      for (const blockId of blockIds) {
        if (pendingWrites.get(blockId) === persistence) pendingWrites.delete(blockId);
      }
    };
    void persistence.then(cleanup, cleanup);
  }

  return {
    track,
    pending: (blockId) => pendingWrites.get(blockId) ?? null,
    has: (blockId) => pendingWrites.has(blockId),
    flush: async () => {
      await Promise.all([...new Set(pendingWrites.values())]);
    },
  };
}
