<script lang="ts">
  import { onMount, tick } from "svelte";
  import Archive from "@lucide/svelte/icons/archive";
  import Copy from "@lucide/svelte/icons/copy";
  import Plus from "@lucide/svelte/icons/plus";
  import Shield from "@lucide/svelte/icons/shield";
  import X from "@lucide/svelte/icons/x";
  import * as chatApi from "$lib/api/chat";
  import type {
    ChatAccessProfileImpactPreviewRead,
    ChatAccessProfileRead,
    ChatFolderCapability,
  } from "$lib/chat/contracts";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import Select from "$lib/components/ui/Select.svelte";
  import Checkbox from "$lib/components/ui/Checkbox.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";

  let {
    profiles,
    onProfilesChange,
    onClose,
  }: {
    profiles: ChatAccessProfileRead[];
    onProfilesChange: (profiles: ChatAccessProfileRead[]) => void;
    onClose: () => void;
  } = $props();

  type DraftMode = "existing" | "create" | "duplicate";
  type PendingNavigation = { kind: "close" } | { kind: "select"; profileId: string };

  const { t } = getLocalization();
  let dialog = $state<HTMLDivElement>();
  let selectedId = $state<string | null>(null);
  let mode = $state<DraftMode>("existing");
  let duplicateSourceId = $state<string | null>(null);
  let displayName = $state("");
  let readHistory = $state(true);
  let participate = $state(true);
  let historyBoundary = $state<"entire" | "fromGrant">("entire");
  let maximumFolderCapability = $state<ChatFolderCapability>("none");
  let baseline = $state("");
  let impact = $state<ChatAccessProfileImpactPreviewRead | null>(null);
  let impactSnapshot = $state<string | null>(null);
  let saving = $state(false);
  let error = $state<string | null>(null);
  let pendingNavigation = $state<PendingNavigation | null>(null);
  let archiveTarget = $state<ChatAccessProfileRead | null>(null);
  let initialProfileApplied = false;

  const selected = $derived(profiles.find((profile) => profile.id === selectedId) ?? null);
  const currentSnapshot = $derived(JSON.stringify({
    displayName: displayName.trim(),
    readHistory,
    participate,
    historyBoundary,
    maximumFolderCapability,
  }));
  const dirty = $derived(Boolean(baseline && currentSnapshot !== baseline));
  const editableRevision = $derived(mode === "create" || (mode === "existing" && !selected?.builtinKey));
  const canSave = $derived(Boolean(
    displayName.trim()
      && !saving
      && (mode !== "existing" || dirty)
      && (mode !== "existing" || !selected?.builtinKey),
  ));

  $effect(() => {
    const snapshot = currentSnapshot;
    if (impact && impactSnapshot !== snapshot) {
      impact = null;
      impactSnapshot = null;
    }
  });

  $effect(() => {
    if (initialProfileApplied || profiles.length === 0) return;
    initialProfileApplied = true;
    loadProfile(profiles[0]!);
  });

  onMount(() => {
    void tick().then(() => dialog?.focus());
  });

  function profileLabel(profile: ChatAccessProfileRead): string {
    return profile.builtinKey
      ? t(`settings.chat.teammates.profiles.${profile.builtinKey}`)
      : profile.displayName;
  }

  function snapshot(): string {
    return JSON.stringify({
      displayName: displayName.trim(),
      readHistory,
      participate,
      historyBoundary,
      maximumFolderCapability,
    });
  }

  function loadProfile(profile: ChatAccessProfileRead): void {
    selectedId = profile.id;
    mode = "existing";
    duplicateSourceId = null;
    displayName = profileLabel(profile);
    readHistory = profile.latestRevision.defaultChannelCapabilities.readHistory;
    participate = profile.latestRevision.defaultChannelCapabilities.participate;
    historyBoundary = profile.latestRevision.defaultHistoryBoundary.kind === "entire"
      ? "entire"
      : "fromGrant";
    maximumFolderCapability = profile.latestRevision.maximumFolderCapability;
    baseline = snapshot();
    impact = null;
    impactSnapshot = null;
    error = null;
  }

  function beginCreate(): void {
    selectedId = null;
    mode = "create";
    duplicateSourceId = null;
    displayName = "";
    readHistory = true;
    participate = true;
    historyBoundary = "entire";
    maximumFolderCapability = "none";
    baseline = JSON.stringify({
      displayName: "",
      readHistory,
      participate,
      historyBoundary,
      maximumFolderCapability,
    });
    impact = null;
    impactSnapshot = null;
    error = null;
  }

  function beginDuplicate(profile: ChatAccessProfileRead): void {
    selectedId = profile.id;
    mode = "duplicate";
    duplicateSourceId = profile.id;
    displayName = t("settings.chat.teammates.profileManager.copyName", profileLabel(profile));
    readHistory = profile.latestRevision.defaultChannelCapabilities.readHistory;
    participate = profile.latestRevision.defaultChannelCapabilities.participate;
    historyBoundary = profile.latestRevision.defaultHistoryBoundary.kind === "entire"
      ? "entire"
      : "fromGrant";
    maximumFolderCapability = profile.latestRevision.maximumFolderCapability;
    baseline = "duplicate";
    impact = null;
    impactSnapshot = null;
    error = null;
  }

  function requestSelect(profile: ChatAccessProfileRead): void {
    if (dirty || mode === "duplicate") {
      pendingNavigation = { kind: "select", profileId: profile.id };
      return;
    }
    loadProfile(profile);
  }

  function requestClose(): void {
    if (dirty || mode === "duplicate") {
      pendingNavigation = { kind: "close" };
      return;
    }
    onClose();
  }

  function discardAndContinue(): void {
    const pending = pendingNavigation;
    pendingNavigation = null;
    if (!pending) return;
    if (pending.kind === "close") {
      onClose();
      return;
    }
    const profile = profiles.find((entry) => entry.id === pending.profileId);
    if (profile) loadProfile(profile);
  }

  function revisionInput() {
    return {
      defaultChannelCapabilities: { readHistory, participate },
      defaultHistoryBoundary: historyBoundary === "entire"
        ? { kind: "entire" as const }
        : { kind: "fromGrant" as const },
      maximumFolderCapability,
    };
  }

  function replaceProfile(next: ChatAccessProfileRead): void {
    const nextProfiles = profiles.some((profile) => profile.id === next.id)
      ? profiles.map((profile) => profile.id === next.id ? next : profile)
      : [...profiles, next];
    onProfilesChange(nextProfiles);
    loadProfile(next);
  }

  async function save(previewAccepted = false): Promise<void> {
    if (!canSave && !previewAccepted) return;
    saving = true;
    error = null;
    try {
      if (mode === "create") {
        const created = await chatApi.createChatAccessProfile({
          accessProfileId: `access-profile:${crypto.randomUUID()}`,
          displayName: displayName.trim(),
          revision: revisionInput(),
        });
        replaceProfile(created);
        return;
      }
      if (mode === "duplicate" && duplicateSourceId) {
        const created = await chatApi.duplicateChatAccessProfile({
          sourceAccessProfileId: duplicateSourceId,
          accessProfileId: `access-profile:${crypto.randomUUID()}`,
          displayName: displayName.trim(),
        });
        replaceProfile(created);
        return;
      }
      if (!selected || selected.builtinKey) return;
      const request = {
        accessProfileId: selected.id,
        expectedRevision: selected.revision,
        revision: revisionInput(),
      };
      if (!previewAccepted) {
        impact = await chatApi.previewChatAccessProfileRevision(request);
        impactSnapshot = currentSnapshot;
        if (impact.issues.length > 0 || impact.isExpansion || impact.isReduction
          || impact.affectedChannelIds.length > 0) return;
      }
      replaceProfile(await chatApi.publishChatAccessProfileRevision(request));
    } catch (cause: unknown) {
      error = chatErrorMessage(cause, t("settings.chat.teammates.profileManager.saveFailed"));
    } finally {
      saving = false;
    }
  }

  async function archiveProfile(): Promise<void> {
    const target = archiveTarget;
    archiveTarget = null;
    if (!target || target.builtinKey || saving) return;
    saving = true;
    error = null;
    try {
      await chatApi.archiveChatAccessProfile({
        accessProfileId: target.id,
        expectedRevision: target.revision,
        archived: true,
      });
      const next = profiles.filter((profile) => profile.id !== target.id);
      onProfilesChange(next);
      if (selectedId === target.id) {
        const fallback = next[0];
        if (fallback) loadProfile(fallback);
        else beginCreate();
      }
    } catch (cause: unknown) {
      error = chatErrorMessage(cause, t("settings.chat.teammates.profileManager.archiveFailed"));
    } finally {
      saving = false;
    }
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key !== "Escape") return;
    if (pendingNavigation || archiveTarget) return;
    event.preventDefault();
    event.stopPropagation();
    requestClose();
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="profile-manager-backdrop surface-backdrop" role="presentation" onclick={(event) => { if (event.target === event.currentTarget) requestClose(); }}>
  <div bind:this={dialog} class="profile-manager surface-dialog" role="dialog" aria-modal="true" aria-labelledby="chat-access-profile-title" tabindex="-1">
    <header>
      <div><h3 id="chat-access-profile-title">{t("settings.chat.teammates.profileManager.heading")}</h3><p>{t("settings.chat.teammates.profileManager.description")}</p></div>
      <button type="button" aria-label={t("common.close")} onclick={requestClose}><X size={16} /></button>
    </header>
    <div class="profile-manager-body">
      <aside use:scrollEdgeFadeAction>
        <button type="button" class="new-profile" onclick={beginCreate}><Plus size={14} />{t("settings.chat.teammates.profileManager.new")}</button>
        <nav aria-label={t("settings.chat.teammates.profileManager.directoryLabel")}>
          {#each profiles as profile (profile.id)}
            <button type="button" class:active={mode === "existing" && selectedId === profile.id} onclick={() => requestSelect(profile)}>
              <Shield size={14} /><span><strong>{profileLabel(profile)}</strong><small>{profile.builtinKey ? t("settings.chat.teammates.profileManager.builtIn") : t("settings.chat.teammates.profileManager.custom")}</small></span>
            </button>
          {/each}
        </nav>
      </aside>
      <form onsubmit={(event) => { event.preventDefault(); void save(); }}>
        <div class="profile-editor-scroll" use:scrollEdgeFadeAction>
          <div class="profile-heading"><div><h4>{mode === "create" ? t("settings.chat.teammates.profileManager.new") : displayName}</h4><p>{mode === "duplicate" ? t("settings.chat.teammates.profileManager.duplicateDescription") : selected?.builtinKey ? t("settings.chat.teammates.profileManager.builtInDescription") : t("settings.chat.teammates.profileManager.customDescription")}</p></div>{#if selected}<button type="button" class="quiet-button" onclick={() => beginDuplicate(selected)}><Copy size={14} />{t("settings.chat.teammates.profileManager.duplicate")}</button>{/if}</div>
          <label><span>{t("settings.chat.teammates.profileManager.name")}</span><input bind:value={displayName} maxlength="160" disabled={mode === "existing"} /></label>
          <fieldset disabled={!editableRevision || mode === "duplicate"}>
            <legend>{t("settings.chat.teammates.profileManager.channelDefaults")}</legend>
            <div class="check-row"><Checkbox checked={readHistory} label={t("settings.chat.teammates.readHistory")} onChange={(checked) => { readHistory = checked; }} /><span><strong>{t("settings.chat.teammates.readHistory")}</strong><small>{t("settings.chat.teammates.readHistoryDescription")}</small></span></div>
            <div class="check-row"><Checkbox checked={participate} label={t("settings.chat.teammates.participate")} onChange={(checked) => { participate = checked; }} /><span><strong>{t("settings.chat.teammates.participate")}</strong><small>{t("settings.chat.teammates.participateDescription")}</small></span></div>
          </fieldset>
          <div class="profile-fields">
            <div class="select-field"><span>{t("settings.chat.teammates.history")}</span><Select value={historyBoundary} options={[{ value: "entire", label: t("settings.chat.teammates.historyEntire") }, { value: "fromGrant", label: t("settings.chat.teammates.historyFromGrant") }]} onChange={(value) => { historyBoundary = value === "fromGrant" ? "fromGrant" : "entire"; }} disabled={!editableRevision || mode === "duplicate" || !readHistory} class="w-full" /></div>
            <div class="select-field"><span>{t("settings.chat.teammates.profileManager.maximumFolderCapability")}</span><Select value={maximumFolderCapability} options={[{ value: "none", label: t("settings.chat.teammates.folderCapabilities.none") }, { value: "read", label: t("settings.chat.teammates.folderCapabilities.read") }, { value: "edit", label: t("settings.chat.teammates.folderCapabilities.edit") }, { value: "execute", label: t("settings.chat.teammates.folderCapabilities.execute") }, { value: "publish", label: t("settings.chat.teammates.folderCapabilities.publish") }]} onChange={(value) => { if (value === "none" || value === "read" || value === "edit" || value === "execute" || value === "publish") maximumFolderCapability = value; }} disabled={!editableRevision || mode === "duplicate"} class="w-full" /></div>
          </div>
          <p class="profile-note">{t("settings.chat.teammates.profileManager.ceilingNote")}</p>
          {#if impact}
            <aside class="profile-impact" role={impact.issues.length ? "alert" : "status"}>
              <strong>{impact.isExpansion ? t("settings.chat.teammates.expansionTitle") : impact.isReduction ? t("settings.chat.teammates.reductionTitle") : t("settings.chat.teammates.profileManager.impactTitle")}</strong>
              <p>{t("settings.chat.teammates.profileManager.impact", impact.affectedTeammateIds.length, impact.affectedChannelIds.length, impact.activeAuthorizationCount)}</p>
              {#each impact.issues as issue}<small>{issue.message}</small>{/each}
            </aside>
          {/if}
          {#if error}<Notice class="profile-error" message={error} />{/if}
        </div>
        <footer>
          <div>{#if mode === "existing" && selected && !selected.builtinKey}<button type="button" class="archive-button" disabled={saving} onclick={() => { archiveTarget = selected; }}><Archive size={14} />{t("settings.chat.teammates.profileManager.archive")}</button>{/if}</div>
          <div><button type="button" class="secondary-button" onclick={requestClose}>{t("common.close")}</button>{#if impact && impact.issues.length === 0}<button type="button" class="primary-button" disabled={saving} onclick={() => void save(true)}>{t("settings.chat.teammates.profileManager.applyRevision")}</button>{:else if mode !== "existing" || !selected?.builtinKey}<button type="submit" class="primary-button" disabled={!canSave}>{saving ? t("settings.chat.teammates.saving") : mode === "existing" ? t("settings.chat.teammates.profileManager.reviewRevision") : t("settings.chat.teammates.profileManager.create")}</button>{/if}</div>
        </footer>
      </form>
    </div>
  </div>
</div>

{#if pendingNavigation}
  <ConfirmDialog title={t("settings.chat.teammates.profileManager.discardTitle")} message={t("settings.chat.teammates.profileManager.discardMessage")} confirmLabel={t("settings.chat.teammates.profileManager.discard")} cancelLabel={t("settings.chat.teammates.keepEditing")} onConfirm={discardAndContinue} onCancel={() => { pendingNavigation = null; }} />
{/if}

{#if archiveTarget}
  <ConfirmDialog title={t("settings.chat.teammates.profileManager.archiveTitle", profileLabel(archiveTarget))} message={t("settings.chat.teammates.profileManager.archiveMessage")} confirmLabel={t("settings.chat.teammates.profileManager.archive")} cancelLabel={t("common.cancel")} onConfirm={() => void archiveProfile()} onCancel={() => { archiveTarget = null; }} />
{/if}

<style>
  .profile-manager-backdrop { position:fixed; z-index:110; inset:0; display:grid; place-items:center; padding:1rem; }
  .profile-manager { display:grid; width:min(56rem,90vw); height:min(40rem,80dvh); grid-template-rows:auto minmax(0,1fr); overflow:hidden; outline:0; }
  .profile-manager > header { display:flex; align-items:center; justify-content:space-between; gap:1rem; border-bottom:1px solid var(--border); padding:0.8rem 0.95rem; }
  .profile-manager > header h3 { font-size:calc(0.9rem * var(--type-scale)); font-weight:650; }
  .profile-manager > header p,.profile-heading p,.profile-note { margin-top:0.15rem; color:var(--muted-foreground); font-size:calc(0.67rem * var(--type-scale)); line-height:1rem; }
  .profile-manager > header button { display:grid; width:2rem; height:2rem; place-items:center; border-radius:0.42rem; }
  .profile-manager > header button:hover { background:var(--accent); }
  .profile-manager-body { display:grid; min-height:0; grid-template-columns:minmax(12rem,0.7fr) minmax(0,2fr); }
  .profile-manager-body > aside { min-height:0; border-right:1px solid var(--border); padding:0.6rem; overflow-y:auto; }
  .new-profile { display:flex; min-height:2.1rem; width:100%; align-items:center; justify-content:center; gap:0.4rem; border:1px solid var(--border); border-radius:0.45rem; font-size:calc(0.7rem * var(--type-scale)); font-weight:600; }
  .profile-manager nav { display:grid; gap:0.18rem; margin-top:0.55rem; }
  .profile-manager nav button { display:grid; min-height:2.75rem; grid-template-columns:auto minmax(0,1fr); align-items:center; gap:0.45rem; border-radius:0.45rem; padding:0.45rem; text-align:left; }
  .profile-manager nav button:hover,.profile-manager nav button.active { background:var(--accent); }
  .profile-manager nav span { display:grid; min-width:0; }
  .profile-manager nav strong,.profile-manager nav small { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  .profile-manager nav strong { font-size:calc(0.7rem * var(--type-scale)); }
  .profile-manager nav small { color:var(--muted-foreground); font-size:calc(0.6rem * var(--type-scale)); }
  .profile-manager form { display:grid; min-height:0; grid-template-rows:minmax(0,1fr) auto; }
  .profile-editor-scroll { display:grid; align-content:start; gap:0.9rem; padding:1rem; overflow-y:auto; }
  .profile-heading { display:flex; align-items:flex-start; justify-content:space-between; gap:1rem; padding-bottom:0.8rem; border-bottom:1px solid var(--border); }
  .profile-heading h4 { font-size:calc(0.82rem * var(--type-scale)); font-weight:650; }
  .quiet-button,.archive-button { display:flex; min-height:2rem; align-items:center; gap:0.35rem; border-radius:0.42rem; padding:0.35rem 0.55rem; font-size:calc(0.67rem * var(--type-scale)); font-weight:600; }
  .quiet-button:hover { background:var(--accent); }
  .archive-button { color:var(--destructive); }
  .archive-button:hover { background:color-mix(in srgb,var(--destructive) 12%,transparent); }
  .profile-editor-scroll > label,.select-field { display:grid; gap:0.3rem; color:var(--muted-foreground); font-size:calc(0.67rem * var(--type-scale)); font-weight:550; }
  .profile-editor-scroll input { min-height:2.1rem; border:1px solid var(--border); border-radius:var(--floating-item-radius); background:transparent; padding:0.42rem 0.55rem; color:var(--foreground); font-weight:400; outline:0; }
  .profile-editor-scroll fieldset { display:grid; grid-template-columns:repeat(2,minmax(0,1fr)); gap:0.55rem; }
  .profile-editor-scroll legend { grid-column:1/-1; margin-bottom:0.15rem; font-size:calc(0.7rem * var(--type-scale)); font-weight:650; }
  .check-row { display:grid; grid-template-columns:auto minmax(0,1fr); gap:0.5rem; padding-block:0.35rem; }
  .check-row span { display:grid; }
  .check-row strong { font-size:calc(0.68rem * var(--type-scale)); }
  .check-row small { margin-top:0.12rem; color:var(--muted-foreground); font-size:calc(0.6rem * var(--type-scale)); line-height:0.9rem; }
  .profile-fields { display:grid; grid-template-columns:repeat(2,minmax(0,1fr)); gap:0.7rem; }
  .profile-impact { border-left:2px solid var(--ring); padding:0.25rem 0.7rem; }
  .profile-impact strong { font-size:calc(0.7rem * var(--type-scale)); }
  .profile-impact p,.profile-impact small { display:block; margin-top:0.15rem; color:var(--muted-foreground); font-size:calc(0.64rem * var(--type-scale)); }
  .profile-editor-scroll > :global(.profile-error) { font-size:calc(0.67rem * var(--type-scale)); }
  .profile-manager form > footer { display:flex; align-items:center; justify-content:space-between; gap:0.7rem; border-top:1px solid var(--border); padding:0.65rem 0.8rem; }
  .profile-manager form > footer > div { display:flex; align-items:center; gap:0.45rem; }
  .primary-button,.secondary-button { display:inline-flex; min-height:2rem; align-items:center; justify-content:center; border-radius:0.42rem; padding:0.35rem 0.7rem; font-size:calc(0.7rem * var(--type-scale)); font-weight:600; }
  .primary-button { background:var(--primary); color:var(--primary-foreground); }
  .secondary-button { border:1px solid var(--border); background:var(--background); }
  button:disabled,fieldset:disabled { opacity:0.55; }
  @media (pointer:coarse) { button,.profile-editor-scroll input { min-height:2.75rem; } }
  @media (max-width:700px) { .profile-manager { width:100%; height:94dvh; }.profile-manager-body { grid-template-columns:1fr; grid-template-rows:minmax(7rem,28%) minmax(0,1fr); }.profile-manager-body > aside { border-right:0; border-bottom:1px solid var(--border); }.profile-manager nav { grid-template-columns:repeat(auto-fill,minmax(10rem,1fr)); }.profile-fields,.profile-editor-scroll fieldset { grid-template-columns:1fr; } }
</style>
