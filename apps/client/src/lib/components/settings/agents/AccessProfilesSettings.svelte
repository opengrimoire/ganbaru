<script lang="ts">
  import { onMount } from "svelte";
  import Archive from "@lucide/svelte/icons/archive";
  import Copy from "@lucide/svelte/icons/copy";
  import FilePenLine from "@lucide/svelte/icons/file-pen-line";
  import Folder from "@lucide/svelte/icons/folder";
  import GitPullRequest from "@lucide/svelte/icons/git-pull-request";
  import MessagesSquare from "@lucide/svelte/icons/messages-square";
  import PencilRuler from "@lucide/svelte/icons/pencil-ruler";
  import Plus from "@lucide/svelte/icons/plus";
  import Save from "@lucide/svelte/icons/save";
  import X from "@lucide/svelte/icons/x";
  import * as chatApi from "$lib/api/chat";
  import { CHAT_FOLDER_CAPABILITIES } from "$lib/chat/contracts";
  import type {
    ChatAccessProfileImpactPreviewRead,
    ChatAccessProfileRead,
    ChatFolderCapability,
  } from "$lib/chat/contracts";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import ChatIconTile from "$lib/components/chat/identity/ChatIconTile.svelte";
  import CalendarScrollbar from "$lib/components/calendar/CalendarScrollbar.svelte";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import Select from "$lib/components/ui/Select.svelte";
  import Switch from "$lib/components/ui/Switch.svelte";
  import { SETTINGS_PRIMARY_BUTTON_CLASS, SETTINGS_SECONDARY_BUTTON_CLASS } from "$lib/components/settings/action-buttons";

  /**
   * Settings > Agents > Access profiles: the profile directory beside an editor for the selected
   * profile. Built-in profiles are read only and can be duplicated; custom profiles publish reviewed
   * revisions and can be archived. Unsaved edits are reported through `onDraftStateChange`, and
   * leaving them goes through `onRequestNavigation`, which asks before discarding.
   */
  let {
    onRequestNavigation = (navigate) => navigate(),
    onDraftStateChange = () => {},
  }: {
    onRequestNavigation?: (navigate: () => void) => void;
    onDraftStateChange?: (open: boolean) => void;
  } = $props();

  type DraftMode = "existing" | "create" | "duplicate";
  type HistoryBoundaryKind = "entire" | "fromGrant";

  interface ProfileDraft {
    displayName: string;
    readHistory: boolean;
    participate: boolean;
    historyBoundary: HistoryBoundaryKind;
    maximumFolderCapability: ChatFolderCapability;
  }

  /** Tile sizes match the agent avatars in the Agents tab directory and editor heading. */
  const DIRECTORY_TILE_SIZE = 32;
  const LARGE_TILE_SIZE = 38;
  const TILE_ICON_RATIO = 0.5;
  const NEW_PROFILE_DRAFT: ProfileDraft = {
    displayName: "",
    readHistory: true,
    participate: true,
    historyBoundary: "entire",
    maximumFolderCapability: "none",
  };

  const { t } = getLocalization();
  let profiles = $state<ChatAccessProfileRead[]>([]);
  let loading = $state(true);
  let loadError = $state<string | null>(null);
  let selectedId = $state<string | null>(null);
  let mode = $state<DraftMode>("existing");
  let draft = $state<ProfileDraft>({ ...NEW_PROFILE_DRAFT });
  let baseline = $state(snapshotOf(NEW_PROFILE_DRAFT));
  let impact = $state<ChatAccessProfileImpactPreviewRead | null>(null);
  let impactSnapshot = $state<string | null>(null);
  let saving = $state(false);
  let saveError = $state<string | null>(null);
  let archiveTarget = $state<ChatAccessProfileRead | null>(null);
  let directoryScrollElement = $state<HTMLElement>();
  let detailScrollElement = $state<HTMLElement>();

  const selected = $derived(profiles.find((profile) => profile.id === selectedId) ?? null);
  const currentSnapshot = $derived(snapshotOf(draft));
  const dirty = $derived(currentSnapshot !== baseline);
  const drafting = $derived(mode !== "existing");
  const builtIn = $derived(mode === "existing" && Boolean(selected?.builtinKey));
  /** Duplicates copy the source's saved revision, so only new and custom profiles edit their settings. */
  const settingsLocked = $derived(saving || builtIn || mode === "duplicate");
  const canSave = $derived(Boolean(
    draft.displayName.trim()
      && !saving
      && !builtIn
      && (drafting || dirty),
  ));
  const impactBlocked = $derived(Boolean(impact && impact.issues.length > 0));
  const historyOptions = $derived([
    { value: "entire", label: t("settings.agents.agents.historyEntire") },
    { value: "fromGrant", label: t("settings.agents.agents.historyFromGrant") },
  ]);
  const folderCapabilityOptions = $derived(CHAT_FOLDER_CAPABILITIES.map((capability) => ({
    value: capability,
    label: folderCapabilityLabel(capability),
  })));

  $effect(() => {
    onDraftStateChange(dirty);
    return () => onDraftStateChange(false);
  });

  $effect(() => {
    const snapshot = currentSnapshot;
    if (impact && impactSnapshot !== snapshot) {
      impact = null;
      impactSnapshot = null;
    }
  });

  onMount(() => {
    void loadProfiles();
  });

  function snapshotOf(value: ProfileDraft): string {
    return JSON.stringify({ ...value, displayName: value.displayName.trim() });
  }

  function draftFromProfile(profile: ChatAccessProfileRead, displayName: string): ProfileDraft {
    const revision = profile.latestRevision;
    return {
      displayName,
      readHistory: revision.defaultChannelCapabilities.readHistory,
      participate: revision.defaultChannelCapabilities.participate,
      historyBoundary: revision.defaultHistoryBoundary.kind === "entire" ? "entire" : "fromGrant",
      maximumFolderCapability: revision.maximumFolderCapability,
    };
  }

  function resetDraft(nextMode: DraftMode, next: ProfileDraft): void {
    mode = nextMode;
    draft = next;
    baseline = snapshotOf(next);
    impact = null;
    impactSnapshot = null;
    saveError = null;
  }

  async function loadProfiles(): Promise<void> {
    loading = true;
    loadError = null;
    try {
      profiles = await chatApi.listChatAccessProfiles(false);
      const current = profiles.find((profile) => profile.id === selectedId) ?? profiles[0];
      if (current) loadProfile(current);
      else beginCreate();
    } catch (cause: unknown) {
      loadError = chatErrorMessage(cause, t("settings.agents.accessProfiles.loadFailed"));
    } finally {
      loading = false;
    }
  }

  /** Retries loading the profiles and rejects when the load fails again. */
  async function retryLoadProfiles(): Promise<void> {
    await loadProfiles();
    if (loadError) throw new Error(loadError);
  }

  function profileLabel(profile: ChatAccessProfileRead): string {
    return profile.builtinKey
      ? t(`settings.agents.agents.profiles.${profile.builtinKey}`)
      : profile.displayName;
  }

  function folderCapabilityLabel(capability: ChatFolderCapability): string {
    if (capability === "read") return t("settings.agents.agents.folderCapabilities.read");
    if (capability === "edit") return t("settings.agents.agents.folderCapabilities.edit");
    if (capability === "execute") return t("settings.agents.agents.folderCapabilities.execute");
    if (capability === "publish") return t("settings.agents.agents.folderCapabilities.publish");
    return t("settings.agents.agents.folderCapabilities.none");
  }

  function isFolderCapability(value: string): value is ChatFolderCapability {
    return CHAT_FOLDER_CAPABILITIES.some((capability) => capability === value);
  }

  function loadProfile(profile: ChatAccessProfileRead): void {
    selectedId = profile.id;
    resetDraft("existing", draftFromProfile(profile, profileLabel(profile)));
  }

  function beginCreate(): void {
    resetDraft("create", { ...NEW_PROFILE_DRAFT });
  }

  function beginDuplicate(profile: ChatAccessProfileRead): void {
    selectedId = profile.id;
    resetDraft("duplicate", draftFromProfile(
      profile,
      t("settings.agents.accessProfiles.copyName", profileLabel(profile)),
    ));
  }

  function cancelDraft(): void {
    const fallback = selected ?? profiles[0];
    if (fallback) loadProfile(fallback);
  }

  function requestSelect(profile: ChatAccessProfileRead): void {
    if (!drafting && profile.id === selectedId) return;
    onRequestNavigation(() => loadProfile(profile));
  }

  function revisionInput() {
    return {
      defaultChannelCapabilities: { readHistory: draft.readHistory, participate: draft.participate },
      defaultHistoryBoundary: draft.historyBoundary === "entire"
        ? { kind: "entire" as const }
        : { kind: "fromGrant" as const },
      maximumFolderCapability: draft.maximumFolderCapability,
    };
  }

  function replaceProfile(next: ChatAccessProfileRead): void {
    profiles = profiles.some((profile) => profile.id === next.id)
      ? profiles.map((profile) => profile.id === next.id ? next : profile)
      : [...profiles, next];
    loadProfile(next);
  }

  /**
   * Creates the draft, or publishes a revision of the selected custom profile. A revision that
   * changes linked access is previewed first, and `previewAccepted` publishes it after review.
   */
  async function save(previewAccepted = false): Promise<void> {
    if (!canSave && !previewAccepted) return;
    saving = true;
    saveError = null;
    try {
      if (mode === "create") {
        replaceProfile(await chatApi.createChatAccessProfile({
          accessProfileId: `access-profile:${crypto.randomUUID()}`,
          displayName: draft.displayName.trim(),
          revision: revisionInput(),
        }));
        return;
      }
      if (mode === "duplicate" && selected) {
        replaceProfile(await chatApi.duplicateChatAccessProfile({
          sourceAccessProfileId: selected.id,
          accessProfileId: `access-profile:${crypto.randomUUID()}`,
          displayName: draft.displayName.trim(),
        }));
        return;
      }
      if (!selected || selected.builtinKey) return;
      const request = {
        accessProfileId: selected.id,
        expectedRevision: selected.revision,
        revision: revisionInput(),
      };
      if (!previewAccepted) {
        const preview = await chatApi.previewChatAccessProfileRevision(request);
        if (preview.issues.length > 0 || preview.isExpansion || preview.isReduction
          || preview.affectedChannelIds.length > 0) {
          impact = preview;
          impactSnapshot = currentSnapshot;
          return;
        }
      }
      replaceProfile(await chatApi.publishChatAccessProfileRevision(request));
    } catch (cause: unknown) {
      saveError = chatErrorMessage(cause, t("settings.agents.accessProfiles.saveFailed"));
    } finally {
      saving = false;
    }
  }

  async function archiveProfile(): Promise<void> {
    const target = archiveTarget;
    archiveTarget = null;
    if (!target || target.builtinKey || saving) return;
    saving = true;
    saveError = null;
    try {
      await chatApi.archiveChatAccessProfile({
        accessProfileId: target.id,
        expectedRevision: target.revision,
        archived: true,
      });
      profiles = profiles.filter((profile) => profile.id !== target.id);
      if (selectedId === target.id) {
        const fallback = profiles[0];
        if (fallback) loadProfile(fallback);
        else beginCreate();
      }
    } catch (cause: unknown) {
      saveError = chatErrorMessage(cause, t("settings.agents.accessProfiles.archiveFailed"));
    } finally {
      saving = false;
    }
  }

  function impactTitle(preview: ChatAccessProfileImpactPreviewRead): string {
    if (preview.isExpansion) return t("settings.agents.agents.expansionTitle");
    if (preview.isReduction) return t("settings.agents.agents.reductionTitle");
    return t("settings.agents.accessProfiles.impactTitle");
  }
