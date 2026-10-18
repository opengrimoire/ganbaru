<script lang="ts">
  import ArchiveRestore from "@lucide/svelte/icons/archive-restore";
  import Archive from "@lucide/svelte/icons/archive";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import FolderPlus from "@lucide/svelte/icons/folder-plus";
  import Link2 from "@lucide/svelte/icons/link-2";
  import Pencil from "@lucide/svelte/icons/pencil";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import ShieldCheck from "@lucide/svelte/icons/shield-check";
  import ShieldOff from "@lucide/svelte/icons/shield-off";
  import Star from "@lucide/svelte/icons/star";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import Unplug from "@lucide/svelte/icons/unplug";
  import WandSparkles from "@lucide/svelte/icons/wand-sparkles";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { ProjectWorkingFolderRead } from "$lib/chat/contracts";
  import { onDestroy } from "svelte";
  import { adoptChatProjectPrimaryWorkingFolder, getChat } from "$lib/stores/chat.svelte";
  import * as chatApi from "$lib/api/chat";
  import * as folderApi from "$lib/api/project-working-folders";
  import * as trustApi from "$lib/api/working-folder-trust";
  import { createProjectSettingsWorkingFoldersDraft } from "$lib/projects/settings/working-folders-draft.svelte";
  import { orderProjectWorkingFolders } from "$lib/projects/working-folder-order";
  import { cn } from "$lib/utils";
  import CollectionMenu from "$lib/components/collections/CollectionMenu.svelte";
  import ChatProviderIcon from "$lib/components/chat/identity/ChatProviderIcon.svelte";
  import Select from "$lib/components/ui/Select.svelte";
  import ProjectSettingsSectionHeading from "./ProjectSettingsSectionHeading.svelte";

  let { projectId, saving = false, onDirtyChange = () => {}, onBusyChange = () => {} }: {
    projectId: string;
    saving?: boolean;
    onDirtyChange?: (dirty: boolean) => void;
    onBusyChange?: (busy: boolean) => void;
  } = $props();

  type ProviderOption = {
    value: string;
    label: string;
    familyId: string | null;
    summary?: string;
    disabled?: boolean;
  };

  const chat = getChat();
  const { t } = getLocalization();
  let operationId = $state<string | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(true);
  let initializedProjectId: string | null = null;
  let operationGeneration = 0;
  const draft = createProjectSettingsWorkingFoldersDraft({
    pick: (id) => folderApi.pickProjectWorkingFolder(projectId, t("projects.settings.workingFolders.pickerTitle"), id),
    add: (request, selection) => persist(folderApi.addSelectedProjectWorkingFolder(request, selection)),
    bind: (id, selection) => persist(folderApi.bindSelectedProjectWorkingFolder(id, selection)),
    rename: (id, name, revision) => persist(folderApi.renameProjectWorkingFolder(id, name, revision)),
    archive: (id, revision) => persist(folderApi.archiveProjectWorkingFolder(id, revision)),
    restore: (id, revision) => persist(folderApi.restoreProjectWorkingFolder(id, revision)),
    recreate: (id) => persist(folderApi.recreateManagedProjectWorkingFolder(id)),
    remove: (id) => chat.removeWorkingFolder(id),
    provider: (id, value) => chat.setWorkingFolderProviderPreference(id, value),
    primary: async (id, folderId, revision) => {
      const primary = await chatApi.setChatProjectPrimaryWorkingFolder(id, folderId, revision);
      adoptChatProjectPrimaryWorkingFolder(primary);
      return primary;
    },
  });
  const folders = $derived(orderProjectWorkingFolders(draft.folders));
  const providerOptions = $derived.by(() => {
    const providers = chat.settings?.providerInstances ?? [];
    const families = chat.settings?.providerFamilies ?? [];
    const options: ProviderOption[] = [
      { value: "", label: t("projects.settings.workingFolders.providerAutomatic"), familyId: null },
      ...providers.map((provider): ProviderOption => ({
        value: provider.configuration.instanceId,
        label: provider.configuration.label,
        familyId: provider.configuration.familyId,
      })),
    ];
    for (const family of families) {
      if (providers.some((provider) => provider.configuration.familyId === family.familyId)) continue;
      options.push({
        value: `unconfigured-family:${family.familyId}`,
        label: family.displayName,
        familyId: family.familyId,
        summary: family.implementationStatus === "available"
          ? t("settings.chat.providers.notConfigured")
          : t("settings.chat.providers.notAvailable"),
        disabled: true,
      });
    }
    return options;
  });

  $effect(() => {
    const selectedProjectId = projectId;
    if (saving || initializedProjectId === selectedProjectId) return;
    initializedProjectId = selectedProjectId;
    operationGeneration += 1;
    draft.clear(selectedProjectId);
    let current = true;
    loading = true;
    error = null;
    void (async () => {
      try {
        await chat.ensureLoaded();
        const primary = await chatApi.readChatProjectPrimaryWorkingFolder(selectedProjectId);
        if (current) draft.load(selectedProjectId, chat.workingFolders, primary, chat.settings?.configuration.workingFolderProviderPreferences ?? {});
      } catch (reason) {
        if (current) error = errorMessage(reason);
      } finally {
        if (current) loading = false;
      }
    })();
    return () => { current = false; draft.invalidate(); };
  });

  $effect(() => { onDirtyChange(draft.dirty); });
  $effect(() => { onBusyChange(loading || operationId !== null); });
  onDestroy(() => { operationGeneration += 1; draft.invalidate(); onDirtyChange(false); onBusyChange(false); });

  /** Publish only successful Save commands to canonical Chat state. */
  async function persist(operation: Promise<ProjectWorkingFolderRead>): Promise<ProjectWorkingFolderRead> {
    const result = await operation;
    chat.workingFolders = [...chat.workingFolders.filter((entry) => entry.workingFolder.id !== result.workingFolder.id), result];
    return result;
  }

  /** Expose the folder draft to the parent panel's explicit Save boundary. */
  export async function saveDraft(): Promise<void> {
    if (loading || operationId !== null) throw new Error(t("projects.settings.workingFolders.primaryUnavailable"));
    await draft.save();
  }

  /** Restore pending folder edits together with the rest of Project settings. */
  export function discardDraft(): void { operationGeneration += 1; draft.discard(); error = null; }

  /** Serialize native preview and Open operations and surface their failures. */
  async function run(id: string, operation: () => Promise<void>): Promise<void> {
    if (operationId !== null || saving || loading || !draft.primaryReady) return;
    operationId = id;
    const generation = operationGeneration;
    error = null;
    try {
      await operation();
    } catch (reason) {
      if (generation === operationGeneration) error = errorMessage(reason);
    } finally {
      operationId = null;
    }
  }

  /** Describe binding problems without repeating healthy status on every row. */
  function statusLabel(folder: ProjectWorkingFolderRead): string {
    if (folder.bindingStatus === "available") return t("projects.settings.workingFolders.statusAvailable");
    if (folder.bindingStatus === "missing") return t("projects.settings.workingFolders.statusMissing");
    if (folder.bindingStatus === "repository_mismatch") return t("projects.settings.workingFolders.statusRepositoryMismatch");
    return t("projects.settings.workingFolders.statusUnbound");
  }

  /** Distinguish a repository mismatch from a missing device binding. */
  function statusClass(folder: ProjectWorkingFolderRead): string {
    if (folder.bindingStatus === "available") return "text-success";
    if (folder.bindingStatus === "repository_mismatch") return "text-destructive";
    return "text-warning";
  }

  /** Resolve the pending device-local provider preference. */
  function providerPreference(folder: ProjectWorkingFolderRead): string {
    return draft.preference(folder.workingFolder.id);
  }

  /** Keep a missing preference recognizable only in its own folder's picker. */
  function providerOptionsForFolder(folder: ProjectWorkingFolderRead): ProviderOption[] {
    const preference = providerPreference(folder);
    if (!preference || providerOptions.some((option) => option.value === preference)) return providerOptions;
    return [...providerOptions, {
      value: preference,
      label: t("projects.settings.workingFolders.providerUnavailable"),
      familyId: null,
      disabled: true,
    }];
  }

  /** Stage only configured provider instances or the automatic preference. */
  function selectProvider(folder: ProjectWorkingFolderRead, value: string): void {
    const option = providerOptions.find((entry) => entry.value === value);
    if (!option || option.disabled) return;
    draft.setPreference(folder.workingFolder.id, value);
  }

  /** Show the full device path when a folder has a local binding. */
  function pathLabel(folder: ProjectWorkingFolderRead): string {
    return folder.canonicalPath ?? t("projects.settings.workingFolders.noLocalPath");
  }

  /** Recover a managed folder or select a new external device binding. */
  function changeFolderLocation(folder: ProjectWorkingFolderRead): void {
    if (folder.workingFolder.archivedAt) return;
    if (folder.workingFolder.kind === "managed" && folder.bindingStatus === "available") return;
    void run(folder.workingFolder.id, () => {
      if (folder.workingFolder.kind === "managed") {
        draft.recreate(folder);
        return Promise.resolve();
      }
      return draft.choose(folder);
    });
  }

  /** Describe this device's trust decision for the folder's repository programs and hooks. */
  function trustLabel(folder: ProjectWorkingFolderRead): string {
    if (folder.trust === "trusted") return t("projects.settings.workingFolders.trustStateTrusted");
    if (folder.trust === "untrusted") return t("projects.settings.workingFolders.trustStateUntrusted");
    return t("projects.settings.workingFolders.trustStateUndecided");
  }

  /** Apply a device-local trust change immediately; it is not part of the Settings draft. */
  function changeTrust(folder: ProjectWorkingFolderRead): void {
    const id = folder.workingFolder.id;
    void run(id, async () => {
      const result = folder.trust === "trusted"
        ? await trustApi.revokeProjectWorkingFolderTrust(id)
        : await trustApi.decideProjectWorkingFolderTrust(id);
      if (!result) return;
      await persist(Promise.resolve(result));
      draft.adoptTrust(result);
    });
  }

  /** Rename an external folder after collecting a nonempty display name. */
  function rename(folder: ProjectWorkingFolderRead): void {
    const name = window.prompt(
      t("projects.settings.workingFolders.renamePrompt"),
      folder.workingFolder.displayName,
    );
    if (!name?.trim() || name.trim() === folder.workingFolder.displayName) return;
    draft.rename(folder, name.trim());
  }

  /** Confirm removal of the project association while keeping files on disk. */
  function remove(folder: ProjectWorkingFolderRead): void {
    if (!window.confirm(t("projects.settings.workingFolders.removeConfirm", folder.workingFolder.displayName))) return;
    draft.remove(folder);
  }

  /** Preserve useful error details from native folder operations. */
  function errorMessage(reason: unknown): string {
    return reason instanceof Error ? reason.message : String(reason);
  }
