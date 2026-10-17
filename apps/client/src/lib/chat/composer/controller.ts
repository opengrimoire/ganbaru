import * as chatApi from "$lib/api/chat";
import type {
  ChatAttachmentId,
  ChatDraftBase,
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
import { chatErrorCode, chatErrorMessage } from "$lib/chat/error-presentation";
import { chatDraftWindowSync, type ChatDraftWindowSync } from "./window-sync";

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
  /** Another window saved this draft while it had unsaved edits here, so its version replaced them. */
  changedElsewhere: boolean;
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
const ANY_BASE: ChatDraftBase = { kind: "any" };
const NO_BASE: ChatDraftBase = { kind: "none" };

type DraftContent = Omit<SaveChatDraftRequest, "base">;

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
  /** The stored version of the bound draft that the next save expects to replace. */
  private base: { draftId: string; base: ChatDraftBase } | null = null;
  /** Set by a send or its rollback, whose content must be stored whatever another window saved. */
  private overwrite = false;
  /** Counts replacements of the bound draft by its stored version, which supersede saves queued before them. */
  private adoptions = 0;
  private listening: Promise<void> | null = null;
  private stopListening: (() => void) | null = null;
  private readonly listeners = new Set<(snapshot: ChatComposerSnapshot) => void>();

  public constructor(
    private readonly api: ChatDraftApi = nativeApi,
    private readonly debounceMs = SAVE_DEBOUNCE_MS,
    private readonly windowSync: ChatDraftWindowSync = chatDraftWindowSync,
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
    this.listenForOtherWindows();
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
      this.base = { draftId, base: draftBase(stored) };
      this.overwrite = false;
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
    this.windowSync.publish(draftId);
    this.base = { draftId, base: NO_BASE };
    this.overwrite = false;
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
    this.base = null;
    this.overwrite = false;
    this.state = emptySnapshot();
    this.notify();
  }

  /** Hides the notice that another window's version replaced local edits. */
  public dismissChangedElsewhere(): void {
    if (!this.state.changedElsewhere) return;
    this.state = { ...this.state, changedElsewhere: false };
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
    if (this.change({ text: "", richContent: null, attachmentIds: [], mentions: [], sentSnapshot })) this.overwrite = true;
  }

  public restoreSentSnapshot(): boolean {
    const sent = parseSentSnapshot(this.state.sentSnapshot);
    if (sent === null) return false;
    const changed = this.change({
      text: sent.text,
      richContent: sent.richContent,
      attachmentIds: sent.attachmentIds,
      mentions: sent.mentions,
    });
    if (changed) this.overwrite = true;
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
    const overwrite = this.overwrite;
    this.state = { ...this.state, saving: true, error: null };
    this.notify();
    const adoptions = this.adoptions;
    this.saveChain = this.saveChain.catch(() => undefined).then(async () => {
      // Content captured before a stored version replaced it would undo that replacement.
      if (adoptions !== this.adoptions) return;
      try {
        // The base is read when the save runs, after earlier saves in the chain have recorded theirs.
        const saved = await this.api.save({ ...payload, base: overwrite ? ANY_BASE : this.baseFor(payload.id) });
        if (this.base === null || this.base.draftId === saved.id) this.base = { draftId: saved.id, base: draftBase(saved) };
        this.windowSync.publish(saved.id);
        if (this.state.draftId === payload.id && this.revision === revision) {
          this.overwrite = false;
          this.state = { ...this.state, dirty: false, saving: false };
          this.notify();
        }
      } catch (error: unknown) {
        if (chatErrorCode(error) === "stale_revision") {
          await this.adoptStoredDraft(payload.id, "conflict");
          return;
        }
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
    this.stopListening?.();
    this.stopListening = null;
    return this.flush();
  }

  private change(patch: Partial<ChatComposerSnapshot>): boolean {
    if (this.state.draftId === null || this.state.loading) return false;
    this.revision += 1;
    this.state = { ...this.state, ...patch, dirty: true, changedElsewhere: false, error: null };
    this.notify();
    this.cancelTimer();
    this.saveTimer = setTimeout(() => {
      void this.flush().catch(() => undefined);
    }, this.debounceMs);
    return true;
  }

  private baseFor(draftId: string): ChatDraftBase {
    return this.base?.draftId === draftId ? this.base.base : ANY_BASE;
  }

  private listenForOtherWindows(): void {
    if (this.listening !== null) return;
    this.listening = this.windowSync.listen((draftIds) => this.refreshFromOtherWindow(draftIds)).then(
      (stop) => { this.stopListening = stop; },
      (error: unknown) => {
        this.listening = null;
        console.warn("Chat drafts from other windows could not be followed", error);
      },
    );
  }

  /** Shows a draft another window saved, unless this window has edits of its own to save first. */
  private refreshFromOtherWindow(draftIds: ReadonlySet<string>): void {
    const draftId = this.state.draftId;
    if (draftId === null || !draftIds.has(draftId) || this.state.dirty || this.state.loading) return;
    this.saveChain = this.saveChain.catch(() => undefined).then(() => this.adoptStoredDraft(draftId, "refresh"));
  }

  /**
   * Replaces the bound draft with its stored version. A refresh skips the replacement once local edits exist;
   * a conflict replaces them and flags the composer so it can say why.
   */
  private async adoptStoredDraft(draftId: string, reason: "conflict" | "refresh"): Promise<void> {
    const generation = this.generation;
    let stored: ChatDraftRead | null;
    try {
      stored = await this.api.read(draftId);
    } catch (error: unknown) {
      if (generation !== this.generation || this.state.draftId !== draftId) return;
      this.state = { ...this.state, saving: false, error: chatErrorMessage(error, "Chat draft could not be loaded") };
      this.notify();
      return;
    }
    if (generation !== this.generation || this.state.draftId !== draftId) return;
    if (reason === "refresh" && this.state.dirty) return;
    this.cancelTimer();
    this.revision += 1;
    this.overwrite = false;
    this.base = { draftId, base: draftBase(stored) };
    this.adoptions += 1;
    const { workingFolderId, threadId, providerInstanceId, modelSelection, safetyMode, interactionMode } = this.state;
    const changedElsewhere = reason === "conflict" || this.state.changedElsewhere;
    if (stored === null) {
      this.state = {
        ...emptySnapshot(),
        draftId, workingFolderId, threadId, providerInstanceId, modelSelection, safetyMode, interactionMode,
        changedElsewhere,
      };
    } else {
      const restored = snapshotFromDraft(stored);
      const folderChanged = restored.workingFolderId !== workingFolderId;
      this.state = {
        ...restored,
        workingFolderId,
        threadId,
        attachmentIds: folderChanged ? [] : restored.attachmentIds,
        mentions: folderChanged ? [] : restored.mentions,
        changedElsewhere,
      };
    }
    this.notify();
  }

  private payload(): DraftContent | null {
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
    changedElsewhere: false,
    error: null,
  };
}

function draftBase(stored: ChatDraftRead | null): ChatDraftBase {
  return stored === null ? NO_BASE : { kind: "saved", updatedAt: stored.updatedAt };
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
    changedElsewhere: false,
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
