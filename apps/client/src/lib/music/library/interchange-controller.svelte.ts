import { pickAndReadMusicInterchangeFile, pickMusicRootBindingFolder } from "$lib/api/music";
import { commitMusicTransfer, exportMusicTransfer, MusicLibraryApiError, previewMusicTransfer, setLocalRootBinding } from "$lib/api/music-library";
import type { LocalRootBinding, MusicInterchangeImportResult, MusicPlaylistSummary } from "./contracts";
import type { MusicTransferCommit, MusicTransferFormat, MusicTransferPreview, MusicTransferSource } from "./interchange";

export type MusicInterchangeFormat = MusicTransferFormat;
export type MusicInterchangeMode = "import" | "export";

/** Owns dialog drafts and explicit review decisions over native transfer services. */
export class MusicInterchangeController {
  open = $state(false);
  mode = $state<MusicInterchangeMode>("export");
  format = $state<MusicInterchangeFormat>("json");
  selectedPlaylistIds = $state<Set<string>>(new Set());
  busy = $state(false);
  error = $state<string | null>(null);
  jsonPreview = $state<MusicTransferPreview | null>(null);
  m3u8Preview = $state<MusicTransferPreview | null>(null);
  importedContents = $state<string | null>(null);
  playlistConflict = $state<MusicTransferCommit["playlistConflict"]>("import-copy");
  replaceItemDescriptions = $state(false);
  importContextAssignments = $state(false);
  importPlaylistName = $state("Imported playlist");
  m3u8RootId = $state("");
  mappedRootIds = $state<Set<string>>(new Set());
  importResult = $state<MusicInterchangeImportResult | null>(null);
  availableRoots = $state<Array<{ id: string; name: string }>>([]);
  private selectedAtMs = 0;
  private sourceVaultId: string | null = null;
  private previewSource: MusicTransferSource | null = null;
  private pendingCommit = $state<MusicTransferCommit | null>(null);

  get retryPending(): boolean { return this.pendingCommit !== null; }

  constructor(
    private readonly playlists: () => readonly MusicPlaylistSummary[],
    private readonly bindings: () => readonly LocalRootBinding[],
    private readonly vaultId: () => string | null,
    private readonly now: () => number = Date.now,
  ) {}

  show(mode: MusicInterchangeMode, preferredPlaylistId: string | null = null): void {
    if (this.pendingCommit) { this.open = true; return; }
    this.mode = mode;
    this.format = "json";
    this.error = null;
    this.jsonPreview = null;
    this.m3u8Preview = null;
    this.importedContents = null;
    this.importResult = null;
    this.previewSource = null;
    this.pendingCommit = null;
    this.sourceVaultId = this.vaultId();
    this.mappedRootIds = new Set();
    this.selectedPlaylistIds = new Set(preferredPlaylistId ? [preferredPlaylistId] : this.playlists().map((playlist) => playlist.id));
    this.open = true;
  }

  close(): void {
    if (!this.busy) this.open = false;
  }

  togglePlaylist(playlistId: string): void {
    const next = new Set(this.selectedPlaylistIds);
    if (next.has(playlistId)) next.delete(playlistId); else next.add(playlistId);
    this.selectedPlaylistIds = next;
  }

  isRootMapped(rootId: string): boolean {
    const preview = this.jsonPreview ?? this.m3u8Preview;
    return preview ? preview.boundRootIds.includes(rootId) : this.bindings().some((binding) => binding.rootId === rootId && binding.status === "available");
  }

  unresolvedM3uCount(): number {
    return this.m3u8Preview?.unresolvedLocalCount ?? 0;
  }

  private currentSource(): MusicTransferSource {
    if (this.importedContents === null) throw new Error("Choose a Music import file");
    return { contents: this.importedContents, playlistName: this.importPlaylistName.trim(), relativeRootId: this.m3u8RootId || null, selectedAtMs: this.selectedAtMs };
  }

  private requireVault(): string {
    const vaultId = this.vaultId();
    if (!vaultId || vaultId !== this.sourceVaultId) throw new Error("The active vault changed. Reopen the Music transfer dialog");
    return vaultId;
  }

