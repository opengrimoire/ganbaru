import * as chatApi from "$lib/api/chat";
import type { ChatCodeEditorSelection } from "./code-editor-runtime";
import type {
  ChatWorkspaceChangeBatch,
  ProjectWorkingFolderFilePreview,
} from "$lib/chat/contracts";
import { chatErrorMessage } from "$lib/chat/error-presentation";
import { boundTerminalContext } from "./terminal-model";
import { workspacePreviewImpact } from "./change-model";
import { getChat } from "$lib/stores/chat.svelte";

interface FileMutationContext {
  requestId: number;
  scopeRevision: number;
  workingFolderId: string;
  executionEnvironmentId: string | null;
  relativePath: string;
}

export interface ChatFileEditorSessionOptions {
  selectedPath: () => string | null;
  setSelectedPath: (path: string) => void;
  refreshParentDirectory: (path: string) => Promise<void>;
  onReviewCreated: () => void;
  discardUnsavedMessage: () => string;
  confirmOverwriteMessage: () => string;
  confirmRecreateMessage: () => string;
  previewUnavailableMessage: () => string;
}

/** Owns editable file preview state, conflict recovery, and review selection actions. */
export class ChatFileEditorSession {
  preview = $state<ProjectWorkingFolderFilePreview | null>(null);
  draftText = $state("");
  editorSelection = $state<ChatCodeEditorSelection | null>(null);
  reviewComposerOpen = $state(false);
  reviewDraft = $state("");
  creatingReview = $state(false);
  saving = $state(false);
  saveConflict = $state(false);
  fileDeleted = $state(false);
  conflictDiskText = $state<string | null>(null);
  saveCopyPath = $state("");
  loading = $state(false);
  loadingVisible = $state(false);
  error = $state<string | null>(null);

  private readonly chat = getChat();
  private scopeRevision = 0;
  private previewRequestId = 0;
  private mutationRequestId = 0;
  private loadingTimer: number | null = null;

  constructor(private readonly options: ChatFileEditorSessionOptions) {}

  get dirty(): boolean {
    return this.preview?.text !== null
      && this.preview?.text !== undefined
      && this.draftText !== this.preview.text;
  }

  reset(): void {
    this.scopeRevision += 1;
    this.previewRequestId += 1;
    this.mutationRequestId += 1;
    this.finishLoading();
    this.preview = null;
    this.draftText = "";
    this.editorSelection = null;
    this.reviewComposerOpen = false;
    this.reviewDraft = "";
    this.creatingReview = false;
    this.saving = false;
    this.saveConflict = false;
    this.fileDeleted = false;
    this.conflictDiskText = null;
    this.saveCopyPath = "";
    this.error = null;
  }

  destroy(): void {
    this.scopeRevision += 1;
    this.previewRequestId += 1;
    this.mutationRequestId += 1;
    this.finishLoading();
  }

  setDraftText(text: string): void {
    this.draftText = text;
    if (this.fileDeleted && !this.saveCopyPath && this.preview) {
      this.saveCopyPath = fileConflictCopyPath(this.preview.relativePath);
    }
  }

  setSelection(selection: ChatCodeEditorSelection): void {
    this.editorSelection = selection;
  }

  closeReviewComposer(): void {
    this.reviewComposerOpen = false;
    this.reviewDraft = "";
  }