</script>

{#snippet profileTile(capability: ChatFolderCapability, size: number)}
  {@const iconSize = Math.round(size * TILE_ICON_RATIO)}
  <ChatIconTile {size}>
    {#if capability === "read"}<Folder size={iconSize} />
    {:else if capability === "edit"}<FilePenLine size={iconSize} />
    {:else if capability === "execute"}<PencilRuler size={iconSize} />
    {:else if capability === "publish"}<GitPullRequest size={iconSize} />
    {:else}<MessagesSquare size={iconSize} />{/if}
  </ChatIconTile>
{/snippet}

{#snippet rowLabel(label: string)}
  <div class="min-w-0 flex-1 wrap-break-word text-[0.866667rem] text-foreground">{label}</div>
{/snippet}

<section class="access-profiles" data-agents-settings-subsection="accessProfiles">
  {#if loading && profiles.length === 0}
    <Loading layout="rows" rows={3} />
  {:else if loadError && profiles.length === 0}
    <div class="min-h-40 text-sm"><Notice layout="fill" message={loadError} onRetry={retryLoadProfiles} /></div>
  {:else}
    <div class="directory-layout">
      <aside class="directory-panel">
        <div class="scroll-frame">
          <div bind:this={directoryScrollElement} data-mobile-scroll-fade class="directory-scroll hide-scrollbar">
            <nav class="profile-directory" aria-label={t("settings.agents.accessProfiles.directoryLabel")}>
              {#each profiles as profile (profile.id)}
                {@const active = !drafting && profile.id === selectedId}
                <button
                  type="button"
                  class="profile-row"
                  class:active
                  aria-current={active ? "page" : undefined}
                  disabled={saving}
                  onclick={() => requestSelect(profile)}
                >
                  {@render profileTile(profile.latestRevision.maximumFolderCapability, DIRECTORY_TILE_SIZE)}
                  <span class="directory-summary">
                    <strong>{profileLabel(profile)}</strong>
                    <small>{profile.builtinKey ? t("settings.agents.accessProfiles.builtIn") : t("settings.agents.accessProfiles.custom")}</small>
                  </span>
                </button>
              {/each}
              {#if drafting}
                <button type="button" class="profile-row active" aria-current="page">
                  {@render profileTile(draft.maximumFolderCapability, DIRECTORY_TILE_SIZE)}
                  <span class="directory-summary">
                    <strong>{draft.displayName.trim() || t("settings.agents.accessProfiles.newProfile")}</strong>
                    <small>{t("settings.agents.accessProfiles.custom")}</small>
                  </span>
                </button>
              {:else}
                <button type="button" class="profile-row add-row" disabled={saving} onclick={() => onRequestNavigation(beginCreate)}>
                  <span class="draft-tile"><Plus size={15} /></span>
                  <span class="directory-summary"><strong>{t("settings.agents.accessProfiles.add")}</strong></span>
                </button>
              {/if}
            </nav>
          </div>
          <CalendarScrollbar scrollContainer={directoryScrollElement} wheelPassthrough />
        </div>
      </aside>

      <form class="detail-panel" aria-busy={saving} onsubmit={(event) => { event.preventDefault(); void save(); }}>
        <div class="detail-scroll-frame">
          <div bind:this={detailScrollElement} data-mobile-scroll-fade class="detail-scroll hide-scrollbar">
            <div class="editor-heading">
              {@render profileTile(draft.maximumFolderCapability, LARGE_TILE_SIZE)}
              <div class="min-w-0">
                <h3>{draft.displayName.trim() || t("settings.agents.accessProfiles.newProfile")}</h3>
                <p>{builtIn ? t("settings.agents.accessProfiles.builtIn") : t("settings.agents.accessProfiles.custom")}</p>
              </div>
            </div>

            {#if mode === "duplicate"}
              <Notice class="px-1 text-[0.8rem]" role="status" copy={false} message={t("settings.agents.accessProfiles.duplicateNotice")} />
            {/if}

            <section class="flex flex-col gap-3">
              {#if drafting}
                <div class="flex items-center justify-between gap-4 px-1 py-1 max-[480px]:flex-col max-[480px]:items-stretch max-[480px]:gap-2">
                  <label for="access-profile-name" class="min-w-0 flex-1 text-[0.866667rem] text-foreground">{t("settings.agents.accessProfiles.name")}</label>
                  <input
                    id="access-profile-name"
                    class="field w-56 min-w-0 px-2.5 text-left text-[0.8rem] font-medium text-foreground max-[480px]:w-full"
                    bind:value={draft.displayName}
                    placeholder={t("settings.agents.accessProfiles.namePlaceholder")}
                    maxlength="160"
                    required
                    disabled={saving}
                    spellcheck="false"
                    oninput={() => { saveError = null; }}
                  />
                </div>
              {/if}
              <div class="flex items-center justify-between gap-4 px-1 py-1">
                {@render rowLabel(t("settings.agents.accessProfiles.readHistory"))}
                <Switch checked={draft.readHistory} disabled={settingsLocked} ariaLabel={t("settings.agents.accessProfiles.readHistory")} onChange={(checked) => { draft.readHistory = checked; }} />
              </div>
              {#if draft.readHistory}
                <div class="flex items-center justify-between gap-4 px-1 py-1 max-[480px]:flex-col max-[480px]:items-stretch max-[480px]:gap-2">
                  {@render rowLabel(t("settings.agents.agents.history"))}
                  <Select inline value={draft.historyBoundary} options={historyOptions} disabled={settingsLocked} ariaLabel={t("settings.agents.agents.history")} onChange={(value) => { draft.historyBoundary = value === "fromGrant" ? "fromGrant" : "entire"; }} />
                </div>
              {/if}
              <div class="flex items-center justify-between gap-4 px-1 py-1">
                {@render rowLabel(t("settings.agents.accessProfiles.participate"))}
                <Switch checked={draft.participate} disabled={settingsLocked} ariaLabel={t("settings.agents.accessProfiles.participate")} onChange={(checked) => { draft.participate = checked; }} />
              </div>
              <div class="flex items-center justify-between gap-4 px-1 py-1 max-[480px]:flex-col max-[480px]:items-stretch max-[480px]:gap-2">
                {@render rowLabel(t("settings.agents.accessProfiles.maximumFolderCapability"))}
                <Select inline value={draft.maximumFolderCapability} options={folderCapabilityOptions} disabled={settingsLocked} ariaLabel={t("settings.agents.accessProfiles.maximumFolderCapability")} onChange={(value) => { if (isFolderCapability(value)) draft.maximumFolderCapability = value; }} />
              </div>
            </section>

            {#if impact}
              <Notice
                class="px-1 text-[0.8rem]"
                tone="warning"
                role={impactBlocked ? "alert" : "status"}
                copy={false}
                title={impactTitle(impact)}
                message={t("settings.agents.accessProfiles.impact", impact.affectedAgentIds.length, impact.affectedChannelIds.length, impact.activeAuthorizationCount)}
              />
              {#each impact.issues as issue, index (`${issue.code}-${index}`)}
                <Notice class="px-1 text-[0.8rem]" tone="warning" copy={false} message={t(`chatErrors.accessIssues.${issue.code}`)} />
              {/each}
            {/if}
          </div>
          <CalendarScrollbar scrollContainer={detailScrollElement} wheelPassthrough />
        </div>

        <footer class="detail-footer">
          <div>
            {#if drafting}
              <button type="button" class={SETTINGS_SECONDARY_BUTTON_CLASS} disabled={saving} onclick={cancelDraft}>
                <X size={14} strokeWidth={1.9} class="shrink-0" />
                <span>{t("common.cancel")}</span>
              </button>
            {:else if selected}
              {@const source = selected}
              <button type="button" class={SETTINGS_SECONDARY_BUTTON_CLASS} disabled={saving} onclick={() => onRequestNavigation(() => beginDuplicate(source))}>
                <Copy size={14} strokeWidth={1.9} class="shrink-0" />
                <span>{t("settings.agents.accessProfiles.duplicate")}</span>
              </button>
              {#if !source.builtinKey}
                <button type="button" class={SETTINGS_SECONDARY_BUTTON_CLASS} disabled={saving} onclick={() => { archiveTarget = source; }}>
                  <Archive size={14} strokeWidth={1.9} class="shrink-0" />
                  <span>{t("settings.agents.accessProfiles.archive")}</span>
                </button>
              {/if}
            {/if}
          </div>
          <div class="save-area">
            {#if saveError}<span class="save-error" role="alert">{saveError}</span>{/if}
            {#if impact && !impactBlocked}
              <button type="button" class={SETTINGS_PRIMARY_BUTTON_CLASS} disabled={saving} onclick={() => void save(true)}>
                <Save size={14} strokeWidth={1.9} class="shrink-0" />
                <span>{saving ? t("settings.agents.accessProfiles.saving") : t("settings.agents.accessProfiles.publish")}</span>
              </button>
            {:else if !builtIn}
              <button type="submit" class={SETTINGS_PRIMARY_BUTTON_CLASS} disabled={!canSave || impactBlocked}>
                {#if drafting}<Plus size={14} strokeWidth={1.9} class="shrink-0" />{:else}<Save size={14} strokeWidth={1.9} class="shrink-0" />{/if}
                <span>{saving ? t("settings.agents.accessProfiles.saving") : drafting ? t("settings.agents.accessProfiles.create") : t("settings.agents.accessProfiles.save")}</span>
              </button>
            {/if}
          </div>
        </footer>
      </form>
    </div>
  {/if}
</section>

{#if archiveTarget}
  <ConfirmDialog
    title={t("settings.agents.accessProfiles.archiveTitle", profileLabel(archiveTarget))}
    message={t("settings.agents.accessProfiles.archiveMessage")}
    confirmLabel={t("settings.agents.accessProfiles.archive")}
    cancelLabel={t("common.cancel")}
    onConfirm={() => void archiveProfile()}
    onCancel={() => { archiveTarget = null; }}
  />
{/if}

<style>
  .access-profiles { display:grid; height:100%; min-height:0; grid-template-rows:minmax(0,1fr); }
  .directory-layout { display:grid; min-height:0; isolation:isolate; grid-template-columns:minmax(12.5rem,0.62fr) minmax(0,1.6fr); }
  .directory-panel { position:relative; z-index:1; min-width:0; min-height:0; padding-right:0.75rem; }
  .scroll-frame,.detail-scroll-frame { position:relative; height:100%; min-height:0; }
  .directory-scroll,.detail-scroll { height:100%; overflow-y:auto; overscroll-behavior:contain; }
  .profile-directory { display:grid; align-content:start; gap:0; padding:0.15rem 0.2rem 0.15rem 0; }
  .profile-row { display:grid; min-width:0; grid-template-columns:auto minmax(0,1fr); align-items:center; gap:0.55rem; margin-inline:0.2rem; border-radius:0.5rem; padding:0.6rem; text-align:left; }
  .profile-row:hover:not(:disabled),.profile-row.active { background:var(--accent); color:var(--accent-foreground); }
  .add-row { color:var(--muted-foreground); }
  .add-row:hover:not(:disabled) { color:var(--accent-foreground); }
  .draft-tile { display:grid; width:2rem; height:2rem; place-items:center; border:1px dashed var(--border); border-radius:0.45rem; color:var(--muted-foreground); }
  .directory-summary { display:grid; min-width:0; }
  .directory-summary strong,.directory-summary small { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  .directory-summary strong { font-size:calc(0.8rem * var(--type-scale)); font-weight:600; }
  .directory-summary small { margin-top:0.05rem; color:var(--muted-foreground); font-size:calc(0.65rem * var(--type-scale)); }
  .detail-panel { position:relative; z-index:2; display:grid; min-width:0; min-height:0; grid-template-rows:minmax(0,1fr) auto; border-left:1px solid var(--border); padding-left:1rem; }
  .detail-footer,.detail-footer > div { display:flex; min-width:0; align-items:center; gap:0.5rem; }
  .detail-footer { justify-content:space-between; gap:0.75rem; border-top:1px solid var(--border); padding:0.65rem 0.75rem 0.65rem 0; }
  .save-area { margin-left:auto; justify-content:flex-end; }
  .save-error { min-width:0; overflow-wrap:anywhere; color:var(--destructive); font-size:calc(0.733333rem * var(--type-scale)); }
  .detail-scroll { display:grid; grid-template-columns:minmax(0,1fr); align-content:start; gap:1.5rem; padding:0.2rem 0.75rem 1rem 0; }
  .editor-heading { display:grid; min-width:0; grid-template-columns:auto minmax(0,1fr); align-items:center; gap:0.65rem; padding-inline:0.25rem; }
  .editor-heading h3,.editor-heading p { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  .editor-heading h3 { font-size:calc(0.833333rem * var(--type-scale)); font-weight:600; }
  .editor-heading p { margin-top:0.08rem; color:var(--muted-foreground); font-size:calc(0.7rem * var(--type-scale)); }
  button:disabled { cursor:not-allowed; }
  @media (max-width:700px) {
    .directory-layout { grid-template-columns:1fr; grid-template-rows:minmax(8rem,30%) minmax(0,1fr); }
    .directory-panel { border-bottom:1px solid var(--border); padding:0 0 0.75rem; }
    .detail-panel { border-left:0; padding:0.8rem 0 0; }
    .detail-scroll { padding-right:0.35rem; }
    .detail-footer { align-items:stretch; flex-direction:column; padding-right:0; }
    .detail-footer > div { justify-content:space-between; }
    .save-area :global(button) { flex:1; }
  }
</style>