  private async loadPreview(): Promise<void> {
    const source = this.currentSource();
    const preview = await previewMusicTransfer(this.requireVault(), source);
    this.requireVault();
    this.format = preview.format;
    this.jsonPreview = preview.format === "json" ? preview : null;
    this.m3u8Preview = preview.format === "m3u8" ? preview : null;
    this.availableRoots = preview.availableRoots;
    this.mappedRootIds = new Set(preview.boundRootIds);
    this.previewSource = source;
    this.pendingCommit = null;
  }

  /** Refreshes semantic preview after a completed name or root selection change. */
  async refreshPreview(): Promise<void> {
    if (this.busy || this.pendingCommit || this.importedContents === null) return;
    this.busy = true;
    this.error = null;
    try { await this.loadPreview(); }
    catch (error) { this.error = error instanceof Error ? error.message : String(error); }
    finally { this.busy = false; }
  }

  async readImport(): Promise<boolean> {
    if (this.busy || this.pendingCommit) return false;
    this.busy = true;
    this.error = null;
    try {
      this.requireVault();
      const contents = await pickAndReadMusicInterchangeFile();
      if (contents === null) return false;
      this.importedContents = contents;
      this.selectedAtMs = this.now();
      await this.loadPreview();
      return true;
    } catch (error) {
      this.error = error instanceof Error ? error.message : String(error);
      return false;
    } finally { this.busy = false; }
  }

  async exportSelected(): Promise<boolean> {
    if (this.busy || this.selectedPlaylistIds.size === 0) return false;
    this.busy = true;
    this.error = null;
    try { return await exportMusicTransfer(this.requireVault(), [...this.selectedPlaylistIds], this.format); }
    catch (error) {
      this.error = error instanceof Error ? error.message : String(error);
      return false;
    } finally { this.busy = false; }
  }

  async mapImportedRoot(rootId: string): Promise<boolean> {
    if (this.busy || this.pendingCommit) return false;
    this.busy = true;
    this.error = null;
    try {
      const vaultId = this.requireVault();
      const folderPath = await pickMusicRootBindingFolder();
      if (!folderPath) return false;
      this.requireVault();
      await setLocalRootBinding(vaultId, rootId, folderPath);
      await this.loadPreview();
      return true;
    } catch (error) {
      this.error = error instanceof Error ? error.message : String(error);
      return false;
    } finally { this.busy = false; }
  }

  async commitImport(): Promise<boolean> {
    const preview = this.jsonPreview ?? this.m3u8Preview;
    if (this.busy || !preview || !this.previewSource) return false;
    this.busy = true;
    this.error = null;
    try {
      const source = this.currentSource();
      if (!this.pendingCommit && JSON.stringify(source) !== JSON.stringify(this.previewSource)) {
        await this.loadPreview();
        return false;
      }
      const intent = {
        source, expectedRevision: preview.revision, playlistConflict: this.playlistConflict,
        replaceItemDescriptions: this.replaceItemDescriptions, importContextAssignments: this.importContextAssignments,
      };
      if (!this.pendingCommit) {
        this.pendingCommit = { ...intent, actionId: crypto.randomUUID() };
      }
      this.importResult = await commitMusicTransfer(this.requireVault(), this.pendingCommit);
      this.pendingCommit = null;
      return true;
    } catch (error) {
      this.error = error instanceof Error ? error.message : String(error);
      if (error instanceof MusicLibraryApiError && error.code === "stale-write") {
        this.pendingCommit = null;
        try { await this.loadPreview(); }
        catch (refreshError) { this.error += "; " + (refreshError instanceof Error ? refreshError.message : String(refreshError)); }
      }
      return false;
    } finally { this.busy = false; }
  }
}

export function createMusicInterchangeController(
  playlists: () => readonly MusicPlaylistSummary[],
  bindings: () => readonly LocalRootBinding[],
  vaultId: () => string | null,
): MusicInterchangeController {
  return new MusicInterchangeController(playlists, bindings, vaultId);
}