  async select(
    path: string,
    executionEnvironmentId = this.chat.selectedExecutionEnvironmentId,
    discardDirty = false,
  ): Promise<boolean> {
    const workingFolderId = this.chat.selectedWorkingFolderId;
    if (!workingFolderId) return false;
    if (!discardDirty
      && path !== this.preview?.relativePath
      && this.dirty
      && !window.confirm(this.options.discardUnsavedMessage())) return false;
    this.mutationRequestId += 1;
    this.saving = false;
    const scopeRevision = this.scopeRevision;
    const requestId = ++this.previewRequestId;
    const previousPath = this.preview?.relativePath ?? null;
    this.options.setSelectedPath(path);
    this.beginLoading();
    this.error = null;
    let opened = false;
    try {
      const result = await chatApi.previewProjectWorkingFolderFile(
        workingFolderId,
        path,
        executionEnvironmentId,
      );
      if (requestId === this.previewRequestId
        && this.scopeIsCurrent(workingFolderId, executionEnvironmentId, scopeRevision)) {
        this.applyPreview(result);
        opened = true;
      }
    } catch (reason: unknown) {
      if (requestId === this.previewRequestId
        && this.scopeIsCurrent(workingFolderId, executionEnvironmentId, scopeRevision)) {
        this.error = chatErrorMessage(reason);
        if (previousPath) this.options.setSelectedPath(previousPath);
      }
    } finally {
      if (requestId === this.previewRequestId) this.finishLoading();
    }
    return opened;
  }

  async reconcileWorkspaceChange(batch: ChatWorkspaceChangeBatch): Promise<void> {
    const currentPath = this.options.selectedPath();
    if (!currentPath) return;
    const impact = workspacePreviewImpact(batch, currentPath, this.dirty);
    if (impact.kind === "none") return;
    if (impact.kind === "renamed" || impact.kind === "deleted_after_rename") {
      this.options.setSelectedPath(impact.relativePath);
      await this.refreshFromDisk(impact.relativePath, true, currentPath);
      return;
    }
    await this.refreshFromDisk(currentPath, false);
  }

  async refreshFromDisk(path: string, moved: boolean, previousPath = path): Promise<void> {
    const workingFolderId = this.chat.selectedWorkingFolderId;
    const current = this.preview;
    if (!workingFolderId) return;
    const environmentId = this.chat.selectedExecutionEnvironmentId;
    const scopeRevision = this.scopeRevision;
    const requestId = ++this.previewRequestId;
    try {
      const disk = await chatApi.previewProjectWorkingFolderFile(workingFolderId, path, environmentId);
      if (requestId !== this.previewRequestId
        || !this.scopeIsCurrent(workingFolderId, environmentId, scopeRevision)
        || (this.options.selectedPath() !== path && this.options.selectedPath() !== previousPath)) return;
      const diskChanged = moved || disk.contentRevision !== current?.contentRevision;
      if (this.dirty && diskChanged) {
        if (moved) this.preview = disk;
        this.fileDeleted = false;
        this.saveConflict = true;
        this.conflictDiskText = null;
        if (!this.saveCopyPath) this.saveCopyPath = fileConflictCopyPath(path);
        return;
      }
      if (!diskChanged) return;
      this.preview = disk;
      this.draftText = disk.text ?? "";
      this.clearConflictState();
    } catch (reason: unknown) {
      if (requestId !== this.previewRequestId
        || !this.scopeIsCurrent(workingFolderId, environmentId, scopeRevision)
        || (this.options.selectedPath() !== path && this.options.selectedPath() !== previousPath)) return;
      if (errorCode(reason) === "not_found") {
        this.fileDeleted = true;
        this.saveConflict = this.dirty;
        this.conflictDiskText = null;
        if (this.dirty && !this.saveCopyPath) this.saveCopyPath = fileConflictCopyPath(path);
        return;
      }
      this.error = chatErrorMessage(reason);
    }
  }

