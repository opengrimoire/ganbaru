import * as chatApi from "$lib/api/chat";
import type {
  ChatAttachmentId,
  ChatDraftMention,
  ChatDraftRead,
  ChatThreadId,
  ProjectWorkingFolderId,
  InteractionMode,
  ProviderInstanceId,
  SafetyMode,
  SaveChatDraftRequest,
  VersionedJson,
} from "$lib/chat/contracts";
import { chatErrorMessage } from "$lib/chat/error-presentation";

export interface ChatDraftApi {
  read(draftId: string): Promise<ChatDraftRead | null>;
  save(draft: SaveChatDraftRequest): Promise<ChatDraftRead>;
  delete(draftId: string): Promise<boolean>;
}

export interface ChatComposerSnapshot {
  draftId: string | null;
  workingFolderId: ProjectWorkingFolderId | null;
  threadId: ChatThreadId | null;
  text: string;
  richContent: VersionedJson | null;
  attachmentIds: ChatAttachmentId[];
  mentions: ChatDraftMention[];
  providerInstanceId: ProviderInstanceId | null;
  modelSelection: VersionedJson | null;
  safetyMode: SafetyMode | null;
  interactionMode: InteractionMode | null;
  sentSnapshot: VersionedJson | null;
  loading: boolean;
  saving: boolean;
  dirty: boolean;
  error: string | null;
}

export interface ChatComposerSeed {
  providerInstanceId: ProviderInstanceId | null;
  modelSelection: VersionedJson | null;
  safetyMode: SafetyMode | null;
  interactionMode: InteractionMode | null;
}

const DRAFT_SCHEMA_VERSION = 1;
const SAVE_DEBOUNCE_MS = 300;

const nativeApi: ChatDraftApi = {
  read: chatApi.readChatDraft,
  save: chatApi.saveChatDraft,
  delete: chatApi.deleteChatDraft,
};

export class ChatComposerController {
  private state: ChatComposerSnapshot = emptySnapshot();
  private revision = 0;
  private generation = 0;
  private saveTimer: ReturnType<typeof setTimeout> | null = null;
  private saveChain: Promise<void> = Promise.resolve();
  private readonly listeners = new Set<(snapshot: ChatComposerSnapshot) => void>();

  public constructor(
    private readonly api: ChatDraftApi = nativeApi,
    private readonly debounceMs = SAVE_DEBOUNCE_MS,
  ) {}

  public snapshot(): ChatComposerSnapshot {
    return {
      ...this.state,
      attachmentIds: [...this.state.attachmentIds],
      mentions: this.state.mentions.map((mention) => ({ ...mention })),
    };
  }

  public subscribe(listener: (snapshot: ChatComposerSnapshot) => void): () => void {
    this.listeners.add(listener);
    listener(this.snapshot());
    return () => this.listeners.delete(listener);
  }

  public async bind(
    workingFolderId: ProjectWorkingFolderId,
    threadId: ChatThreadId | null,
    seed: ChatComposerSeed | null = null,
    draftScopeId: string | null = null,
  ): Promise<void> {
    const pendingFlush = this.flush();
    const generation = ++this.generation;
    const draftId = draftScopeId ?? chatDraftId(workingFolderId, threadId);
    this.state = {
      ...emptySnapshot(),
      draftId,
      workingFolderId,
      threadId,
      providerInstanceId: seed?.providerInstanceId ?? null,
      modelSelection: seed?.modelSelection ?? null,
      safetyMode: seed?.safetyMode ?? null,
      interactionMode: seed?.interactionMode ?? null,
      loading: true,
    };
    this.notify();
    try {
      await pendingFlush;
      if (generation !== this.generation) return;
      const stored = await this.api.read(draftId);
      if (generation !== this.generation) return;
      if (stored === null) {
        this.state = { ...this.state, loading: false };
      } else {
        const restored = snapshotFromDraft(stored);
        const folderChanged = draftScopeId !== null && restored.workingFolderId !== workingFolderId;
        const threadChanged = draftScopeId !== null && restored.threadId !== threadId;
        this.state = draftScopeId === null
          ? restored
          : {
              ...restored,
              draftId,
              workingFolderId,
              threadId,
              attachmentIds: folderChanged ? [] : restored.attachmentIds,
              mentions: folderChanged ? [] : restored.mentions,
              dirty: folderChanged || threadChanged,
            };
      }
      this.revision = 0;
      this.notify();
    } catch (error: unknown) {
      if (generation !== this.generation) return;
      this.state = { ...this.state, loading: false, error: chatErrorMessage(error, "Chat draft could not be loaded") };
      this.notify();
      throw error;
    }
  }