</script>

<section class="flex flex-col gap-2" data-project-working-folders>
  <div class="flex items-center justify-between gap-2">
    <ProjectSettingsSectionHeading label={t("projects.settings.workingFolders.title")} />
    <button
      type="button"
      class="flex min-h-7 shrink-0 items-center gap-1.5 rounded-md px-1.5 text-[0.733333rem] text-muted-foreground hover:bg-accent hover:text-foreground disabled:cursor-not-allowed disabled:opacity-50"
      disabled={saving || operationId !== null || loading || !draft.primaryReady}
      onclick={() => { void run("add", () => draft.choose()); }}
    >
      <FolderPlus size={14} strokeWidth={1.75} />
      {t("projects.settings.workingFolders.addExisting")}
    </button>
  </div>
  {#if error}
    <Notice class="px-1 text-xs" message={error} />
  {/if}
  <div class="flex flex-col gap-1.5">
    {#each folders as folder (folder.workingFolder.id)}
      {@const busy = saving || operationId !== null || loading}
      {@const isPrimary = draft.primaryId === folder.workingFolder.id}
      {@const isExternal = folder.workingFolder.kind === "external"}
      {@const needsRecreate = !isExternal && folder.bindingStatus !== "available"}
      <div
        class="flex min-w-0 items-center justify-between gap-4 px-1 py-1 max-[480px]:flex-col max-[480px]:items-stretch max-[480px]:gap-2"
        role="group"
        aria-label={folder.workingFolder.displayName}
      >
        <div class="flex min-w-0 flex-1 items-center gap-1.5">
          <div class="min-w-0">
            <span class="block truncate text-[0.866667rem] text-foreground" title={folder.workingFolder.displayName}>{folder.workingFolder.displayName}</span>
            {#if folder.workingFolder.archivedAt}
              <span class="text-[0.733333rem] text-muted-foreground">{t("projects.settings.workingFolders.archived")}</span>
            {/if}
            {#if folder.bindingStatus !== "available"}
              <p class={cn("mt-0.5 text-[0.733333rem]", statusClass(folder))}>{statusLabel(folder)}</p>
            {/if}
          </div>
          <CollectionMenu
            kind="actions"
            iconOnly
            label={t("projects.settings.workingFolders.actions", folder.workingFolder.displayName)}
            disabled={busy}
            triggerClass="size-6 h-6 shrink-0 justify-center px-0"
          >
            <div class="flex flex-col gap-1 px-2 py-1.5 text-panel-detail text-muted-foreground">
              <p class="break-all">{pathLabel(folder)}</p>
              {#if folder.workingFolder.repositoryKind === "git"}
                <p>{folder.currentBranch ? t("projects.settings.workingFolders.gitBranch", folder.currentBranch) : t("projects.settings.workingFolders.gitRepository")}</p>
              {/if}
              {#if draft.canOpen(folder)}
                <p>{trustLabel(folder)}</p>
              {/if}
            </div>
            <div class="menu-separator" aria-hidden="true"></div>
            <button
              type="button"
              class="menu-item"
              aria-label={t("projects.settings.workingFolders.openFolder", folder.workingFolder.displayName)}
              disabled={busy || !draft.canOpen(folder)}
              onclick={() => { void run(folder.workingFolder.id, () => chat.openWorkingFolder(folder.workingFolder.id)); }}
            ><ExternalLink size={14} /><span class="min-w-0 truncate">{t("projects.settings.workingFolders.open")}</span></button>
            <button
              type="button"
              class="menu-item"
              data-collection-menu-keep-open
              aria-pressed={isPrimary}
              disabled={busy || !draft.primaryReady || isPrimary || Boolean(folder.workingFolder.archivedAt)}
              onclick={() => draft.makePrimary(folder)}
            ><Star size={14} fill={isPrimary ? "currentColor" : "none"} /><span class="min-w-0 truncate">{isPrimary ? t("projects.settings.workingFolders.primary") : t("projects.settings.workingFolders.makePrimary")}</span></button>
            <button
              type="button"
              class="menu-item"
              disabled={busy || (!isExternal && !needsRecreate) || Boolean(folder.workingFolder.archivedAt)}
              onclick={() => changeFolderLocation(folder)}
            >
              {#if needsRecreate}<RefreshCw size={14} />{:else}<Link2 size={14} />{/if}
              <span class="min-w-0 truncate">{needsRecreate ? t("projects.settings.workingFolders.recreate") : folder.bindingStatus === "unbound" || folder.bindingStatus === "missing" ? t("projects.settings.workingFolders.locate") : t("projects.settings.workingFolders.rebind")}</span>
            </button>
            <button
              type="button"
              class="menu-item"
              disabled={busy || !draft.canOpen(folder)}
              onclick={() => changeTrust(folder)}
            >
              {#if folder.trust === "trusted"}<ShieldOff size={14} />{:else}<ShieldCheck size={14} />{/if}
              <span class="min-w-0 truncate">{folder.trust === "trusted" ? t("projects.settings.workingFolders.trustRevoke") : t("projects.settings.workingFolders.trustGrant")}</span>
            </button>
            <button type="button" class="menu-item" disabled={busy || !isExternal} onclick={() => rename(folder)}><Pencil size={14} /><span class="min-w-0 truncate">{t("projects.settings.workingFolders.rename")}</span></button>
            {#if folder.workingFolder.archivedAt}
              <button type="button" class="menu-item" disabled={busy || !isExternal} onclick={() => draft.setArchived(folder, false)}><ArchiveRestore size={14} /><span class="min-w-0 truncate">{t("projects.settings.workingFolders.restore")}</span></button>
            {:else}
              <button type="button" class="menu-item" disabled={busy || !isExternal} onclick={() => draft.setArchived(folder, true)}><Archive size={14} /><span class="min-w-0 truncate">{t("projects.settings.workingFolders.archive")}</span></button>
            {/if}
            <div class="menu-separator" aria-hidden="true"></div>
            <button type="button" class="menu-item menu-item-destructive" disabled={busy || !isExternal} onclick={() => remove(folder)}><Trash2 size={14} /><span class="min-w-0 truncate">{t("projects.settings.workingFolders.remove")}</span></button>
          </CollectionMenu>
        </div>
        <Select
          inline
          ariaLabel={t("projects.settings.workingFolders.providerForFolder", folder.workingFolder.displayName)}
          value={providerPreference(folder)}
          options={providerOptionsForFolder(folder)}
          disabled={busy}
          showSelectedSummary={false}
          class="w-44 shrink-0 max-[480px]:w-full"
          triggerProps={{ "data-app-tooltip": t("projects.settings.workingFolders.provider") }}
          onChange={(value) => selectProvider(folder, value)}
        >
          {#snippet leading(value: string)}
            {@const option = providerOptions.find((entry) => entry.value === value)}
            {#if value === ""}
              <WandSparkles size={14} strokeWidth={1.75} class="shrink-0 text-muted-foreground" />
            {:else if option?.familyId}
              <ChatProviderIcon familyId={option.familyId} label={option.label} size={14} monochrome />
            {:else}
              <Unplug size={14} strokeWidth={1.75} class="shrink-0 text-muted-foreground" />
            {/if}
          {/snippet}
        </Select>
      </div>
    {/each}
  </div>
</section>