  async save(): Promise<void> {
    if (this.fileDeleted) {
      await this.recreateDeletedFile();
      return;
    }
    if (this.saveConflict) return;
    const workingFolderId = this.chat.selectedWorkingFolderId;
    const current = this.preview;
    if (!workingFolderId || !current?.contentRevision || !this.dirty || this.saving) return;
    const mutation = this.beginMutation(workingFolderId, current.relativePath);
    this.saving = true;
    this.error = null;
    try {
      const saved = await chatApi.saveProjectWorkingFolderFile({
        workingFolderId,
        relativePath: current.relativePath,
        contents: this.draftText,
        expectedRevision: current.contentRevision,
        executionEnvironmentId: mutation.executionEnvironmentId,
      });
      if (!this.mutationIsCurrent(mutation)) return;
      this.applySavedPreview(saved);
      await this.options.refreshParentDirectory(saved.relativePath);
    } catch (reason: unknown) {
      if (!this.mutationIsCurrent(mutation)) return;
      this.error = chatErrorMessage(reason);
      this.saveConflict = errorCode(reason) === "conflict";
      if (this.saveConflict && !this.saveCopyPath) {
        this.saveCopyPath = fileConflictCopyPath(current.relativePath);
      }
    } finally {
      if (this.mutationScopeIsCurrent(mutation)) this.saving = false;
    }
  }

  async compareConflict(): Promise<void> {
    const workingFolderId = this.chat.selectedWorkingFolderId;
    const current = this.preview;
    if (!workingFolderId || !current) return;
    const environmentId = this.chat.selectedExecutionEnvironmentId;
    const scopeRevision = this.scopeRevision;
    try {
      const disk = await chatApi.previewProjectWorkingFolderFile(
        workingFolderId,
        current.relativePath,
        environmentId,
      );
      if (!this.scopeIsCurrent(workingFolderId, environmentId, scopeRevision)
        || this.preview?.relativePath !== current.relativePath) return;
      this.conflictDiskText = disk.text ?? "";
    } catch (reason: unknown) {
      if (this.scopeIsCurrent(workingFolderId, environmentId, scopeRevision)
        && this.preview?.relativePath === current.relativePath) {
        this.error = chatErrorMessage(reason);
      }
    }
  }

  async saveConflictCopy(): Promise<void> {
    const workingFolderId = this.chat.selectedWorkingFolderId;
    const current = this.preview;
    const target = this.saveCopyPath.trim();
    if (!workingFolderId || !current || !target || this.saving) return;
    const mutation = this.beginMutation(workingFolderId, current.relativePath);
    this.saving = true;
    this.error = null;
    try {
      const saved = this.fileDeleted
        ? await chatApi.recreateProjectWorkingFolderFile({
          workingFolderId,
          relativePath: target,
          contents: this.draftText,
          confirmed: true,
          executionEnvironmentId: mutation.executionEnvironmentId,
        })
        : await chatApi.saveProjectWorkingFolderFileCopy({
          workingFolderId,
          sourceRelativePath: current.relativePath,
          targetRelativePath: target,
          contents: this.draftText,
          executionEnvironmentId: mutation.executionEnvironmentId,
        });
      if (!this.mutationIsCurrent(mutation)) return;
      this.applySavedPreview(saved);
      this.options.setSelectedPath(saved.relativePath);
      await this.options.refreshParentDirectory(saved.relativePath);
    } catch (reason: unknown) {
      if (this.mutationIsCurrent(mutation)) this.error = chatErrorMessage(reason);
    } finally {
      if (this.mutationScopeIsCurrent(mutation)) this.saving = false;
    }
  }

  reloadSelectedFile(): void {
    const selectedPath = this.options.selectedPath();
    if (!selectedPath) return;
    if (this.dirty && !window.confirm(this.options.discardUnsavedMessage())) return;
    void this.select(selectedPath, this.chat.selectedExecutionEnvironmentId, true);
  }