  public setText(text: string): void {
    this.change({ text, richContent: null });
  }

  public setRichContent(text: string, richContent: VersionedJson): void {
    this.change({ text, richContent });
  }

  public setAttachments(attachmentIds: ChatAttachmentId[]): void {
    this.change({ attachmentIds: [...attachmentIds] });
  }

  public setMentions(mentions: ChatDraftMention[]): void {
    this.change({ mentions: mentions.map((mention) => ({ ...mention })) });
  }

  public setProvider(providerInstanceId: ProviderInstanceId | null): void {
    this.change({ providerInstanceId });
  }

  public setModelSelection(modelSelection: VersionedJson | null): void {
    this.change({ modelSelection });
  }

  public setModes(safetyMode: SafetyMode | null, interactionMode: InteractionMode | null): void {
    this.change({ safetyMode, interactionMode });
  }

  public async clear(): Promise<void> {
    this.cancelTimer();
    const draftId = this.state.draftId;
    if (draftId === null) return;
    await this.saveChain;
    await this.api.delete(draftId);
    this.revision += 1;
    this.state = {
      ...emptySnapshot(),
      draftId,
      workingFolderId: this.state.workingFolderId,
      threadId: this.state.threadId,
    };
    this.notify();
  }

  /** Drops the active vault binding without saving it into a newly activated vault. */
  public reset(): void {
    this.cancelTimer();
    this.generation += 1;
    this.revision = 0;
    this.state = emptySnapshot();
    this.notify();
  }

  public markSent(): void {
    const sentSnapshot: VersionedJson = {
      schemaVersion: DRAFT_SCHEMA_VERSION,
      value: {
        text: this.state.text,
        richContent: this.state.richContent ? {
          schemaVersion: this.state.richContent.schemaVersion,
          value: this.state.richContent.value,
        } : null,
        attachmentIds: [...this.state.attachmentIds],
        mentions: this.state.mentions.map((mention) => ({ ...mention })),
      },
    };
    this.change({ text: "", richContent: null, attachmentIds: [], mentions: [], sentSnapshot });
  }

  public restoreSentSnapshot(): boolean {
    const sent = parseSentSnapshot(this.state.sentSnapshot);
    if (sent === null) return false;
    this.change({
      text: sent.text,
      richContent: sent.richContent,
      attachmentIds: sent.attachmentIds,
      mentions: sent.mentions,
    });
    return true;
  }

  public async flush(): Promise<void> {
    this.cancelTimer();
    if (!this.state.dirty) {
      await this.saveChain;
      return;
    }
    const payload = this.payload();
    if (payload === null) return;
    const revision = this.revision;
    this.state = { ...this.state, saving: true, error: null };
    this.notify();
    this.saveChain = this.saveChain.then(async () => {
      try {
        await this.api.save(payload);
        if (this.state.draftId === payload.id && this.revision === revision) {
          this.state = { ...this.state, dirty: false, saving: false };
          this.notify();
        }
      } catch (error: unknown) {
        if (this.state.draftId === payload.id) {
          this.state = { ...this.state, saving: false, error: chatErrorMessage(error, "Chat draft could not be saved") };
          this.notify();
        }
        throw error;
      }
    });
    await this.saveChain;
  }

  public dispose(): Promise<void> {
    this.generation += 1;
    return this.flush();
  }

  private change(patch: Partial<ChatComposerSnapshot>): void {
    if (this.state.draftId === null || this.state.loading) return;
    this.revision += 1;
    this.state = { ...this.state, ...patch, dirty: true, error: null };
    this.notify();
    this.cancelTimer();
    this.saveTimer = setTimeout(() => {
      void this.flush().catch(() => undefined);
    }, this.debounceMs);
  }

