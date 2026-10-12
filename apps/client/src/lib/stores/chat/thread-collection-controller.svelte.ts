import * as chatApi from "$lib/api/chat";
import type { ChatThreadId, ChatThreadShellRead } from "$lib/chat/contracts";
import { isDirectChatThreadShell } from "$lib/chat/shell-model";

const RECENT_THREAD_WINDOW = 200;

export interface ChatThreadCollectionControllerOptions {
  selectedThreadId: () => ChatThreadId | null;
  selectThread: (threadId: ChatThreadId | null) => void;
}

/** Owns provider-thread collections, archival loading, and thread mutations. */
export class ChatThreadCollectionController {
  activeThreads = $state<ChatThreadShellRead[]>([]);
  archivedThreads = $state<ChatThreadShellRead[]>([]);
  archivedThreadsLoading = $state(false);

  private generation = 0;
  private archivedThreadsPromise: Promise<void> | null = null;
  private archivedThreadsLoaded = false;

  constructor(private readonly options: ChatThreadCollectionControllerOptions) {}

  reset(): void {
    this.resetWindow();
  }

  /** Clears loaded windows and invalidates any in-flight archive load. */
  resetWindow(): void {
    this.generation += 1;
    this.activeThreads = [];
    this.archivedThreads = [];
    this.archivedThreadsLoading = false;
    this.archivedThreadsPromise = null;
    this.archivedThreadsLoaded = false;
  }

  find(threadId: ChatThreadId): ChatThreadShellRead | null {
    return [...this.activeThreads, ...this.archivedThreads]
      .find((thread) => thread.id === threadId) ?? null;
  }

  async ensureArchived(): Promise<void> {
    if (this.archivedThreadsLoaded) return;
    if (this.archivedThreadsPromise) return this.archivedThreadsPromise;
    const generation = this.generation;
    const load = (async () => {
      this.archivedThreadsLoading = true;
      try {
        const threads = await chatApi.listChatThreadWindow(
          null,
          true,
          RECENT_THREAD_WINDOW,
        );
        if (generation !== this.generation) return;
        const threadsById = new Map(this.archivedThreads.map((thread) => [thread.id, thread]));
        for (const thread of threads) threadsById.set(thread.id, thread);
        this.archivedThreads = [...threadsById.values()].sort((left, right) => (
          right.lastActivityAt.localeCompare(left.lastActivityAt)
        ));
        this.archivedThreadsLoaded = true;
      } finally {
        if (generation === this.generation) {
          this.archivedThreadsLoading = false;
          this.archivedThreadsPromise = null;
        }
      }
    })();
    this.archivedThreadsPromise = load;
    await load;
  }

  async rename(thread: ChatThreadShellRead, title: string): Promise<void> {
    this.upsert(await chatApi.renameChatThread(thread.id, title, thread.revision));
  }

  async setRead(thread: ChatThreadShellRead, read: boolean): Promise<void> {
    this.upsert(await chatApi.setChatThreadRead(thread.id, read, thread.revision));
  }

  async fork(thread: ChatThreadShellRead, title: string): Promise<void> {
    const forked = await chatApi.forkChatThread(thread.id, crypto.randomUUID(), title);
    this.upsert(forked);
    this.options.selectThread(forked.id);
  }

  async archive(thread: ChatThreadShellRead): Promise<void> {
    const archived = await chatApi.archiveChatThread(thread.id, thread.revision);
    this.upsert(archived);
    if (this.options.selectedThreadId() === thread.id) this.options.selectThread(archived.id);
  }

  async restore(thread: ChatThreadShellRead): Promise<void> {
    const restored = await chatApi.restoreChatThread(thread.id, thread.revision);
    this.upsert(restored);
    this.options.selectThread(restored.id);
  }

  async delete(thread: ChatThreadShellRead): Promise<void> {
    await chatApi.deleteChatThreadPermanently(thread.id, thread.revision, thread.title);
    this.activeThreads = this.activeThreads.filter((entry) => entry.id !== thread.id);
    this.archivedThreads = this.archivedThreads.filter((entry) => entry.id !== thread.id);
    if (this.options.selectedThreadId() === thread.id) this.options.selectThread(null);
  }

  upsert(thread: ChatThreadShellRead): void {
    if (!isDirectChatThreadShell(thread)) return;
    const target = thread.archivedAt ? this.archivedThreads : this.activeThreads;
    const next = target.some((entry) => entry.id === thread.id)
      ? target.map((entry) => entry.id === thread.id ? thread : entry)
      : [thread, ...target];
    if (thread.archivedAt) {
      this.activeThreads = this.activeThreads.filter((entry) => entry.id !== thread.id);
      this.archivedThreads = next;
    } else {
      this.archivedThreads = this.archivedThreads.filter((entry) => entry.id !== thread.id);
      this.activeThreads = next;
    }
  }
}