  async overwriteExternalFile(): Promise<void> {
    const workingFolderId = this.chat.selectedWorkingFolderId;
    const current = this.preview;
    if (!workingFolderId || !current || this.fileDeleted || this.saving
      || !window.confirm(this.options.confirmOverwriteMessage())) return;
    const mutation = this.beginMutation(workingFolderId, current.relativePath);
    this.saving = true;
    this.error = null;
    try {
      const disk = await chatApi.previewProjectWorkingFolderFile(
        workingFolderId,
        current.relativePath,
        mutation.executionEnvironmentId,
      );
      if (!this.mutationIsCurrent(mutation)) return;
      if (!disk.contentRevision) throw new Error(this.options.previewUnavailableMessage());
      const saved = await chatApi.saveProjectWorkingFolderFile({
        workingFolderId,
        relativePath: current.relativePath,
        contents: this.draftText,
        expectedRevision: disk.contentRevision,
        executionEnvironmentId: mutation.executionEnvironmentId,
      });
      if (!this.mutationIsCurrent(mutation)) return;
      this.applySavedPreview(saved);
      await this.options.refreshParentDirectory(saved.relativePath);
    } catch (reason: unknown) {
      if (!this.mutationIsCurrent(mutation)) return;
      this.error = chatErrorMessage(reason);
      this.saveConflict = errorCode(reason) === "conflict";
    } finally {
      if (this.mutationScopeIsCurrent(mutation)) this.saving = false;
    }
  }

  async recreateDeletedFile(): Promise<void> {
    const workingFolderId = this.chat.selectedWorkingFolderId;
    const current = this.preview;
    if (!workingFolderId || !current || !this.fileDeleted || this.saving
      || !window.confirm(this.options.confirmRecreateMessage())) return;
    const mutation = this.beginMutation(workingFolderId, current.relativePath);
    this.saving = true;
    this.error = null;
    try {
      const saved = await chatApi.recreateProjectWorkingFolderFile({
        workingFolderId,
        relativePath: current.relativePath,
        contents: this.draftText,
        confirmed: true,
        executionEnvironmentId: mutation.executionEnvironmentId,
      });
      if (!this.mutationIsCurrent(mutation)) return;
      this.applySavedPreview(saved);
      await this.options.refreshParentDirectory(saved.relativePath);
    } catch (reason: unknown) {
      if (!this.mutationIsCurrent(mutation)) return;
      this.error = chatErrorMessage(reason);
      if (errorCode(reason) === "conflict") this.saveConflict = true;
    } finally {
      if (this.mutationScopeIsCurrent(mutation)) this.saving = false;
    }
  }

  async attachSelection(): Promise<void> {
    const workingFolderId = this.chat.selectedWorkingFolderId;
    const path = this.options.selectedPath();
    if (!workingFolderId || !path) return;
    const environmentId = this.chat.selectedExecutionEnvironmentId;
    const scopeRevision = this.scopeRevision;
    const bounded = boundTerminalContext(this.editorSelection?.text ?? "", 128 * 1024);
    if (!bounded.text) return;
    try {
      const attachment = await chatApi.importChatTextSnippet(
        workingFolderId,
        crypto.randomUUID(),
        `${path} selection.txt`,
        bounded.text,
      );
      if (!this.scopeIsCurrent(workingFolderId, environmentId, scopeRevision)
        || this.options.selectedPath() !== path) return;
      this.chat.setComposerAttachments([...this.chat.composer.attachmentIds, attachment.id]);
    } catch (reason: unknown) {
      if (this.scopeIsCurrent(workingFolderId, environmentId, scopeRevision)) {
        this.error = chatErrorMessage(reason);
      }
    }
  }

