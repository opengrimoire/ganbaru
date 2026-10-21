<script lang="ts">
  import { onMount, tick } from "svelte";
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import Archive from "@lucide/svelte/icons/archive";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import File from "@lucide/svelte/icons/file";
  import Folder from "@lucide/svelte/icons/folder";
  import HardDrive from "@lucide/svelte/icons/hard-drive";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Search from "@lucide/svelte/icons/search";
  import Trash2 from "@lucide/svelte/icons/trash";
  import Upload from "@lucide/svelte/icons/upload";
  import X from "@lucide/svelte/icons/x";
  import * as chatApi from "$lib/api/chat";
  import type {
    ChatFolderGrant,
    ChatScratchCleanupPreviewRead,
    ChatScratchDirectoryEntryRead,
    ChatScratchDirectoryPageRead,
    ChatScratchGenerationRead,
    ChatScratchScopeRead,
    ChatTeammateAccessRead,
    ProjectWorkingFolderId,
  } from "$lib/chat/contracts";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import Select from "$lib/components/ui/Select.svelte";
  import { formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";

  let {
    initialReplyThreadId,
    onClose,
  }: {
    initialReplyThreadId?: string;
    onClose: () => void;
  } = $props();

  type PromotionKind = "workingFolder" | "managedAttachment";

  const localization = getLocalization();
  const { t } = localization;
  let dialog = $state<HTMLDivElement>();
  let scopes = $state<ChatScratchScopeRead[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let notice = $state<string | null>(null);
  let search = $state("");
  let selectedScopeId = $state<string | null>(null);
  let selectedGenerationId = $state<string | null>(null);
  let directory = $state<ChatScratchDirectoryPageRead | null>(null);
  let directoryLoading = $state(false);
  let directoryError = $state<string | null>(null);
  let access = $state<ChatTeammateAccessRead | null>(null);
  let accessRequest = 0;
  let promotionEntry = $state<ChatScratchDirectoryEntryRead | null>(null);
  let promotionKind = $state<PromotionKind>("workingFolder");
  let promotionFolderId = $state<ProjectWorkingFolderId | null>(null);
  let promotionTargetPath = $state("");
  let promotionDisplayName = $state("");
  let promoting = $state(false);
  let cleanupPreview = $state<ChatScratchCleanupPreviewRead | null>(null);
  let cleanupLoading = $state(false);

  const filteredScopes = $derived.by(() => {
    const query = search.trim().toLocaleLowerCase();
    if (!query) return scopes;
    return scopes.filter((scope) => (
      `${scope.teammateName} ${scope.groupName} ${scope.projectName} ${scope.channelName}`
        .toLocaleLowerCase()
        .includes(query)
    ));
  });
  const selectedScope = $derived(
    scopes.find((scope) => scope.id === selectedScopeId) ?? null,
  );
  const selectedGeneration = $derived(
    selectedScope?.generations.find((generation) => generation.id === selectedGenerationId) ?? null,
  );
  const editableFolders = $derived.by(() => {
    const channel = access?.channels.find((entry) => entry.channelId === selectedScope?.channelId);
    return channel?.folderGrants.filter((grant) => (
      grant.revokedAt === null && folderCanEdit(grant)
    )) ?? [];
  });
  const canPromote = $derived(Boolean(
    promotionEntry?.contentRevision
      && (promotionKind === "workingFolder"
        ? promotionFolderId && promotionTargetPath.trim()
        : promotionDisplayName.trim())
      && !promoting,
  ));

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key === "Tab" && !cleanupPreview && dialog) {
      const focusable = [...dialog.querySelectorAll<HTMLElement>(
        "button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex='-1'])",
      )].filter((element) => element.offsetParent !== null);
      const first = focusable[0];
      const last = focusable.at(-1);
      if (first && last && event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (first && last && !event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
      return;
    }
    if (event.key !== "Escape" || cleanupPreview) return;
    event.preventDefault();
    event.stopPropagation();
    onClose();
  }

  onMount(() => {
    const returnFocus = document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null;
    window.addEventListener("keydown", handleKeydown, true);
    void loadScopes().then(() => tick()).then(() => dialog?.focus());
    return () => {
      window.removeEventListener("keydown", handleKeydown, true);
      queueMicrotask(() => returnFocus?.focus());
    };
  });

  async function loadScopes(preferredGenerationId?: string): Promise<void> {
    loading = true;
    error = null;
    try {
      scopes = await chatApi.listChatScratchScopes(null, initialReplyThreadId ?? null);
      const selected = scopes.find((scope) => scope.id === selectedScopeId)
        ?? scopes.find((scope) => scope.replyThreadId === initialReplyThreadId)
        ?? scopes[0]
        ?? null;
      selectedScopeId = selected?.id ?? null;
      const generation = selected?.generations.find((entry) => entry.id === preferredGenerationId)
        ?? selected?.generations.find((entry) => entry.id === selectedGenerationId)
        ?? selected?.generations[0]
        ?? null;
      selectedGenerationId = generation?.id ?? null;
      directory = null;
      if (selected) void loadAccess(selected);
    } catch (cause: unknown) {
      error = chatErrorMessage(cause, t("settings.chat.teammates.scratchManager.loadFailed"));
    } finally {
      loading = false;
    }
  }

  /** Retries loading scratch scopes and rejects when the load fails again. */
  async function retryScopes(): Promise<void> {
    await loadScopes();
    if (error) throw new Error(error);
  }

  async function selectScope(scope: ChatScratchScopeRead): Promise<void> {
    selectedScopeId = scope.id;
    selectedGenerationId = scope.generations[0]?.id ?? null;
    directory = null;
    directoryError = null;
    promotionEntry = null;
    notice = null;
    await loadAccess(scope);
  }

  async function loadAccess(scope: ChatScratchScopeRead): Promise<void> {
    const request = ++accessRequest;
    access = null;
    try {
      const next = await chatApi.readChatTeammateAccess(scope.teammateId);
      if (request !== accessRequest || selectedScopeId !== scope.id) return;
      access = next;
      const folderIds = new Set(editableFolders.map((grant) => grant.workingFolderId));
      if (!promotionFolderId || !folderIds.has(promotionFolderId)) {
        promotionFolderId = editableFolders[0]?.workingFolderId ?? null;
      }
    } catch {
      if (request === accessRequest) access = null;
    }
  }

  async function openGeneration(generation: ChatScratchGenerationRead): Promise<void> {
    selectedGenerationId = generation.id;
    promotionEntry = null;
    notice = null;
    await loadDirectory("");
  }

  async function loadDirectory(relativePath: string, cursor: string | null = null): Promise<void> {
    if (!selectedGeneration) return;
    directoryLoading = true;
    directoryError = null;
    try {
      const page = await chatApi.browseChatScratchGeneration({
        scratchGenerationId: selectedGeneration.id,
        relativePath,
        cursor,
        limit: 20,
        allowRestrictedInspection: selectedGeneration.lifecycleState !== "active",
      });
      directory = cursor && directory?.relativePath === relativePath
        ? { ...page, entries: [...directory.entries, ...page.entries] }
        : page;
    } catch (cause: unknown) {
      directoryError = chatErrorMessage(
        cause,
        t("settings.chat.teammates.scratchManager.browseFailed"),
      );
    } finally {
      directoryLoading = false;
    }
  }

  function upPath(path: string): string {
    const parts = path.split("/").filter(Boolean);
    parts.pop();
    return parts.join("/");
  }

  function beginPromotion(entry: ChatScratchDirectoryEntryRead): void {
    if (!entry.contentRevision || !selectedScope) return;
    promotionEntry = entry;
    promotionKind = editableFolders.length > 0 ? "workingFolder" : "managedAttachment";
    promotionTargetPath = entry.relativePath;
    promotionDisplayName = entry.displayName;
    const folderIds = new Set(editableFolders.map((grant) => grant.workingFolderId));
    if (!promotionFolderId || !folderIds.has(promotionFolderId)) {
      promotionFolderId = editableFolders[0]?.workingFolderId ?? null;
    }
  }

  async function promote(): Promise<void> {
    if (!canPromote || !promotionEntry?.contentRevision || !selectedGeneration
      || !selectedScope) return;
    const destination = promotionKind === "workingFolder"
      ? promotionFolderId ? {
          kind: "workingFolder" as const,
          channelId: selectedScope.channelId,
          workingFolderId: promotionFolderId,
          relativePath: promotionTargetPath.trim(),
        } : null
      : {
          kind: "managedAttachment" as const,
          channelId: selectedScope.channelId,
          attachmentId: `scratch-attachment:${crypto.randomUUID()}`,
          displayName: promotionDisplayName.trim(),
        };
    if (!destination) return;
    promoting = true;
    directoryError = null;
    try {
      await chatApi.promoteChatScratchFile({
        promotionId: `scratch-promotion:${crypto.randomUUID()}`,
        scratchGenerationId: selectedGeneration.id,
        sourceRelativePath: promotionEntry.relativePath,
        expectedContentRevision: promotionEntry.contentRevision,
        destination,
      });
      notice = t("settings.chat.teammates.scratchManager.promotionSuccess");
      promotionEntry = null;
    } catch (cause: unknown) {
      directoryError = chatErrorMessage(
        cause,
        t("settings.chat.teammates.scratchManager.promotionFailed"),
      );
    } finally {
      promoting = false;
    }
  }

  async function requestCleanup(generation: ChatScratchGenerationRead): Promise<void> {
    cleanupLoading = true;
    error = null;
    try {
      const preview = await chatApi.previewChatScratchCleanup({
        scratchGenerationId: generation.id,
      });
      if (preview.activeRunCount > 0) {
        error = t(
          "settings.chat.teammates.scratchManager.cleanupBlocked",
          preview.activeRunCount,
        );
        return;
      }
      cleanupPreview = preview;
    } catch (cause: unknown) {
      error = chatErrorMessage(cause, t("settings.chat.teammates.scratchManager.cleanupFailed"));
    } finally {
      cleanupLoading = false;
    }
  }

  async function confirmCleanup(): Promise<void> {
    const preview = cleanupPreview;
    cleanupPreview = null;
    if (!preview) return;
    cleanupLoading = true;
    error = null;
    try {
      const result = await chatApi.cleanupChatScratch({
        scratchGenerationId: preview.scratchGenerationId,
        expectedScopeRevision: preview.expectedScopeRevision,
        confirmed: true,
      });
      notice = result.state === "completed"
        ? t("settings.chat.teammates.scratchManager.cleanupComplete")
        : t("settings.chat.teammates.scratchManager.cleanupDeferred");
      await loadScopes();
    } catch (cause: unknown) {
      error = chatErrorMessage(cause, t("settings.chat.teammates.scratchManager.cleanupFailed"));
    } finally {
      cleanupLoading = false;
    }
  }

  function folderCanEdit(grant: ChatFolderGrant): boolean {
    return grant.capability === "edit"
      || grant.capability === "execute"
      || grant.capability === "publish";
  }

  function selectPromotionKind(value: string): void {
    promotionKind = value === "managedAttachment" ? "managedAttachment" : "workingFolder";
  }

  function selectPromotionFolder(value: string): void {
    promotionFolderId = editableFolders.find((folder) => folder.workingFolderId === value)
      ?.workingFolderId ?? null;
  }

  function generationStatus(generation: ChatScratchGenerationRead): string {
    if (generation.lifecycleState === "quarantined") {
      return t("settings.chat.teammates.scratchManager.quarantined");
    }
    if (generation.lifecycleState === "cleanupPending") {
      return t("settings.chat.teammates.scratchManager.cleanupPending");
    }
    if (generation.lifecycleState === "cleanupFailed") {
      return t("settings.chat.teammates.scratchManager.cleanupFailedState");
    }
    return t("settings.chat.teammates.scratchManager.active");
  }

  function cleanupMessage(preview: ChatScratchCleanupPreviewRead): string {
    if (preview.activeRunCount > 0) {
      return t("settings.chat.teammates.scratchManager.cleanupBlocked", preview.activeRunCount);
    }
    if (preview.deviceAvailability === "unavailableOnThisDevice") {
      return t("settings.chat.teammates.scratchManager.cleanupUnavailableMessage");
    }
    if (preview.sizeTruncated) {
      return t(
        "settings.chat.teammates.scratchManager.cleanupLimitedMessage",
        formatNumber(localization.locale, preview.byteSize),
        preview.entryCount,
      );
    }
    return t(
      "settings.chat.teammates.scratchManager.cleanupMessage",
      formatNumber(localization.locale, preview.byteSize),
      preview.entryCount,
    );
  }
</script>

<div class="scratch-manager-backdrop surface-backdrop" role="presentation" onclick={(event) => { if (event.target === event.currentTarget) onClose(); }}>
  <div bind:this={dialog} class="scratch-manager surface-dialog" role="dialog" aria-modal="true" aria-labelledby="chat-scratch-manager-title" tabindex="-1">
    <header>
      <div><h3 id="chat-scratch-manager-title">{t("settings.chat.teammates.scratchManager.heading")}</h3><p>{t("settings.chat.teammates.scratchManager.description")}</p></div>
      <button type="button" aria-label={t("settings.chat.teammates.scratchManager.close")} onclick={onClose}><X size={17} /></button>
    </header>
    <div class="scratch-manager-body">
      <aside>
        <label class="scratch-search"><Search size={14} /><input bind:value={search} aria-label={t("settings.chat.teammates.scratchManager.search")} placeholder={t("settings.chat.teammates.scratchManager.search")} /></label>
        <nav aria-label={t("settings.chat.teammates.scratchManager.scopeList")} use:scrollEdgeFadeAction>
          {#if loading}<Loading layout="rows" rows={3} />
          {:else if scopes.length === 0}<p class="empty">{t("settings.chat.teammates.scratchManager.empty")}</p>
          {:else if filteredScopes.length === 0}<p class="empty">{t("settings.chat.teammates.scratchManager.noMatch")}</p>
          {:else}{#each filteredScopes as scope (scope.id)}
            <button type="button" class:active={scope.id === selectedScopeId} aria-current={scope.id === selectedScopeId ? "page" : undefined} onclick={() => void selectScope(scope)}>
              <HardDrive size={15} /><span><strong>{scope.teammateName}</strong><small>{scope.groupName} / {scope.projectName} / #{scope.channelName}</small></span><span class="count">{scope.generations.length}</span>
            </button>
          {/each}{/if}
        </nav>
      </aside>

      <main use:scrollEdgeFadeAction>
        {#if error}<Notice class="scratch-error" message={error} onRetry={scopes.length === 0 ? retryScopes : undefined} />{/if}
        {#if notice}<p class="notice" role="status">{notice}</p>{/if}
        {#if selectedScope}
          <section class="scope-heading">
            <div><h4>{selectedScope.teammateName}</h4><p>{selectedScope.groupName} / {selectedScope.projectName} / #{selectedScope.channelName}</p></div>
            {#if selectedScope.lifecycleState === "archived"}<span><Archive size={13} />{t("settings.chat.teammates.scratchManager.archived")}</span>{/if}
          </section>
          <section class="generation-grid">
            {#each selectedScope.generations as generation (generation.id)}
              <article class:selected={selectedGenerationId === generation.id} class:quarantined={generation.lifecycleState === "quarantined"}>
                <button type="button" class="generation-select" onclick={() => void openGeneration(generation)}>
                  <span class="generation-title"><strong>{t("settings.chat.teammates.scratchManager.generation", generation.generation)}</strong><small>{generationStatus(generation)}</small></span>
                  <span class="generation-meta"><small>{generation.deviceAvailability === "available" ? t("settings.chat.teammates.scratchManager.available") : t("settings.chat.teammates.scratchManager.unavailable")}</small><small>{t("settings.chat.teammates.scratchManager.bytes", formatNumber(localization.locale, generation.byteSize))} · {t("settings.chat.teammates.scratchManager.entries", generation.entryCount)}</small>{#if generation.sizeTruncated}<small>{t("settings.chat.teammates.scratchManager.sizeLimited")}</small>{/if}</span>
                </button>
                <div class="generation-actions">
                  <button type="button" disabled={generation.deviceAvailability !== "available" || cleanupLoading} onclick={() => void openGeneration(generation)}><Folder size={14} />{t("settings.chat.teammates.scratchManager.browse")}</button>
                  <button type="button" class="cleanup" disabled={cleanupLoading} onclick={() => void requestCleanup(generation)}><Trash2 size={14} />{t("settings.chat.teammates.scratchManager.cleanup")}</button>
                </div>
                <div class="source-summary">
                  <strong>{t("settings.chat.teammates.scratchManager.sources")}</strong>
                  {#if generation.retainedSources.length === 0}<small>{t("settings.chat.teammates.scratchManager.noSources")}</small>{:else}{#each generation.retainedSources as source (source.channelId)}<small>{t("settings.chat.teammates.scratchManager.sourceRange", source.channelName, source.lowerOrdinal, source.highOrdinal)}</small>{/each}{/if}
                </div>
              </article>
            {/each}
          </section>

          {#if selectedGeneration && directory}
            <section class="browser" aria-label={t("settings.chat.teammates.scratchManager.browseLabel")}>
              <header><button type="button" disabled={!directory.relativePath || directoryLoading} aria-label={t("settings.chat.teammates.scratchManager.up")} onclick={() => void loadDirectory(upPath(directory?.relativePath ?? ""))}><ArrowLeft size={14} /></button><span><HardDrive size={14} /><strong>{directory.relativePath || t("settings.chat.teammates.scratchManager.root")}</strong></span></header>
              {#if directoryError}<Notice class="scratch-error" message={directoryError} />{/if}
              <div class="entry-list">
                {#if directory.entries.length === 0}<p class="empty">{t("settings.chat.teammates.scratchManager.emptyFolder")}</p>{/if}
                {#each directory.entries as entry (entry.relativePath)}
                  <div class="entry-row">
                    <button type="button" class="entry-open" onclick={() => entry.kind === "directory" ? void loadDirectory(entry.relativePath) : beginPromotion(entry)} disabled={entry.kind === "file" && !entry.promotable} title={entry.kind === "file" && !entry.promotable ? t("settings.chat.teammates.scratchManager.fileNotPromotable") : undefined}>{#if entry.kind === "directory"}<Folder size={15} />{:else}<File size={15} />{/if}<span><strong>{entry.displayName}</strong>{#if entry.byteSize !== null}<small>{t("settings.chat.teammates.scratchManager.bytes", formatNumber(localization.locale, entry.byteSize))}</small>{/if}</span>{#if entry.kind === "directory"}<ChevronRight size={14} />{/if}</button>
                    {#if entry.kind === "file" && entry.promotable}<button type="button" class="promote-button" onclick={() => beginPromotion(entry)}><Upload size={14} />{t("settings.chat.teammates.scratchManager.promote")}</button>{/if}
                  </div>
                {/each}
              </div>
              {#if directory.nextCursor}<button type="button" class="load-more" disabled={directoryLoading} aria-busy={directoryLoading || undefined} onclick={() => void loadDirectory(directory?.relativePath ?? "", directory?.nextCursor ?? null)}>{#if directoryLoading}<LoaderCircle class="size-3.5 animate-spin" aria-hidden="true" />{/if}{t("settings.chat.teammates.scratchManager.loadMore")}</button>{/if}
            </section>
          {/if}

          {#if promotionEntry}
            <form class="promotion" onsubmit={(event) => { event.preventDefault(); void promote(); }}>
              <header><div><h4>{t("settings.chat.teammates.scratchManager.promoteHeading")}</h4><p>{promotionEntry.relativePath}</p></div><button type="button" aria-label={t("common.close")} onclick={() => { promotionEntry = null; }}><X size={15} /></button></header>
              <div class="promotion-fields">
                <div class="select-field"><span>{t("settings.chat.teammates.scratchManager.destination")}</span><Select value={promotionKind} options={[{ value: "workingFolder", label: t("settings.chat.teammates.scratchManager.projectFolder") }, { value: "managedAttachment", label: t("settings.chat.teammates.scratchManager.managedAttachment") }]} onChange={selectPromotionKind} class="w-full" /></div>
                {#if promotionKind === "workingFolder"}
                  {#if editableFolders.length === 0}<p class="alert full" role="alert">{t("settings.chat.teammates.scratchManager.chooseFolder")}</p>{:else}<div class="select-field"><span>{t("settings.chat.teammates.scratchManager.chooseFolder")}</span><Select value={promotionFolderId ?? ""} options={editableFolders.map((folder) => ({ value: folder.workingFolderId, label: folder.displayName }))} onChange={selectPromotionFolder} class="w-full" /></div><label class="full"><span>{t("settings.chat.teammates.scratchManager.targetPath")}</span><input bind:value={promotionTargetPath} maxlength="4096" /></label>{/if}
                {:else}
                  <label class="full"><span>{t("settings.chat.teammates.scratchManager.attachmentName")}</span><input bind:value={promotionDisplayName} maxlength="1000" /></label>
                {/if}
              </div>
              <footer><button type="button" onclick={() => { promotionEntry = null; }}>{t("common.cancel")}</button><button type="submit" class="primary" disabled={!canPromote} aria-busy={promoting || undefined}>{#if promoting}<LoaderCircle class="size-3.5 animate-spin" aria-hidden="true" />{/if}{t("settings.chat.teammates.scratchManager.promote")}</button></footer>
            </form>
          {/if}
        {:else if !loading}<p class="empty">{t("settings.chat.teammates.scratchManager.empty")}</p>{/if}
      </main>
    </div>
  </div>
</div>

{#if cleanupPreview}
  <ConfirmDialog
    title={t("settings.chat.teammates.scratchManager.cleanupTitle")}
    message={cleanupMessage(cleanupPreview)}
    confirmLabel={t("settings.chat.teammates.scratchManager.cleanupConfirm")}
    cancelLabel={t("common.cancel")}
    onConfirm={() => void confirmCleanup()}
    onCancel={() => { cleanupPreview = null; }}
  />
{/if}

<style>
  .scratch-manager-backdrop { position:fixed; z-index:110; inset:0; display:grid; place-items:center; padding:1rem; }
  .scratch-manager { display:grid; width:min(56rem,90vw); height:min(40rem,80dvh); grid-template-rows:auto minmax(0,1fr); overflow:hidden; outline:0; }
  .scratch-manager > header { display:flex; align-items:center; justify-content:space-between; gap:1rem; border-bottom:1px solid var(--border); padding:0.8rem 0.95rem; }
  .scratch-manager h3 { font-size:calc(0.92rem * var(--type-scale)); font-weight:650; }
  .scratch-manager > header p,.scope-heading p,.promotion header p { margin-top:0.15rem; color:var(--muted-foreground); font-size:calc(0.67rem * var(--type-scale)); }
  .scratch-manager > header button,.browser > header button,.promotion header button { display:grid; min-width:2.1rem; min-height:2.1rem; place-items:center; border-radius:0.42rem; }
  .scratch-manager button:hover:not(:disabled) { background:var(--accent); }
  .scratch-manager button:disabled { cursor:not-allowed; opacity:0.5; }
  .scratch-manager-body { display:grid; min-height:0; grid-template-columns:minmax(14rem,0.72fr) minmax(0,2fr); }
  .scratch-manager-body > aside { display:grid; min-height:0; grid-template-rows:auto minmax(0,1fr); gap:0.55rem; border-right:1px solid var(--border); padding:0.65rem; }
  .scratch-search { display:flex; min-height:2.25rem; align-items:center; gap:0.4rem; border:1px solid var(--border); border-radius:var(--floating-item-radius); padding-inline:0.55rem; color:var(--muted-foreground); }
  .scratch-search input { width:100%; min-width:0; border:0; background:transparent; color:var(--foreground); outline:0; font-size:calc(0.68rem * var(--type-scale)); }
  .scratch-manager nav { display:grid; align-content:start; gap:0.18rem; overflow-y:auto; }
  .scratch-manager nav button { display:grid; min-height:3rem; grid-template-columns:auto minmax(0,1fr) auto; align-items:center; gap:0.5rem; border-radius:0.5rem; padding:0.48rem; text-align:left; }
  .scratch-manager nav button.active { background:var(--accent); }
  .scratch-manager nav span:not(.count) { display:grid; min-width:0; }
  .scratch-manager nav strong,.scratch-manager nav small { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  .scratch-manager nav strong { font-size:calc(0.72rem * var(--type-scale)); }
  .scratch-manager nav small,.count { color:var(--muted-foreground); font-size:calc(0.61rem * var(--type-scale)); }
  .count { padding-inline:0.2rem; }
  .scratch-manager main { display:grid; min-height:0; align-content:start; gap:0.8rem; padding:0.9rem; overflow-y:auto; }
  .scope-heading { display:flex; align-items:center; justify-content:space-between; gap:1rem; border-bottom:1px solid var(--border); padding-bottom:0.7rem; }
  .scope-heading h4,.promotion h4 { font-size:calc(0.82rem * var(--type-scale)); font-weight:650; }
  .scope-heading > span { display:flex; align-items:center; gap:0.3rem; color:var(--muted-foreground); font-size:calc(0.62rem * var(--type-scale)); }
  .generation-grid { display:grid; }
  .generation-grid article { display:grid; gap:0.45rem; border-bottom:1px solid var(--border); padding:0.7rem 0.2rem; }
  .generation-grid article:first-child { border-top:1px solid var(--border); }
  .generation-grid article.selected { background:color-mix(in srgb,var(--accent) 45%,transparent); }
  .generation-grid article.quarantined { background:color-mix(in srgb,var(--destructive) 5%,var(--card)); }
  .generation-select { display:flex; min-height:2.8rem; align-items:flex-start; justify-content:space-between; gap:0.6rem; border-radius:0.42rem; text-align:left; }
  .generation-title,.generation-meta,.source-summary { display:grid; gap:0.12rem; }
  .generation-title strong { font-size:calc(0.72rem * var(--type-scale)); }
  .generation-title small,.generation-meta small,.source-summary small { color:var(--muted-foreground); font-size:calc(0.6rem * var(--type-scale)); }
  .generation-meta { text-align:right; }
  .generation-actions { display:flex; flex-wrap:wrap; gap:0.35rem; }
  .generation-actions button,.promote-button,.load-more,.promotion footer button { display:flex; min-height:2.15rem; align-items:center; justify-content:center; gap:0.32rem; border:1px solid var(--border); border-radius:0.42rem; padding:0.3rem 0.52rem; font-size:calc(0.64rem * var(--type-scale)); font-weight:600; }
  .generation-actions .cleanup { color:var(--destructive); }
  .source-summary { border-top:1px solid var(--border); padding-top:0.4rem; }
  .source-summary strong { font-size:calc(0.61rem * var(--type-scale)); }
  .browser,.promotion { display:grid; gap:0.55rem; border-block:1px solid var(--border); padding:0.65rem 0; }
  .browser > header,.promotion > header { display:flex; align-items:center; justify-content:space-between; gap:0.55rem; }
  .browser > header > span { display:flex; min-width:0; align-items:center; gap:0.4rem; }
  .browser > header strong { overflow:hidden; font-size:calc(0.7rem * var(--type-scale)); text-overflow:ellipsis; white-space:nowrap; }
  .entry-list { display:grid; gap:0.16rem; }
  .entry-row { display:grid; grid-template-columns:minmax(0,1fr) auto; align-items:center; gap:0.35rem; border-radius:0.45rem; }
  .entry-row:hover { background:color-mix(in srgb,var(--accent) 65%,transparent); }
  .entry-open { display:grid; min-height:2.7rem; min-width:0; grid-template-columns:auto minmax(0,1fr) auto; align-items:center; gap:0.45rem; padding:0.35rem 0.45rem; text-align:left; }
  .entry-open > span { display:grid; min-width:0; }
  .entry-open strong,.entry-open small { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  .entry-open strong { font-size:calc(0.68rem * var(--type-scale)); }
  .entry-open small { color:var(--muted-foreground); font-size:calc(0.59rem * var(--type-scale)); }
  .promote-button { margin-right:0.35rem; }
  .load-more { justify-self:center; }
  .promotion-fields { display:grid; grid-template-columns:repeat(2,minmax(0,1fr)); gap:0.55rem; }
  .promotion-fields label,.promotion-fields .select-field { display:grid; gap:0.28rem; color:var(--muted-foreground); font-size:calc(0.64rem * var(--type-scale)); }
  .promotion-fields .full { grid-column:1/-1; }
  .promotion-fields input { min-height:2.25rem; border:1px solid var(--border); border-radius:var(--floating-item-radius); background:transparent; outline:0; padding:0.4rem 0.5rem; color:var(--foreground); }
  .promotion footer { display:flex; justify-content:flex-end; gap:0.4rem; }
  .promotion footer .primary { background:var(--primary); color:var(--primary-foreground); }
  .empty,.alert,.notice { padding:0.65rem; border-radius:0.5rem; font-size:calc(0.67rem * var(--type-scale)); }
  .scratch-manager :global(.scratch-error) { padding:0.65rem; font-size:calc(0.67rem * var(--type-scale)); }
  .empty { color:var(--muted-foreground); }
  .alert { background:color-mix(in srgb,var(--destructive) 10%,transparent); color:var(--destructive); }
  .notice { background:color-mix(in srgb,var(--primary) 10%,transparent); color:var(--foreground); }
  @media (pointer:coarse) { .scratch-manager button,.scratch-manager input { min-height:2.75rem; } }
  @media (max-width:760px) { .scratch-manager-backdrop { align-items:end; padding:0; }.scratch-manager { width:100%; height:94dvh; border-radius:0.85rem 0.85rem 0 0; }.scratch-manager-body { grid-template-columns:1fr; grid-template-rows:minmax(7rem,27%) minmax(0,1fr); }.scratch-manager-body > aside { border-right:0; border-bottom:1px solid var(--border); }.scratch-manager nav { grid-template-columns:repeat(auto-fill,minmax(12rem,1fr)); }.promotion-fields { grid-template-columns:1fr; }.promotion-fields .full { grid-column:auto; } }
  @media (prefers-reduced-motion:reduce) { * { scroll-behavior:auto; transition:none; } }
</style>
