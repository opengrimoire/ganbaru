import type { ProjectTaskDetailData } from "$lib/projects/types";

interface CacheEntry {
  detail: ProjectTaskDetailData;
  bytes: number;
}

/** Keeps recently used task details within a serialized byte budget. */
export class ProjectTaskDetailCache {
  readonly #entries = new Map<string, CacheEntry>();
  #bytes = 0;

  constructor(private readonly maxBytes: number) {
    if (!Number.isSafeInteger(maxBytes) || maxBytes <= 0) {
      throw new Error("maxBytes must be a positive safe integer");
    }
  }

  get(taskId: string, updatedAt: string): ProjectTaskDetailData | undefined {
    const key = `${taskId}\u0000${updatedAt}`;
    const entry = this.#entries.get(key);
    if (!entry) return undefined;
    this.#entries.delete(key);
    this.#entries.set(key, entry);
    return entry.detail;
  }

  set(detail: ProjectTaskDetailData): void {
    const key = `${detail.task.id}\u0000${detail.task.updatedAt}`;
    const bytes = new TextEncoder().encode(JSON.stringify(detail)).byteLength;
    this.deleteTask(detail.task.id);
    if (bytes > this.maxBytes) return;
    this.#entries.set(key, { detail, bytes });
    this.#bytes += bytes;
    while (this.#bytes > this.maxBytes) {
      const oldestKey = this.#entries.keys().next().value as string | undefined;
      if (!oldestKey) break;
      const oldest = this.#entries.get(oldestKey);
      this.#entries.delete(oldestKey);
      this.#bytes -= oldest?.bytes ?? 0;
    }
  }

  deleteTask(taskId: string): void {
    const prefix = `${taskId}\u0000`;
    for (const [key, entry] of this.#entries) {
      if (!key.startsWith(prefix)) continue;
      this.#entries.delete(key);
      this.#bytes -= entry.bytes;
    }
  }
}