  async createReviewComment(): Promise<void> {
    const threadId = this.chat.selectedThreadId;
    const current = this.preview;
    const selection = this.editorSelection;
    if (!threadId || !current?.contentRevision || !selection?.text
      || !this.reviewDraft.trim() || this.creatingReview) return;
    const workingFolderId = this.chat.selectedWorkingFolderId;
    if (!workingFolderId) return;
    const environmentId = this.chat.selectedExecutionEnvironmentId;
    const scopeRevision = this.scopeRevision;
    this.creatingReview = true;
    this.error = null;
    try {
      await chatApi.createChatReviewComment({
        id: crypto.randomUUID(),
        threadId,
        relativePath: current.relativePath,
        contentRevision: current.contentRevision,
        startLine: selection.startLine,
        startColumn: selection.startColumn,
        endLine: selection.endLine,
        endColumn: selection.endColumn,
        selectedText: selection.text,
        commentText: this.reviewDraft.trim(),
      });
      if (!this.scopeIsCurrent(workingFolderId, environmentId, scopeRevision)
        || this.chat.selectedThreadId !== threadId
        || this.preview?.relativePath !== current.relativePath) return;
      this.closeReviewComposer();
      this.options.onReviewCreated();
    } catch (reason: unknown) {
      if (this.scopeIsCurrent(workingFolderId, environmentId, scopeRevision)
        && this.chat.selectedThreadId === threadId
        && this.preview?.relativePath === current.relativePath) {
        this.error = chatErrorMessage(reason);
      }
    } finally {
      if (this.scopeIsCurrent(workingFolderId, environmentId, scopeRevision)
        && this.chat.selectedThreadId === threadId) this.creatingReview = false;
    }
  }

  private beginLoading(): void {
    this.loading = true;
    this.loadingVisible = false;
    if (this.loadingTimer !== null) window.clearTimeout(this.loadingTimer);
    this.loadingTimer = window.setTimeout(() => {
      this.loadingTimer = null;
      if (this.loading) this.loadingVisible = true;
    }, 140);
  }

  private finishLoading(): void {
    this.loading = false;
    this.loadingVisible = false;
    if (this.loadingTimer !== null) window.clearTimeout(this.loadingTimer);
    this.loadingTimer = null;
  }

  private applyPreview(preview: ProjectWorkingFolderFilePreview): void {
    this.preview = preview;
    this.draftText = preview.text ?? "";
    this.editorSelection = null;
    this.reviewComposerOpen = false;
    this.reviewDraft = "";
    this.clearConflictState();
  }

  private applySavedPreview(preview: ProjectWorkingFolderFilePreview): void {
    this.preview = preview;
    this.draftText = preview.text ?? "";
    this.clearConflictState();
  }

  private clearConflictState(): void {
    this.saveConflict = false;
    this.fileDeleted = false;
    this.conflictDiskText = null;
    this.saveCopyPath = "";
  }

  private beginMutation(workingFolderId: string, relativePath: string): FileMutationContext {
    return {
      requestId: ++this.mutationRequestId,
      scopeRevision: this.scopeRevision,
      workingFolderId,
      executionEnvironmentId: this.chat.selectedExecutionEnvironmentId,
      relativePath,
    };
  }

  private mutationScopeIsCurrent(context: FileMutationContext): boolean {
    return context.requestId === this.mutationRequestId
      && this.scopeIsCurrent(
        context.workingFolderId,
        context.executionEnvironmentId,
        context.scopeRevision,
      );
  }

  private mutationIsCurrent(context: FileMutationContext): boolean {
    return this.mutationScopeIsCurrent(context)
      && this.preview?.relativePath === context.relativePath;
  }

  private scopeIsCurrent(
    workingFolderId: string,
    executionEnvironmentId: string | null,
    scopeRevision: number,
  ): boolean {
    return scopeRevision === this.scopeRevision
      && workingFolderId === this.chat.selectedWorkingFolderId
      && executionEnvironmentId === this.chat.selectedExecutionEnvironmentId;
  }
}

export function fileConflictCopyPath(path: string): string {
  const slash = path.lastIndexOf("/");
  const directory = slash >= 0 ? path.slice(0, slash + 1) : "";
  const name = slash >= 0 ? path.slice(slash + 1) : path;
  const dot = name.lastIndexOf(".");
  return dot > 0
    ? `${directory}${name.slice(0, dot)}.ganbaru-copy${name.slice(dot)}`
    : `${directory}${name}.ganbaru-copy`;
}

function errorCode(reason: unknown): string | null {
  if (typeof reason !== "object" || reason === null || !("code" in reason)) return null;
  return typeof reason.code === "string" ? reason.code : null;
}
