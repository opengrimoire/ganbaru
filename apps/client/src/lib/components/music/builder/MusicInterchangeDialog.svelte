<script lang="ts">
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { MusicInterchangeController } from "$lib/music/library/interchange-controller.svelte";
  import type { MusicTransferCommit } from "$lib/music/library/interchange";
  import type { MusicPlaylistSummary } from "$lib/music/library/contracts";
  import { systemMusicPlaylistName } from "$lib/music/playlists/system";
  import Checkbox from "$lib/components/ui/Checkbox.svelte";
  import Select from "$lib/components/ui/Select.svelte";
  import MusicBuilderDialog from "./MusicBuilderDialog.svelte";

  type PlaylistConflict = MusicTransferCommit["playlistConflict"];

  let { controller, playlists, onClose, onImported }: { controller: MusicInterchangeController; playlists: MusicPlaylistSummary[]; onClose: () => void; onImported: () => void } = $props();
  const { t } = getLocalization();
  const preview = $derived(controller.jsonPreview);
  const m3u = $derived(controller.m3u8Preview);
  const missingRoots = $derived(preview?.roots.filter((root) => !controller.isRootMapped(root.id)) ?? []);
  const conflictOptions = $derived<Array<{ value: PlaylistConflict; label: string }>>([
    { value: "import-copy", label: t("music.builder.importAsCopy") },
    { value: "keep-existing", label: t("music.builder.keepExisting") },
    { value: "replace-existing", label: t("music.builder.replaceExisting") },
  ]);
  const rootOptions = $derived([
    { value: "", label: t("music.builder.noRootSelected") },
    ...controller.availableRoots.map((root) => ({ value: root.id, label: root.name })),
  ]);

  /** Applies a conflict choice only when it is one of the offered values. */
  function chooseConflict(value: string): void {
    const option = conflictOptions.find((candidate) => candidate.value === value);
    if (option) controller.playlistConflict = option.value;
  }

  /** Sets the relative path root and refreshes the M3U8 preview. */
  function chooseRoot(value: string): void {
    controller.m3u8RootId = value;
    void controller.refreshPreview();
  }

  async function commit(): Promise<void> {
    if (await controller.commitImport()) onImported();
  }
</script>

<MusicBuilderDialog
  title={controller.mode === "export" ? t("music.builder.exportPlaylists") : t("music.builder.importPlaylists")}
  titleId="music-interchange-title"
  description={controller.mode === "export" ? t("music.builder.exportPlaylistsHint") : t("music.builder.importPlaylistsHint")}
  size="large"
  dismissDisabled={controller.busy}
  onDismiss={onClose}