  private payload(): SaveChatDraftRequest | null {
    const { draftId: id, workingFolderId, threadId } = this.state;
    if (id === null || workingFolderId === null) return null;
    return {
      id,
      workingFolderId,
      threadId,
      text: this.state.text,
      richContent: this.state.richContent,
      attachmentIds: [...this.state.attachmentIds],
      mentions: {
        schemaVersion: DRAFT_SCHEMA_VERSION,
        value: this.state.mentions.map((mention) => ({
          relativePath: mention.relativePath,
          kind: mention.kind,
          ignored: mention.ignored,
        })),
      },
      providerInstanceId: this.state.providerInstanceId,
      modelSelection: this.state.modelSelection,
      safetyMode: this.state.safetyMode,
      interactionMode: this.state.interactionMode,
      sentSnapshot: this.state.sentSnapshot,
    };
  }

  private cancelTimer(): void {
    if (this.saveTimer !== null) clearTimeout(this.saveTimer);
    this.saveTimer = null;
  }

  private notify(): void {
    const snapshot = this.snapshot();
    for (const listener of this.listeners) listener(snapshot);
  }
}

export function chatDraftId(workingFolderId: ProjectWorkingFolderId, threadId: ChatThreadId | null): string {
  return `workspace:${workingFolderId}:thread:${threadId ?? "new"}`;
}

function emptySnapshot(): ChatComposerSnapshot {
  return {
    draftId: null,
    workingFolderId: null,
    threadId: null,
    text: "",
    richContent: null,
    attachmentIds: [],
    mentions: [],
    providerInstanceId: null,
    modelSelection: null,
    safetyMode: null,
    interactionMode: null,
    sentSnapshot: null,
    loading: false,
    saving: false,
    dirty: false,
    error: null,
  };
}

function snapshotFromDraft(draft: ChatDraftRead): ChatComposerSnapshot {
  return {
    draftId: draft.id,
    workingFolderId: draft.workingFolderId,
    threadId: draft.threadId,
    text: draft.text,
    richContent: draft.richContent,
    attachmentIds: [...draft.attachmentIds],
    mentions: parseDraftMentions(draft.mentions),
    providerInstanceId: draft.providerInstanceId,
    modelSelection: draft.modelSelection,
    safetyMode: draft.safetyMode,
    interactionMode: draft.interactionMode,
    sentSnapshot: draft.sentSnapshot,
    loading: false,
    saving: false,
    dirty: false,
    error: null,
  };
}

export function parseDraftMentions(value: VersionedJson): ChatDraftMention[] {
  if (!Array.isArray(value.value)) return [];
  return value.value.flatMap((entry) => {
    if (typeof entry !== "object" || entry === null || Array.isArray(entry)) return [];
    const relativePath = entry.relativePath;
    const kind = entry.kind;
    const ignored = entry.ignored;
    if (typeof relativePath !== "string" || (kind !== "file" && kind !== "directory") || typeof ignored !== "boolean") return [];
    return [{ relativePath, kind, ignored }];
  });
}

function parseSentSnapshot(value: VersionedJson | null): {
  text: string;
  richContent: VersionedJson | null;
  attachmentIds: ChatAttachmentId[];
  mentions: ChatDraftMention[];
} | null {
  if (!value || typeof value.value !== "object" || value.value === null || Array.isArray(value.value)) return null;
  const { text, richContent, attachmentIds, mentions } = value.value;
  if (typeof text !== "string" || !Array.isArray(attachmentIds) || !Array.isArray(mentions)) return null;
  const parsedRichContent = richContent === null || richContent === undefined
    ? null
    : parseVersionedJsonValue(richContent);
  if (richContent !== null && richContent !== undefined && parsedRichContent === null) return null;
  const parsedAttachmentIds = attachmentIds.filter((entry): entry is string => typeof entry === "string");
  return {
    text,
    richContent: parsedRichContent,
    attachmentIds: parsedAttachmentIds,
    mentions: parseDraftMentions({ schemaVersion: value.schemaVersion, value: mentions }),
  };
}

function parseVersionedJsonValue(value: unknown): VersionedJson | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const schemaVersion = Reflect.get(value, "schemaVersion");
  if (!Number.isInteger(schemaVersion) || typeof schemaVersion !== "number" || schemaVersion < 1) return null;
  return { schemaVersion, value: Reflect.get(value, "value") };
}