>
    <div>
      {#if controller.importResult}
        <div class="py-4"><h3 class="text-sm font-semibold text-success">{t("music.builder.importComplete")}</h3><p class="mt-1 text-xs text-muted-foreground">{t("music.builder.importCompleteCounts", controller.importResult.playlistCount, controller.importResult.itemCount, controller.importResult.membershipCount)}</p></div>
      {:else if controller.mode === "export"}
        <fieldset class="divide-y divide-border border-y border-border"><legend class="sr-only">{t("music.builder.exportPlaylists")}</legend><label class="format-choice"><input type="radio" name="music-export-format" checked={controller.format === "json"} onchange={() => controller.format = "json"} class="mt-1 accent-primary" /><span><strong>{t("music.builder.ganbaruJson")}</strong><small>{t("music.builder.ganbaruJsonHint")}</small></span></label><label class="format-choice"><input type="radio" name="music-export-format" checked={controller.format === "m3u8"} onchange={() => controller.format = "m3u8"} class="mt-1 accent-primary" /><span><strong>M3U8</strong><small>{t("music.builder.m3u8Hint")}</small></span></label></fieldset>
        {#if controller.format === "m3u8"}<p class="mt-3 rounded-lg border border-warning/20 bg-warning/8 p-2.5 text-[0.66rem] leading-relaxed text-muted-foreground">{t("music.builder.m3u8LossWarning")}</p>{/if}
        <div class="mt-4 flex items-center justify-between"><h3 class="text-xs font-semibold">{t("music.builder.choosePlaylists")}</h3><button type="button" onclick={() => controller.selectedPlaylistIds = new Set(playlists.map((playlist) => playlist.id))} class="text-[0.65rem] text-primary hover:underline">{t("music.builder.selectAll")}</button></div>
        <div class="mt-2 divide-y divide-border border-y border-border">{#each playlists as playlist}<label class="flex min-w-0 items-center gap-2 py-2.5 text-xs"><Checkbox checked={controller.selectedPlaylistIds.has(playlist.id)} onChange={() => controller.togglePlaylist(playlist.id)} /><span class="min-w-0 flex-1 truncate">{systemMusicPlaylistName(playlist.id, playlist.name, t)}</span><span class="text-muted-foreground">{playlist.totalCount}</span></label>{/each}</div>
      {:else if !preview && !m3u}
        <div class="py-8 text-center"><h3 class="text-sm font-semibold">{t("music.builder.chooseImportFile")}</h3><p class="mx-auto mt-1 max-w-sm text-xs leading-relaxed text-muted-foreground">{t("music.builder.chooseImportFileHint")}</p><button type="button" onclick={() => { void controller.readImport(); }} disabled={controller.busy} class="mt-4 min-h-10 rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground disabled:opacity-50">{t("music.builder.browseFiles")}</button></div>
      {:else if preview}
        <div class="preview-summary"><div class="preview-stat"><strong>{preview.newPlaylists}</strong><span>{t("music.builder.newPlaylists")}</span></div><div class="preview-stat"><strong>{preview.matchedItems}</strong><span>{t("music.builder.matchedItems")}</span></div><div class="preview-stat"><strong>{preview.newItems}</strong><span>{t("music.builder.newItems")}</span></div><div class="preview-stat"><strong>{preview.duplicateItems}</strong><span>{t("music.builder.duplicateItems")}</span></div><div class="preview-stat"><strong>{preview.missingLocalBindings}</strong><span>{t("music.builder.unmappedRoots")}</span></div><div class="preview-stat"><strong>{preview.unsupportedCount}</strong><span>{t("music.builder.unsupportedEntries")}</span></div></div>
        {#if preview.unsupportedCount > 0}<div class="mt-3 max-h-24 overflow-y-auto rounded-lg bg-warning/8 p-2 text-[0.63rem] leading-relaxed text-muted-foreground">{#each preview.unsupported as issue}<p>{issue}</p>{/each}</div>{/if}
        {#if preview.conflicts.length > 0}<div class="mt-4"><label class="text-[0.68rem] font-semibold" for="music-import-conflicts">{t("music.builder.playlistConflictDecision")}</label><div class="mt-1.5"><Select inline value={controller.playlistConflict} options={conflictOptions} onChange={chooseConflict} disabled={controller.busy || controller.retryPending} ariaLabel={t("music.builder.playlistConflictDecision")} triggerProps={{ id: "music-import-conflicts" }} class="w-full" /></div><p class="mt-1 text-[0.63rem] text-muted-foreground">{preview.conflicts.join(", ")}</p></div>{/if}
        {#if missingRoots.length > 0}<div class="mt-4"><h3 class="text-xs font-semibold">{t("music.builder.mapImportedRoots")}</h3><p class="mt-1 text-[0.65rem] leading-relaxed text-muted-foreground">{t("music.builder.mapImportedRootsHint")}</p><div class="mt-2 divide-y divide-border border-y border-border">{#each missingRoots as root}<div class="flex items-center gap-2 py-2"><span class="min-w-0 flex-1 truncate text-xs">{root.name}</span><button type="button" onclick={() => { void controller.mapImportedRoot(root.id); }} disabled={controller.busy || controller.retryPending} class="min-h-8 rounded-md border border-border px-2 text-xs font-medium hover:bg-accent">{controller.mappedRootIds.has(root.id) ? t("music.builder.rootMapped") : t("music.builder.chooseFolder")}</button></div>{/each}</div></div>{/if}
        <div class="mt-4 space-y-2"><label class="flex items-start gap-2 text-[0.68rem]"><Checkbox bind:checked={controller.replaceItemDescriptions} disabled={controller.busy || controller.retryPending} class="mt-0.5" /><span>{t("music.builder.replaceImportedDescriptions")}</span></label>{#if preview.contextAssignmentCount > 0}<label class="flex items-start gap-2 text-[0.68rem]"><Checkbox bind:checked={controller.importContextAssignments} disabled={controller.busy || controller.retryPending} class="mt-0.5" /><span>{t("music.builder.importAssignments", preview.contextAssignmentCount)}</span></label>{/if}</div>
      {:else if m3u}
        <p class="rounded-lg border border-warning/20 bg-warning/8 p-2.5 text-[0.66rem] leading-relaxed text-muted-foreground">{t("music.builder.m3u8LossWarning")}</p>
        <div class="preview-summary mt-3"><div class="preview-stat"><strong>{m3u.localCount}</strong><span>{t("music.builder.local")}</span></div><div class="preview-stat"><strong>{m3u.youtubeCount}</strong><span>{t("music.builder.youtube")}</span></div><div class="preview-stat"><strong>{controller.unresolvedM3uCount()}</strong><span>{t("music.builder.unresolvedPaths")}</span></div><div class="preview-stat"><strong>{m3u.unsupportedCount}</strong><span>{t("music.builder.unsupportedEntries")}</span></div></div>
        <label class="mt-4 block text-[0.68rem] font-semibold" for="music-m3u-name">{t("music.builder.playlistName")}</label><input id="music-m3u-name" bind:value={controller.importPlaylistName} disabled={controller.busy || controller.retryPending} onchange={() => { void controller.refreshPreview(); }} class="field mt-1.5 h-9 w-full text-xs" />
        <label class="mt-3 block text-[0.68rem] font-semibold" for="music-m3u-root">{t("music.builder.relativePathRoot")}</label><div class="mt-1.5"><Select inline value={controller.m3u8RootId} options={rootOptions} onChange={chooseRoot} disabled={controller.busy || controller.retryPending} ariaLabel={t("music.builder.relativePathRoot")} triggerProps={{ id: "music-m3u-root" }} class="w-full" /></div><p class="mt-1 text-[0.63rem] text-muted-foreground">{t("music.builder.relativePathRootHint")}</p>
      {/if}
      {#if controller.error}<Notice class="mt-3 text-sm" message={controller.error} />{/if}
    </div>
    {#snippet footer()}
      <button type="button" onclick={onClose} disabled={controller.busy} class="min-h-10 rounded-md border border-border bg-card px-3.5 py-2 text-sm font-medium hover:bg-accent disabled:opacity-50">{controller.importResult ? t("music.builder.close") : t("music.builder.cancel")}</button>
      {#if controller.mode === "export"}<button type="button" onclick={() => { void controller.exportSelected(); }} disabled={controller.busy || controller.selectedPlaylistIds.size === 0} class="min-h-10 rounded-md border border-border bg-primary px-3.5 py-2 text-sm font-medium text-primary-foreground hover:bg-primary/90 disabled:opacity-50">{controller.busy ? t("music.builder.exporting") : t("music.builder.exportPlaylists")}</button>{:else if (preview || m3u) && !controller.importResult}<button type="button" onclick={() => { void commit(); }} disabled={controller.busy || (Boolean(m3u) && !controller.importPlaylistName.trim())} class="min-h-10 rounded-md border border-border bg-primary px-3.5 py-2 text-sm font-medium text-primary-foreground hover:bg-primary/90 disabled:opacity-50">{controller.busy ? t("music.builder.importing") : t("music.builder.importPlaylists")}</button>{/if}
    {/snippet}
</MusicBuilderDialog>

<style>
  .format-choice { display: flex; align-items: flex-start; gap: 0.65rem; padding: 0.75rem 0; }
  .format-choice span { min-width: 0; } .format-choice strong { display: block; font-size: calc(0.8rem * var(--type-scale)); } .format-choice small { margin-top: 0.2rem; display: block; color: var(--muted-foreground); font-size: calc(0.7rem * var(--type-scale)); line-height: 1.35; }
  .preview-summary { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); border-block: 1px solid var(--border); padding-block: 0.5rem; }
  .preview-stat { padding: 0.35rem 0.5rem; } .preview-stat strong { display: block; font-size: calc(1rem * var(--type-scale)); } .preview-stat span { display: block; color: var(--muted-foreground); font-size: calc(0.7rem * var(--type-scale)); }
  @media (max-width: 420px) { .preview-summary { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
</style>
