<script lang="ts">
  import { createTeammateEditorController } from "$lib/chat/teammates/editor-controller.svelte";
  import { onMount, tick, untrack } from "svelte";
  import Archive from "@lucide/svelte/icons/archive";
  import ArchiveRestore from "@lucide/svelte/icons/archive-restore";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Eye from "@lucide/svelte/icons/eye";
  import EyeOff from "@lucide/svelte/icons/eye-off";
  import Folder from "@lucide/svelte/icons/folder";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import ShieldCheck from "@lucide/svelte/icons/shield-check";
  import Plus from "@lucide/svelte/icons/plus";
  import Search from "@lucide/svelte/icons/search";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import X from "@lucide/svelte/icons/x";
  import * as chatApi from "$lib/api/chat";
  import type {
    ChatAiTeammateRead,
    ChatChannelRead,
    ChatFolderCapability,
    ChatRuntimeApprovalPolicy,
    ChatTeammateChannelAccessInput,
  } from "$lib/chat/contracts";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import { modelCompany } from "$lib/chat/composer/model-company";
  import { preferredProjectWorkingFolder } from "$lib/chat/workspace/working-folder-selection";
  import {
    applyAccessProfileToScope,
    applyChannelPresetToScope,
    channelCapabilityPreset,
    folderCapabilityFits,
    toggleSelectionGroup,
    type ChatTeammateAccessConfirmationImpact,
    type ChatChannelCapabilityPreset,
  } from "$lib/chat/teammates/access";
  import { formatList } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getChat } from "$lib/stores/chat.svelte";
  import { getProjects } from "$lib/stores/projects.svelte";
  import CalendarScrollbar from "$lib/components/calendar/CalendarScrollbar.svelte";
  import Select from "$lib/components/ui/Select.svelte";
  import Checkbox from "$lib/components/ui/Checkbox.svelte";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import ChatAccessControl from "$lib/components/chat/composer/ChatAccessControl.svelte";
  import ChatControlMenu, {
    type ChatControlIcon,
    type ChatControlOption,
  } from "$lib/components/chat/composer/ChatControlMenu.svelte";
  import ChatModelAvatar from "$lib/components/chat/identity/ChatModelAvatar.svelte";
  import ChatModelControls from "$lib/components/chat/composer/ChatModelControls.svelte";
  import ChatParticipantAvatar from "$lib/components/chat/identity/ChatParticipantAvatar.svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import ChatChannelAccessPicker from "./ChatChannelAccessPicker.svelte";
  import ChatChannelScopeControls from "./ChatChannelScopeControls.svelte";
  type ChatAccessProfilesManagerComponent = typeof import("./ChatAccessProfilesManager.svelte").default;

  let {
    initialTeammateId,
    initialChannelId,
    initialCreate = false,
    onDraftStateChange = () => {},
  }: {
    initialTeammateId?: string;
    initialChannelId?: string;
    initialCreate?: boolean;
    onDraftStateChange?: (open: boolean) => void;
  } = $props();

  type LifecycleAction = "archive" | "delete";

  interface TeammateAccessSummaryChannel {
    channel: ChatChannelRead;
    access: ChatTeammateChannelAccessInput;
    sortOrder: number;
  }

  interface TeammateAccessSummaryProject {
    id: string;
    name: string;
    sortOrder: number;
    channels: TeammateAccessSummaryChannel[];
  }

  interface TeammateAccessSummaryGroup {
    id: string;
    name: string;
    sortOrder: number;
    projects: TeammateAccessSummaryProject[];
  }

  const chat = getChat();
  const projects = getProjects();
  const localization = getLocalization();
  const { t } = localization;

  const editor = createTeammateEditorController({
    chat,
    t,
    initialChannelId: () => initialChannelId ?? null,
    closeAccessPicker: () => { accessPickerOpen = false; },
    setExpandedAccessChannel: (channelId) => { expandedAccessChannelId = channelId; },
    focusConflict: async () => {
      await tick();
      conflictPanelElement?.focus();
    },
    focusRecoveryNotice: () => {
      void tick().then(() => recoveryNoticeElement?.focus());
    },
  });

  let directoryQuery = $state("");

  let directoryScrollElement = $state<HTMLElement>();
  let detailScrollElement = $state<HTMLElement>();
  let saveButtonElement = $state<HTMLButtonElement>();
  let accessPickerOpen = $state(false);
  let accessPickerTriggerElement = $state<HTMLButtonElement>();

  let conflictPanelElement = $state<HTMLElement>();
  let recoveryNoticeElement = $state<HTMLElement>();

  let profileManagerOpen = $state(false);
  let profileManagerLoading = $state(false);
  let ChatAccessProfilesManager = $state<ChatAccessProfilesManagerComponent | null>(null);
  let profileManagerLoad: Promise<void> | null = null;
  let expandedAccessChannelId = $state<string | null>(null);
  let advancedChannelIds = $state<Set<string>>(new Set());

  let lifecycleAction = $state<LifecycleAction | null>(null);
  let lifecycleTarget = $state<ChatAiTeammateRead | null>(null);
  let lifecycleBusy = $state(false);

  let initialSelectionApplied = false;

  const filteredDirectoryTeammates = $derived.by(() => {
    const query = directoryQuery.trim().toLocaleLowerCase();
    if (!query) return editor.allDirectoryTeammates;
    return editor.allDirectoryTeammates.filter((teammate) => (
      `${teammate.participant.displayName} ${teammate.role}`.toLocaleLowerCase().includes(query)
    ));
  });

  const permissionWorkingFolderId = $derived(
    editor.accessDraft.flatMap((channel) => channel.folderGrants)
      .find((grant) => grant.isDefault)?.workingFolderId ?? null,
  );
  const draftCompany = $derived(editor.selectedProvider ? modelCompany(editor.selectedProvider.configuration.familyId, editor.selectedModel) : null);

  const draftConfigurationState = $derived(
    editor.selectedProvider?.configuration.enabled
      && editor.selectedProvider.lastProbe?.state === "healthy"
      && editor.modelSelectionValid
      ? "healthy"
      : "needs_setup",
  );

  const activeNavigationChannels = $derived(editor.navigationChannels.filter((channel) => channel.archivedAt === null));
  const selectedChannelIds = $derived(new Set(editor.accessDraft.map((channel) => channel.channelId)));
  const accessSummaryGroups = $derived.by(() => {
    const projectById = new Map(projects.projects.map((project) => [project.id, project]));
    const groupById = new Map(projects.groups.map((group) => [group.id, group]));
    const channelOrder = new Map(activeNavigationChannels.map((channel, index) => [channel.id, index]));
    const grouped = new Map<string, TeammateAccessSummaryGroup>();

    for (const access of editor.accessDraft) {
      const channel = editor.navigationChannels.find((entry) => entry.id === access.channelId);
      if (!channel) continue;
      const project = projectById.get(channel.projectId);
      const group = groupById.get(project?.groupId ?? "");
      const groupId = group?.id ?? `unknown-group:${project?.groupId ?? channel.projectId}`;
      let summaryGroup = grouped.get(groupId);
      if (!summaryGroup) {
        summaryGroup = {
          id: groupId,
          name: group?.name ?? t("settings.chat.teammates.unknownGroup"),
          sortOrder: group?.sortOrder ?? Number.MAX_SAFE_INTEGER,
          projects: [],
        };
        grouped.set(groupId, summaryGroup);
      }
      let summaryProject = summaryGroup.projects.find((entry) => entry.id === channel.projectId);
      if (!summaryProject) {
        summaryProject = {
          id: channel.projectId,
          name: project?.name ?? t("settings.chat.teammates.unknownProject"),
          sortOrder: project?.sortOrder ?? Number.MAX_SAFE_INTEGER,
          channels: [],
        };
        summaryGroup.projects.push(summaryProject);
      }
      summaryProject.channels.push({
        channel,
        access,
        sortOrder: channelOrder.get(channel.id) ?? Number.MAX_SAFE_INTEGER,
      });
    }

    return [...grouped.values()]
      .sort((left, right) => left.sortOrder - right.sortOrder || left.name.localeCompare(right.name))
      .map((group) => ({
        ...group,
        projects: group.projects
          .sort((left, right) => left.sortOrder - right.sortOrder || left.name.localeCompare(right.name))
          .map((project) => ({
            ...project,
            channels: project.channels.sort((left, right) => left.sortOrder - right.sortOrder),
          })),
      }));
  });

  const accessProfileOptions = $derived<ChatControlOption[]>(editor.accessProfiles.map((profile) => ({
    value: profile.id,
    label: profile.builtinKey
      ? t(`settings.chat.teammates.profiles.${profile.builtinKey}`)
      : profile.displayName,
    description: profileDescription(profile.latestRevision.maximumFolderCapability),
    icon: profileIcon(profile.latestRevision.maximumFolderCapability),
  })));

  const historyOptions = $derived([
    { value: "entire", label: t("settings.chat.teammates.historyEntire") },
    { value: "fromGrant", label: t("settings.chat.teammates.historyFromGrant") },
  ]);
  const channelRuntimeOptions = $derived([
    { value: "inherit", label: t("settings.chat.teammates.runtime.inherit") },
    { value: "ask", label: t("settings.chat.teammates.runtime.ask") },
    { value: "autoApprove", label: t("settings.chat.teammates.runtime.autoApprove") },
    { value: "unattended", label: t("settings.chat.teammates.runtime.unattended") },
    { value: "providerCustom", label: t("settings.chat.teammates.runtime.providerCustom") },
  ]);
  const folderRuntimeOptions = $derived([
    { value: "inherit", label: t("settings.chat.teammates.runtime.inheritChannel") },
    { value: "ask", label: t("settings.chat.teammates.runtime.ask") },
    { value: "autoApprove", label: t("settings.chat.teammates.runtime.autoApprove") },
    { value: "unattended", label: t("settings.chat.teammates.runtime.unattended") },
    { value: "providerCustom", label: t("settings.chat.teammates.runtime.providerCustom") },
  ]);

  $effect(() => {
    onDraftStateChange(editor.dirty);
    return () => onDraftStateChange(false);
  });

  $effect(() => {
    const snapshot = editor.currentDraftSnapshot;
    if (editor.accessConfirmationImpact && editor.accessConfirmationSnapshot !== snapshot) {
      editor.clearAccessConfirmation();
    }
  });

  $effect(() => {
    if (initialSelectionApplied || editor.loadingDirectory) return;
    initialSelectionApplied = true;
    if (initialCreate) {
      editor.beginCreate(initialChannelId ?? null);
      return;
    }
    if (initialTeammateId && editor.allDirectoryTeammates.some((entry) => entry.participant.id === initialTeammateId)) {
      editor.selectedId = initialTeammateId;
      expandedAccessChannelId = initialChannelId ?? null;
      return;
    }
    if (chat.teammates.length === 0) editor.beginCreate(initialChannelId ?? null);
    else editor.selectedId = chat.teammates[0]?.participant.id ?? null;
  });

  $effect(() => {
    const teammate = editor.selected;
    if (!teammate || editor.creating) return;
    untrack(() => editor.initializeSelectedTeammate(teammate));
  });

  onMount(() => {
    void editor.loadDirectoryData();
  });

  function closeAccessPicker(): void {
    accessPickerOpen = false;
    void tick().then(() => accessPickerTriggerElement?.focus());
  }

  function profileDescription(capability: ChatFolderCapability): string {
    if (capability === "read") return t("settings.chat.teammates.profileDescriptions.readOnly");
    if (capability === "edit") return t("settings.chat.teammates.profileDescriptions.editFiles");
    if (capability === "execute") return t("settings.chat.teammates.profileDescriptions.buildAndTest");
    if (capability === "publish") return t("settings.chat.teammates.profileDescriptions.publishChanges");
    return t("settings.chat.teammates.profileDescriptions.conversationOnly");
  }

  function profileIcon(capability: ChatFolderCapability): ChatControlIcon {
    if (capability === "read") return "folder";
    if (capability === "edit") return "file-pen";
    if (capability === "execute") return "pencil-ruler";
    if (capability === "publish") return "git-pull-request";
    return "messages-square";
  }

  function folderCapabilityLabel(capability: ChatFolderCapability): string {
    if (capability === "read") return t("settings.chat.teammates.folderCapabilities.read");
    if (capability === "edit") return t("settings.chat.teammates.folderCapabilities.edit");
    if (capability === "execute") return t("settings.chat.teammates.folderCapabilities.execute");
    if (capability === "publish") return t("settings.chat.teammates.folderCapabilities.publish");
    return t("settings.chat.teammates.folderCapabilities.none");
  }

  function channelById(channelId: string): ChatChannelRead | null {
    return editor.navigationChannels.find((channel) => channel.id === channelId) ?? null;
  }

  function compactChannelList(channelIds: readonly string[]): string {
    const visibleNames = channelIds.slice(0, 3).map((channelId) => (
      `#${channelById(channelId)?.name ?? channelId}`
    ));
    const formatted = formatList(localization.locale, visibleNames);
    const hiddenCount = channelIds.length - visibleNames.length;
    return hiddenCount > 0
      ? t("settings.chat.teammates.accessConfirmation.listWithMore", formatted, hiddenCount)
      : formatted;
  }

  function accessConfirmationMessage(impact: ChatTeammateAccessConfirmationImpact): string {
    const teammateName = editor.displayName.trim();
    const consequences: string[] = [];
    if (impact.historyChannelIds.length > 0) {
      consequences.push(t(
        "settings.chat.teammates.accessConfirmation.history",
        teammateName,
        compactChannelList(impact.historyChannelIds),
      ));
    }
    if (impact.entireHistoryChannelIds.length > 0) {
      consequences.push(t(
        "settings.chat.teammates.accessConfirmation.entireHistory",
        teammateName,
        compactChannelList(impact.entireHistoryChannelIds),
      ));
    }
    if (impact.executeFolderIds.length > 0) {
      consequences.push(t(
        "settings.chat.teammates.accessConfirmation.executeFolders",
        teammateName,
        impact.executeFolderIds.length,
      ));
    }
    if (impact.publishFolderIds.length > 0) {
      consequences.push(t(
        "settings.chat.teammates.accessConfirmation.publishFolders",
        teammateName,
        impact.publishFolderIds.length,
      ));
    }
    if (impact.removedChannelIds.length > 0) {
      consequences.push(t(
        "settings.chat.teammates.accessConfirmation.removeChannels",
        compactChannelList(impact.removedChannelIds),
      ));
    }
    return consequences.join("\n");
  }

  function updateAccessChannel(
    channelId: string,
    update: (channel: ChatTeammateChannelAccessInput) => ChatTeammateChannelAccessInput,
  ): void {
    editor.accessDraft = editor.accessDraft.map((channel) => channel.channelId === channelId ? update(channel) : channel);
    editor.clearAccessConfirmation();
  }

  function setBoundedSelection(channelIds: readonly string[], selectedValue: boolean): void {
    const next = toggleSelectionGroup([...channelIds], selectedChannelIds, selectedValue);
    const byId = new Map(editor.accessDraft.map((channel) => [channel.channelId, channel]));
    editor.accessDraft = [...next].map((channelId) => byId.get(channelId) ?? editor.defaultChannelAccess(channelId));
    editor.clearAccessConfirmation();
  }

  function handleChannelSelection(channelIds: readonly string[], selectedValue: boolean): void {
    if (!selectedValue && expandedAccessChannelId && channelIds.includes(expandedAccessChannelId)) {
      expandedAccessChannelId = null;
    }
    setBoundedSelection(channelIds, selectedValue);
  }

  function channelHasAdditionalAccess(channel: ChatTeammateChannelAccessInput): boolean {
    const profile = editor.accessProfiles.find((entry) => entry.id === channel.accessProfileId);
    return channel.capabilities.readHistory
      || channelCapabilityPreset(channel.capabilities) === "custom"
      || (profile?.latestRevision.maximumFolderCapability ?? "none") !== "none"
      || channel.folderGrants.length > 0;
  }

  function setChannelPreset(
    channelId: string,
    preset: Exclude<ChatChannelCapabilityPreset, "custom">,
  ): void {
    setPresetForChannels([channelId], preset);
    if (preset !== "isolatedResponder") expandedAccessChannelId = channelId;
  }

  function setChannelAccessProfile(channelId: string, profileId: string): void {
    setAccessProfileForChannels([channelId], profileId);
    const profile = editor.accessProfiles.find((entry) => entry.id === profileId);
    if (profile?.latestRevision.maximumFolderCapability !== "none") {
      expandedAccessChannelId = channelId;
    }
  }

  function overflowTooltip(node: HTMLElement, label: string): {
    update: (nextLabel: string) => void;
    destroy: () => void;
  } {
    let currentLabel = label;
    const sync = (): void => {
      if (node.scrollWidth - node.clientWidth > 2) node.dataset.appTooltip = currentLabel;
      else delete node.dataset.appTooltip;
    };
    const observer = new ResizeObserver(sync);
    observer.observe(node);
    queueMicrotask(sync);
    return {
      update(nextLabel: string): void {
        currentLabel = nextLabel;
        sync();
      },
      destroy(): void {
        observer.disconnect();
        delete node.dataset.appTooltip;
      },
    };
  }

  function setPresetForChannels(
    channelIds: readonly string[],
    preset: Exclude<ChatChannelCapabilityPreset, "custom">,
  ): void {
    editor.accessDraft = applyChannelPresetToScope(editor.accessDraft, new Set(channelIds), preset);
    editor.clearAccessConfirmation();
  }

  function folderCapabilityOptions(ceiling: ChatFolderCapability) {
    return (["read", "edit", "execute", "publish"] as const)
      .filter((capability) => folderCapabilityFits(capability, ceiling))
      .map((capability) => ({ value: capability, label: folderCapabilityLabel(capability) }));
  }

  function setAccessProfileForChannels(channelIds: readonly string[], accessProfileId: string): void {
    const profile = editor.accessProfiles.find((entry) => entry.id === accessProfileId);
    const selectedIds = new Set(channelIds);
    const capability = profile?.latestRevision.maximumFolderCapability ?? "none";
    editor.accessDraft = applyAccessProfileToScope(editor.accessDraft, selectedIds, {
      id: accessProfileId,
      revision: profile?.latestRevision.revision ?? 0,
      maximumFolderCapability: capability,
    });
    if (capability !== "none") {
      editor.accessDraft = editor.accessDraft.map((channel) => {
        if (!selectedIds.has(channel.channelId)) return channel;
        if (channel.folderGrants.length > 0) {
          const nativeTarget = capability === "execute" || capability === "publish";
          const defaultFolderId = channel.folderGrants.find((grant) => grant.isDefault)?.workingFolderId
            ?? channel.folderGrants[0]?.workingFolderId;
          return {
            ...channel,
            folderGrants: channel.folderGrants.map((grant) => ({
              ...grant,
              capability,
              isDefault: nativeTarget && grant.workingFolderId === defaultFolderId,
            })),
          };
        }
        const projectId = channelById(channel.channelId)?.projectId;
        if (!projectId) return channel;
        const folder = preferredProjectWorkingFolder(chat.workingFolders, projectId, null);
        if (!folder) return channel;
        return {
          ...channel,
          folderGrants: [{
            workingFolderId: folder.workingFolder.id,
            capability,
            isDefault: capability === "execute" || capability === "publish",
            runtimeApprovalOverride: null,
          }],
        };
      });
    }
    editor.clearAccessConfirmation();
  }

  function foldersForChannel(channelId: string) {
    const projectId = channelById(channelId)?.projectId;
    return chat.workingFolders.filter((folder) => (
      folder.workingFolder.projectId === projectId && folder.workingFolder.archivedAt === null
    ));
  }

  function toggleFolderGrant(channelId: string, workingFolderId: string, selectedValue: boolean): void {
    updateAccessChannel(channelId, (channel) => {
      const grants = channel.folderGrants.filter((grant) => grant.workingFolderId !== workingFolderId);
      if (!selectedValue) return { ...channel, folderGrants: grants };
      const ceiling = editor.profileCeilings.get(channel.accessProfileId) ?? "none";
      const capability = ceiling === "none" ? "none" : ceiling;
      return {
        ...channel,
        folderGrants: [...grants, {
          workingFolderId,
          capability,
          isDefault: false,
          runtimeApprovalOverride: null,
        }],
      };
    });
  }

  function setFolderCapability(
    channelId: string,
    workingFolderId: string,
    capability: ChatFolderCapability,
  ): void {
    updateAccessChannel(channelId, (channel) => ({
      ...channel,
      folderGrants: channel.folderGrants.map((grant) => grant.workingFolderId === workingFolderId
        ? {
            ...grant,
            capability,
            isDefault: grant.isDefault,
          }
        : grant),
    }));
  }

  function setDefaultFolder(channelId: string, workingFolderId: string): void {
    updateAccessChannel(channelId, (channel) => ({
      ...channel,
      folderGrants: channel.folderGrants.map((grant) => ({
        ...grant,
        isDefault: grant.workingFolderId === workingFolderId,
      })),
    }));
  }

  function setFolderRuntimeApproval(
    channelId: string,
    workingFolderId: string,
    policy: ChatRuntimeApprovalPolicy | null,
  ): void {
    updateAccessChannel(channelId, (channel) => ({
      ...channel,
      folderGrants: channel.folderGrants.map((grant) => grant.workingFolderId === workingFolderId
        ? { ...grant, runtimeApprovalOverride: policy }
        : grant),
    }));
  }

  async function recoverFolder(folderId: string, bindingStatus: string, managed: boolean): Promise<void> {
    try {
      if (managed && bindingStatus === "missing") {
        await chat.recreateManagedWorkingFolder(folderId);
      } else if (bindingStatus === "repository_mismatch") {
        await chat.rebindWorkingFolder(folderId, t("settings.chat.teammates.locateFolder"));
      } else {
        await chat.locateWorkingFolder(folderId, t("settings.chat.teammates.locateFolder"));
      }
    } catch (cause: unknown) {
      editor.error = chatErrorMessage(cause, t("settings.chat.teammates.folderRecoveryFailed"));
    }
  }

  function loadProfileManager(): Promise<void> {
    if (ChatAccessProfilesManager) return Promise.resolve();
    profileManagerLoad ??= import("./ChatAccessProfilesManager.svelte")
      .then((module) => { ChatAccessProfilesManager = module.default; })
      .finally(() => { profileManagerLoad = null; });
    return profileManagerLoad;
  }

  async function openProfileManager(): Promise<void> {
    if (profileManagerLoading || profileManagerOpen) return;
    profileManagerLoading = true;
    try {
      await loadProfileManager();
      profileManagerOpen = true;
    } catch (cause: unknown) {
      editor.errorField = null;
      editor.error = chatErrorMessage(cause, t("settings.chat.teammates.loadFailed"));
    } finally {
      profileManagerLoading = false;
    }
  }

  function requestLifecycle(action: LifecycleAction): void {
    if (!editor.selected || lifecycleBusy) return;
    editor.lifecycleError = null;
    if (action === "archive" && editor.selected.activeAssignmentCount > 0) {
      editor.lifecycleError = t("settings.chat.teammates.archiveBlocked", editor.selected.activeAssignmentCount);
      return;
    }
    lifecycleAction = action;
    lifecycleTarget = editor.selected;
  }

  async function confirmLifecycle(): Promise<void> {
    const action = lifecycleAction;
    const target = lifecycleTarget;
    lifecycleAction = null;
    lifecycleTarget = null;
    if (!action || !target) return;
    editor.lifecycleError = null;
    lifecycleBusy = true;
    try {
      if (action === "archive") {
        await chatApi.archiveChatTeammate(target.participant.id, target.participant.revision, true);
      } else {
        await chatApi.deleteUnusedChatTeammate(target.participant.id, target.participant.revision);
      }
      editor.selectedId = null;
      await chat.refreshTeammates();
      await editor.loadDirectoryData();
      editor.lifecycleError = null;
    } catch (cause: unknown) {
      editor.lifecycleError = chatErrorMessage(cause, t("settings.chat.teammates.lifecycleFailed"));
    } finally {
      lifecycleBusy = false;
    }
  }

  async function restoreSelected(): Promise<void> {
    if (!editor.selected || lifecycleBusy) return;
    editor.lifecycleError = null;
    lifecycleBusy = true;
    try {
      const restored = await chatApi.archiveChatTeammate(
        editor.selected.participant.id,
        editor.selected.participant.revision,
        false,
      );
      await chat.refreshTeammates();
      await editor.loadDirectoryData();
      editor.showArchived = false;
      editor.selectedId = restored.participant.id;
      editor.lifecycleError = null;
    } catch (cause: unknown) {
      editor.lifecycleError = chatErrorMessage(cause, t("settings.chat.teammates.restoreFailed"));
    } finally {
      lifecycleBusy = false;
    }
  }

</script>

{#snippet channelAdditionalDetails(channelAccess: ChatTeammateChannelAccessInput)}
  {@const selectedProfile = editor.accessProfiles.find((profile) => profile.id === channelAccess.accessProfileId) ?? null}
  {@const preset = channelCapabilityPreset(channelAccess.capabilities)}
  <div class="access-details">
    {#if channelAccess.capabilities.readHistory}
      <div class="form-field history-field"><span>{t("settings.chat.teammates.history")}</span><Select inline class="w-full" value={channelAccess.historyBoundary.kind} options={historyOptions} ariaLabel={t("settings.chat.teammates.history")} disabled={editor.archivedMode} onChange={(value) => updateAccessChannel(channelAccess.channelId, (channel) => ({ ...channel, historyBoundary: value === "fromGrant" ? { kind: "fromGrant" } : { kind: "entire" } }))} /></div>
    {/if}
    {#if preset === "custom"}
      <fieldset class="capability-switches" disabled={editor.archivedMode}>
        <legend>{t("settings.chat.teammates.channelCapabilities")}</legend>
        <div><Checkbox checked={channelAccess.capabilities.readHistory} label={t("settings.chat.teammates.readHistory")} disabled={editor.archivedMode} onChange={(checked) => updateAccessChannel(channelAccess.channelId, (channel) => ({ ...channel, capabilities: { ...channel.capabilities, readHistory: checked }, historyBoundary: { kind: "entire" } }))} /><span><strong>{t("settings.chat.teammates.readHistory")}</strong><small>{t("settings.chat.teammates.readHistoryDescription")}</small></span></div>
        <div><Checkbox checked={channelAccess.capabilities.participate} label={t("settings.chat.teammates.participate")} disabled={editor.archivedMode} onChange={(checked) => updateAccessChannel(channelAccess.channelId, (channel) => ({ ...channel, capabilities: { ...channel.capabilities, participate: checked } }))} /><span><strong>{t("settings.chat.teammates.participate")}</strong><small>{t("settings.chat.teammates.participateDescription")}</small></span></div>
      </fieldset>
    {/if}
    {#if selectedProfile?.latestRevision.maximumFolderCapability !== "none"}
      <fieldset class="folder-access" disabled={editor.archivedMode}>
        <legend>{t("settings.chat.teammates.foldersForChannel")}</legend>
        {#each foldersForChannel(channelAccess.channelId) as folderRead (folderRead.workingFolder.id)}
          {@const grant = channelAccess.folderGrants.find((entry) => entry.workingFolderId === folderRead.workingFolder.id)}
          <div class="folder-row">
            <Checkbox checked={Boolean(grant)} label={t("settings.chat.teammates.allowFolder", folderRead.workingFolder.displayName)} disabled={editor.archivedMode} onChange={(checked) => toggleFolderGrant(channelAccess.channelId, folderRead.workingFolder.id, checked)} />
            <Folder size={14} />
            <span class="folder-name"><strong>{folderRead.workingFolder.displayName}</strong><small data-status={folderRead.bindingStatus}>{t(`settings.chat.teammates.binding.${folderRead.bindingStatus}`)}</small></span>
            <div class="folder-controls">
              {#if grant}
                <Select inline class="folder-select" value={grant.capability} options={folderCapabilityOptions(selectedProfile?.latestRevision.maximumFolderCapability ?? "none")} ariaLabel={t("settings.chat.teammates.folderCapabilityFor", folderRead.workingFolder.displayName)} onChange={(value) => setFolderCapability(channelAccess.channelId, folderRead.workingFolder.id, value as ChatFolderCapability)} />
                {#if advancedChannelIds.has(channelAccess.channelId)}<Select inline class="approval-select" value={grant.runtimeApprovalOverride ?? "inherit"} options={folderRuntimeOptions} ariaLabel={t("settings.chat.teammates.folderRuntimeApprovalFor", folderRead.workingFolder.displayName)} onChange={(value) => setFolderRuntimeApproval(channelAccess.channelId, folderRead.workingFolder.id, value === "inherit" ? null : value as ChatRuntimeApprovalPolicy)} />{/if}
                <label class="default-target"><input type="radio" name={`default-${channelAccess.channelId}`} checked={grant.isDefault} onchange={() => setDefaultFolder(channelAccess.channelId, folderRead.workingFolder.id)} />{t("settings.chat.teammates.defaultTarget")}</label>
              {/if}
              {#if folderRead.bindingStatus !== "available"}<button type="button" class="link-button" onclick={() => void recoverFolder(folderRead.workingFolder.id, folderRead.bindingStatus, folderRead.workingFolder.kind === "managed")}>{folderRead.workingFolder.kind === "managed" && folderRead.bindingStatus === "missing" ? t("settings.chat.teammates.recreate") : folderRead.bindingStatus === "repository_mismatch" ? t("settings.chat.teammates.relink") : t("settings.chat.teammates.locate")}</button>{/if}
            </div>
          </div>
        {/each}
      </fieldset>
    {/if}
    {#if channelAccess.folderGrants.length > 0}
      <button type="button" class="disclosure-button" aria-expanded={advancedChannelIds.has(channelAccess.channelId)} onclick={() => { const next = new Set(advancedChannelIds); if (next.has(channelAccess.channelId)) next.delete(channelAccess.channelId); else next.add(channelAccess.channelId); advancedChannelIds = next; }}><ChevronRight size={13} class={advancedChannelIds.has(channelAccess.channelId) ? "expanded" : undefined} />{advancedChannelIds.has(channelAccess.channelId) ? t("settings.chat.teammates.hideAdvancedAccess") : t("settings.chat.teammates.advancedAccess")}</button>
    {/if}
    {#if channelAccess.folderGrants.length > 0 && advancedChannelIds.has(channelAccess.channelId)}
      <div class="field-grid compact advanced-fields">
        <div class="form-field"><span>{t("settings.chat.teammates.channelRuntimeApproval")}</span><Select inline class="w-full" value={channelAccess.runtimeApprovalOverride ?? "inherit"} options={channelRuntimeOptions} ariaLabel={t("settings.chat.teammates.channelRuntimeApproval")} disabled={editor.archivedMode} onChange={(value) => updateAccessChannel(channelAccess.channelId, (channel) => ({ ...channel, runtimeApprovalOverride: value === "inherit" ? null : value as ChatRuntimeApprovalPolicy }))} /></div>
      </div>
    {/if}
  </div>
{/snippet}

<section class="teammate-settings" data-chat-settings-subsection="teammates">
  <header class="directory-header">
    <div><h2>{t("settings.chat.teammates.heading")}</h2></div>
    <div class="header-actions">
      {#if editor.archivedTeammates.length > 0}
        {@const archiveFilterLabel = editor.showArchived ? t("settings.chat.teammates.hideArchived") : t("settings.chat.teammates.includeArchived")}
        <button type="button" class="archive-filter" aria-label={archiveFilterLabel} aria-pressed={editor.showArchived} data-app-tooltip={archiveFilterLabel} disabled={editor.creating || editor.dirty || lifecycleBusy} onclick={() => { editor.showArchived = !editor.showArchived; }}><Archive size={14} /><span aria-hidden="true">{#if editor.showArchived}<Eye size={8} />{:else}<EyeOff size={8} />{/if}</span></button>
      {/if}
      <button type="button" class="settings-button" disabled={editor.creating || editor.dirty} onclick={() => editor.beginCreate(null)}><Plus size={13} />{t("settings.chat.teammates.add")}</button>
      <button type="button" class="settings-button" disabled={editor.dirty || profileManagerLoading} aria-busy={profileManagerLoading || undefined} onclick={() => void openProfileManager()}>{#if profileManagerLoading}<LoaderCircle size={13} class="animate-spin" aria-hidden="true" />{:else}<ShieldCheck size={13} />{/if}{t("settings.chat.teammates.profileManager.manage")}</button>
    </div>
  </header>

  <div class="directory-layout">
    <aside class="directory-panel">
      {#if editor.allDirectoryTeammates.length > 8}<label class="directory-search"><Search size={13} /><input bind:value={directoryQuery} aria-label={t("settings.chat.teammates.searchDirectory")} placeholder={t("settings.chat.teammates.searchDirectory")} /></label>{/if}
      <div class="scroll-frame">
        <div bind:this={directoryScrollElement} class="directory-scroll hide-scrollbar">
          <nav aria-label={t("settings.chat.teammates.directoryLabel")} class="directory-list teammate-directory">
            {#if editor.creating}
              <button type="button" class="directory-row active" aria-current="page"><span class="draft-avatar">{#if draftCompany}<ChatModelAvatar familyId={draftCompany.iconFamilyId} label={draftCompany.name} size={30} />{:else}<Plus size={15} />{/if}</span><span><strong>{editor.displayName.trim() || t("settings.chat.teammates.name")}</strong><small>{editor.role.trim() || t("settings.chat.teammates.role")}</small></span></button>
            {/if}
            {#each filteredDirectoryTeammates as teammate (teammate.participant.id)}
              <button type="button" class:active={!editor.creating && editor.selectedId === teammate.participant.id} class="directory-row" aria-current={!editor.creating && editor.selectedId === teammate.participant.id ? "page" : undefined} disabled={editor.dirty || lifecycleBusy} onclick={() => { editor.creating = false; editor.selectedId = teammate.participant.id; }}>
                <span class="directory-avatar"><ChatParticipantAvatar participant={teammate.participant} {teammate} size={32} /></span>
                <span class="directory-summary"><strong>{teammate.participant.displayName}</strong><small>{teammate.role}</small></span>
              </button>
            {/each}
            {#if editor.loadingDirectory}<Loading layout="rows" rows={3} class="col-span-full" />{:else if !editor.creating && filteredDirectoryTeammates.length === 0}<p class="empty-copy">{t("settings.chat.teammates.empty")}</p>{/if}
          </nav>
        </div>
        <CalendarScrollbar scrollContainer={directoryScrollElement} wheelPassthrough />
      </div>
    </aside>

    <div class="detail-panel">
      {#if editor.creating || editor.selected}
        <form class="editor teammate-editor" aria-busy={editor.saving || editor.conflictLoading} onsubmit={(event) => { event.preventDefault(); void editor.save(); }}>
          <div class="editor-scroll-frame">
            <div bind:this={detailScrollElement} class="editor-scroll hide-scrollbar">
              <div class="editor-heading">
                {#if editor.creating}
                  {#if draftCompany}<ChatModelAvatar familyId={draftCompany.iconFamilyId} label={draftCompany.name} size={38} />{:else}<span class="draft-avatar large"><Plus size={16} /></span>{/if}
                {:else if editor.selected}<ChatParticipantAvatar participant={editor.selected.participant} teammate={editor.selected} size={38} />{/if}
                <div class="editor-title">
                  <div class="editor-name-line">
                    <h3>{editor.displayName.trim() || t("settings.chat.teammates.name")}</h3>
                    {#if editor.creating}
                      <span class="editor-state" data-state={draftConfigurationState}><i></i>{draftConfigurationState === "healthy" ? t("settings.chat.teammates.available") : t("settings.chat.teammates.needsSetup")}</span>
                    {:else if editor.selected}
                      {#if editor.archivedMode}
                        <span class="editor-state archived-state"><Archive size={12} />{t("settings.chat.teammates.archived")}</span>
                      {:else}
                        <span class="editor-state" data-state={editor.selected.configurationState}><i></i>{editor.selected.configurationState === "healthy" ? t("settings.chat.teammates.available") : t("settings.chat.teammates.needsSetup")}</span>
                      {/if}
                    {/if}
                  </div>
                  <p>{editor.role.trim() || t("settings.chat.teammates.role")}</p>
                </div>
              </div>

              {#if editor.conflictLoading}
                <aside bind:this={conflictPanelElement} class="conflict-panel" role="status" tabindex="-1">
                  <div>
                    <strong>{t("settings.chat.teammates.conflict.loadingTitle")}</strong>
                    <p>{t("settings.chat.teammates.conflict.loadingDescription")}</p>
                  </div>
                </aside>
              {:else if editor.accessConflict && editor.conflictComparison}
                <aside bind:this={conflictPanelElement} class="conflict-panel" role="alert" tabindex="-1">
                  <div class="conflict-copy">
                    <strong>{t("settings.chat.teammates.conflict.title")}</strong>
                    <p>{t("settings.chat.teammates.conflict.description")}</p>
                    <ul>
                      <li>{editor.conflictComparison.identityChanged ? t("settings.chat.teammates.conflict.identityChanged") : t("settings.chat.teammates.conflict.identityUnchanged")}</li>
                      <li>{editor.conflictComparison.policyChanged ? t("settings.chat.teammates.conflict.policyChanged") : t("settings.chat.teammates.conflict.policyUnchanged")}</li>
                      <li>{t("settings.chat.teammates.conflict.channelSummary", editor.conflictComparison.addedChannelCount, editor.conflictComparison.removedChannelCount, editor.conflictComparison.changedChannelCount)}</li>
                    </ul>
                  </div>
                  <div class="conflict-actions">
                    <button type="button" class="primary-button" onclick={editor.rebaseAccessConflict}>{t("settings.chat.teammates.conflict.keepAndRebase")}</button>
                    <button type="button" class="secondary-button" onclick={() => void editor.reloadCurrentAccessConflict()}>{t("settings.chat.teammates.conflict.reloadCurrent")}</button>
                  </div>
                </aside>
              {:else if editor.conflictError}
                <div bind:this={conflictPanelElement} class="conflict-notice" tabindex="-1">
                  <Notice title={t("settings.chat.teammates.conflict.loadFailedTitle")} message={editor.conflictError} onRetry={editor.retryAccessConflict} />
                </div>
              {/if}

              {#if editor.conflictRecoveryNotice}
                <aside bind:this={recoveryNoticeElement} class="conflict-recovery-notice" role="status" tabindex="-1">
                  {editor.conflictRecoveryNotice === "rebased"
                    ? t("settings.chat.teammates.conflict.rebasedNotice")
                    : t("settings.chat.teammates.conflict.reloadedNotice")}
                </aside>
              {/if}

              <div class="editor-content">
                <section class="editor-section"><div class="section-heading"><h4>{t("settings.chat.teammates.identitySection")}</h4></div><div class="field-grid">
                  <div class="form-field full"><span id="teammate-name-label">{t("settings.chat.teammates.name")}<i class="required-marker" aria-hidden="true">*</i></span><input bind:value={editor.displayName} aria-labelledby="teammate-name-label" aria-describedby={editor.nameTaken || (editor.error && editor.errorField === "displayName") ? "teammate-name-error" : undefined} aria-invalid={editor.nameTaken || (editor.error && editor.errorField === "displayName") ? "true" : undefined} placeholder={t("settings.chat.teammates.namePlaceholder")} maxlength="160" required disabled={editor.archivedMode} oninput={() => editor.clearFieldError("displayName")} />{#if editor.nameTaken}<small id="teammate-name-error" class="field-error" role="alert">{t("settings.chat.teammates.nameTaken")}</small>{:else if editor.error && editor.errorField === "displayName"}<small id="teammate-name-error" class="field-error" role="alert">{editor.error}</small>{/if}</div>
                  <div class="form-field full"><span id="teammate-role-label">{t("settings.chat.teammates.role")}<i class="required-marker" aria-hidden="true">*</i></span><input bind:value={editor.role} aria-labelledby="teammate-role-label" aria-describedby={editor.error && editor.errorField === "role" ? "teammate-role-error" : undefined} aria-invalid={editor.error && editor.errorField === "role" ? "true" : undefined} placeholder={t("settings.chat.teammates.rolePlaceholder")} maxlength="1000" required disabled={editor.archivedMode} oninput={() => editor.clearFieldError("role")} />{#if editor.error && editor.errorField === "role"}<small id="teammate-role-error" class="field-error" role="alert">{editor.error}</small>{/if}</div>
                  <div class="form-field full"><span id="teammate-instructions-label">{t("settings.chat.teammates.instructions")}</span><textarea bind:value={editor.instructions} aria-labelledby="teammate-instructions-label" rows="4" maxlength="65536" disabled={editor.archivedMode} placeholder={t("settings.chat.teammates.instructionsPlaceholder")}></textarea></div>
                </div></section>

                <section class="editor-section"><div class="section-heading"><h4>{t("settings.chat.teammates.executionSection")}</h4></div><div class="field-grid execution-fields">
                  <div class="form-field execution-model-field"><span>{t("settings.chat.teammates.model")}<i class="required-marker" aria-hidden="true">*</i></span><ChatModelControls value={{ providerInstanceId: editor.providerId || null, modelId: editor.modelId || null, providerManaged: editor.providerManagedModel, options: editor.modelOptions }} disabled={editor.archivedMode} onChange={editor.selectExecution} /></div>
                  <div class="form-field execution-approval-field"><span>{t("settings.chat.teammates.approval")}</span><ChatAccessControl value={editor.safetyMode} providerInstanceId={editor.providerId || null} workingFolderId={permissionWorkingFolderId} disabled={editor.archivedMode} onChange={(value) => { editor.safetyMode = value; }} /></div>
                </div></section>

                <section class="editor-section access-section">
                  <div class="access-section-heading">
                    <div class="section-heading"><h4>{t("settings.chat.teammates.accessSection")}</h4></div>
                    <button
                      bind:this={accessPickerTriggerElement}
                      type="button"
                      class="access-configure-button"
                      aria-label={t("settings.chat.teammates.configureChannelAccess")}
                      aria-haspopup="dialog"
                      aria-expanded={accessPickerOpen}
                      disabled={editor.loadingAccess || editor.archivedMode || editor.accessProfiles.length === 0}
                      onclick={() => { accessPickerOpen = !accessPickerOpen; }}
                    >
                      <span>{t("settings.chat.teammates.configureChannelAccessAction")}</span>
                      <ChevronRight size={13} />
                    </button>
                  </div>
                  {#if accessSummaryGroups.length > 0}
                    <div class="access-summary-tree">
                      {#each accessSummaryGroups as summaryGroup (summaryGroup.id)}
                        <section class="access-summary-group">
                          <h5 use:overflowTooltip={summaryGroup.name}>{summaryGroup.name}</h5>
                          {#each summaryGroup.projects as summaryProject (summaryProject.id)}
                            <div class="access-summary-project">
                              <h6 use:overflowTooltip={summaryProject.name}>{summaryProject.name}</h6>
                              {#each summaryProject.channels as summaryChannel (summaryChannel.channel.id)}
                                {@const hasAdditionalAccess = channelHasAdditionalAccess(summaryChannel.access)}
                                <div class="access-summary-channel">
                                  <div class="access-summary-row">
                                    {#if hasAdditionalAccess}
                                      <button
                                        type="button"
                                        class="access-channel-toggle"
                                        aria-label={t("settings.chat.teammates.editChannelAccess")}
                                        aria-expanded={expandedAccessChannelId === summaryChannel.channel.id}
                                        onclick={() => {
                                          expandedAccessChannelId = expandedAccessChannelId === summaryChannel.channel.id
                                            ? null
                                            : summaryChannel.channel.id;
                                        }}
                                      >
                                        <ChevronRight size={12} class={expandedAccessChannelId === summaryChannel.channel.id ? "expanded" : undefined} />
                                        <strong use:overflowTooltip={summaryChannel.channel.name}>#{summaryChannel.channel.name}</strong>
                                      </button>
                                    {:else}
                                      <strong class="access-channel-name" use:overflowTooltip={summaryChannel.channel.name}>#{summaryChannel.channel.name}</strong>
                                    {/if}
                                    <div class="channel-primary-control">
                                      <ChatChannelScopeControls channels={[summaryChannel.access]} disabled={editor.archivedMode} onPresetChange={(preset) => setChannelPreset(summaryChannel.channel.id, preset)} />
                                    </div>
                                    <div class="channel-primary-control work-access-control">
                                      <ChatControlMenu
                                        value={summaryChannel.access.accessProfileId}
                                        options={accessProfileOptions}
                                        ariaLabel={t("settings.chat.teammates.workAccess")}
                                        onChange={(profileId) => setChannelAccessProfile(summaryChannel.channel.id, profileId)}
                                        disabled={editor.archivedMode}
                                        showTooltip={false}
                                      />
                                    </div>
                                  </div>
                                  {#if hasAdditionalAccess && expandedAccessChannelId === summaryChannel.channel.id}
                                    <div class="access-summary-details">
                                      {@render channelAdditionalDetails(summaryChannel.access)}
                                    </div>
                                  {/if}
                                </div>
                              {/each}
                            </div>
                          {/each}
                        </section>
                      {/each}
                    </div>
                  {:else if !editor.loadingAccess}
                    <p class="access-summary-empty">{t("settings.chat.teammates.noAccessTitle")}</p>
                  {/if}
                </section>
              </div>
            </div>
            <CalendarScrollbar scrollContainer={detailScrollElement} wheelPassthrough />
          </div>

          <footer class="editor-footer"><div>{#if editor.creating}<button type="button" class="secondary-button" onclick={editor.cancelCreate}><X size={14} />{t("common.cancel")}</button>{:else if editor.selected && editor.archivedMode}<button type="button" class="danger-button" disabled={editor.selected.hasDurableHistory} onclick={() => requestLifecycle("delete")}><Trash2 size={14} />{t("settings.chat.teammates.deletePermanently")}</button>{:else if editor.selected}<button type="button" class="secondary-button" disabled={editor.selected.activeAssignmentCount > 0} onclick={() => requestLifecycle("archive")}><Archive size={14} />{t("settings.chat.teammates.archive")}</button>{/if}{#if editor.lifecycleError}<span class="field-error" role="alert">{editor.lifecycleError}</span>{/if}</div><div class="save-area">{#if editor.error && editor.errorField !== "displayName" && editor.errorField !== "role"}<span class="field-error" role="alert">{editor.error}</span>{:else if editor.accessErrors.length > 0}<span class="field-error" role="alert">{t("settings.chat.teammates.accessValidationFailed")}</span>{:else if editor.providerResourceBlockers[0]}<span class="field-error" role="alert">{editor.providerResourceBlockers[0]}</span>{/if}{#if editor.archivedMode}<button type="button" class="primary-button" onclick={() => void restoreSelected()}><ArchiveRestore size={14} />{t("settings.chat.teammates.restore")}</button>{:else}<button bind:this={saveButtonElement} type="submit" class="primary-button" disabled={editor.saving || !editor.canSave}>{editor.saving ? t("settings.chat.teammates.saving") : editor.creating ? t("settings.chat.teammates.createInert") : t("settings.chat.teammates.save")}</button>{/if}</div></footer>
        </form>
      {:else}
        <div class="empty-detail"><p>{t("settings.chat.teammates.selectPrompt")}</p></div>
      {/if}
    </div>
  </div>
</section>

{#if accessPickerOpen && accessPickerTriggerElement}
  <ChatChannelAccessPicker
    anchor={accessPickerTriggerElement}
    channels={activeNavigationChannels}
    {selectedChannelIds}
    disabled={editor.archivedMode || editor.accessProfiles.length === 0}
    onSelectionChange={handleChannelSelection}
    onClose={closeAccessPicker}
  />
{/if}

{#if editor.accessConfirmationImpact}
  <ConfirmDialog
    title={t("settings.chat.teammates.accessConfirmation.title")}
    message={accessConfirmationMessage(editor.accessConfirmationImpact)}
    confirmLabel={t("settings.chat.teammates.accessConfirmation.confirm")}
    cancelLabel={t("common.cancel")}
    danger={editor.accessConfirmationImpact.removedChannelIds.length > 0}
    onConfirm={() => void editor.save(true)}
    onCancel={() => {
      editor.clearAccessConfirmation();
      void tick().then(() => saveButtonElement?.focus());
    }}
  />
{/if}

{#if lifecycleAction && lifecycleTarget}
  <ConfirmDialog
    title={lifecycleAction === "archive" ? t("settings.chat.teammates.archiveTitle", lifecycleTarget.participant.displayName) : t("settings.chat.teammates.deleteTitle", lifecycleTarget.participant.displayName)}
    message={lifecycleAction === "archive" ? t("settings.chat.teammates.archiveMessage", lifecycleTarget.participant.displayName) : t("settings.chat.teammates.deleteMessage", lifecycleTarget.participant.displayName)}
    confirmLabel={lifecycleAction === "archive" ? t("settings.chat.teammates.archiveConfirm") : t("settings.chat.teammates.deletePermanently")}
    cancelLabel={t("common.cancel")}
    danger={lifecycleAction === "delete"}
    onConfirm={() => void confirmLifecycle()}
    onCancel={() => { lifecycleAction = null; lifecycleTarget = null; }}
  />
{/if}

{#if profileManagerOpen && ChatAccessProfilesManager}
  {@const Manager = ChatAccessProfilesManager}
  <Manager
    profiles={editor.accessProfiles}
    onProfilesChange={editor.handleProfilesChange}
    onClose={() => { profileManagerOpen = false; }}
  />
{/if}

<style>
  .header-actions,.editor-footer,.save-area { display:flex; align-items:center; }
  .header-actions,.save-area { gap:0.45rem; }
  .primary-button,.secondary-button,.danger-button { display:inline-flex; min-height:2rem; align-items:center; justify-content:center; gap:0.38rem; border-radius:0.42rem; padding:0.35rem 0.7rem; font-size:calc(0.733333rem * var(--type-scale)); font-weight:600; }
  .primary-button { background:var(--primary); color:var(--primary-foreground); }
  .secondary-button { border:1px solid var(--border); background:var(--background); color:var(--foreground); }
  .danger-button { background:var(--destructive); color:var(--destructive-foreground); }
  button:hover:not(:disabled) { filter:brightness(0.96); }
  button:disabled { cursor:not-allowed; opacity:0.5; }
  .scroll-frame,.editor-scroll-frame { position:relative; min-height:0; }
  .directory-scroll,.editor-scroll { height:100%; overflow-y:auto; overscroll-behavior:contain; }
  .directory-list { display:grid; align-content:start; gap:0.15rem; padding-right:0.35rem; }
  .directory-row { display:grid; min-width:0; grid-template-columns:auto minmax(0,1fr) auto; align-items:center; gap:0.55rem; border-radius:0.5rem; padding:0.55rem; text-align:left; }
  .directory-row:hover,.directory-row.active { background:var(--accent); color:var(--accent-foreground); }
  .directory-row > span:not(.channel-count) { display:grid; min-width:0; }
  .directory-row strong,.directory-row small { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  .directory-row strong { font-size:calc(0.766667rem * var(--type-scale)); }
  .directory-row small { color:var(--muted-foreground); font-size:calc(0.65rem * var(--type-scale)); }
  .draft-avatar { display:grid; width:1.9rem; height:1.9rem; place-items:center; border:1px dashed var(--border); border-radius:0.45rem; color:var(--muted-foreground); }
  .draft-avatar.large { width:2.4rem; height:2.4rem; }
  .empty-copy { padding:0.7rem; color:var(--muted-foreground); font-size:calc(0.7rem * var(--type-scale)); }
  .detail-panel { min-width:0; min-height:0; }
  .editor { display:grid; height:100%; min-height:0; grid-template-rows:minmax(0,1fr) auto; }
  .editor-heading { display:grid; min-width:0; grid-template-columns:auto minmax(0,1fr); align-items:center; gap:0.65rem; padding-inline:0.25rem; }
  .editor-title { min-width:0; }
  .editor-name-line { display:flex; min-width:0; align-items:center; justify-content:space-between; gap:0.75rem; }
  .editor-heading h3 { overflow:hidden; font-size:calc(0.833333rem * var(--type-scale)); font-weight:600; text-overflow:ellipsis; white-space:nowrap; }
  .editor-heading p { overflow:hidden; margin-top:0.08rem; color:var(--muted-foreground); font-size:calc(0.7rem * var(--type-scale)); text-overflow:ellipsis; white-space:nowrap; }
  .editor-state { display:flex; flex-shrink:0; align-items:center; gap:0.32rem; color:var(--muted-foreground); font-size:calc(0.65rem * var(--type-scale)); white-space:nowrap; }
  .editor-state i { width:0.38rem; height:0.38rem; border-radius:999px; background:var(--status-tentative); }
  .editor-state[data-state="healthy"] i { background:var(--action-confirm); }
  .archived-state { color:var(--muted-foreground); }
  .editor-content { display:grid; align-content:start; gap:1rem; }
  .section-heading h4 { font-size:calc(0.8rem * var(--type-scale)); font-weight:600; }
  .field-grid { display:grid; grid-template-columns:repeat(2,minmax(0,1fr)); gap:0.75rem; }
  .field-grid.compact { grid-template-columns:repeat(2,minmax(0,1fr)); }
  .form-field { display:grid; min-width:0; align-content:start; gap:0.3rem; color:var(--muted-foreground); font-size:calc(0.68rem * var(--type-scale)); font-weight:550; }
  .execution-fields { gap:0.7rem; padding-inline:0.25rem; }
  .required-marker { margin-left:0.15rem; color:var(--destructive); font-style:normal; }
  .execution-model-field :global(.model-control) { z-index:2; max-width:100%; justify-self:start; }
  .execution-model-field :global(.model-trigger) { min-width:12rem; }
  .execution-approval-field :global(.access-control) { justify-self:start; }
  .execution-approval-field :global(.control-trigger) { min-width:12rem; max-width:100%; justify-content:center; }
  .form-field.full { grid-column:1/-1; }
  .form-field input,.form-field textarea { box-sizing:border-box; width:100%; min-width:0; appearance:none; border:1px solid var(--border); border-radius:var(--floating-item-radius); background:transparent; padding:0.47rem 0.55rem; color:var(--foreground); outline:0; font-weight:400; }
  .form-field textarea { resize:vertical; }
  .form-field small { color:var(--muted-foreground); font-weight:400; line-height:1rem; }
  .conflict-panel { display:grid; grid-template-columns:minmax(0,1fr) auto; align-items:start; gap:0.8rem; margin-bottom:1rem; border:1px solid color-mix(in srgb,var(--destructive) 45%,var(--border)); border-radius:0.6rem; background:color-mix(in srgb,var(--destructive) 7%,var(--background)); padding:0.8rem; outline:0; }
  .conflict-notice { margin-bottom:1rem; border-radius:0.5rem; outline:0; font-size:calc(0.7rem * var(--type-scale)); }
  .conflict-panel:focus-visible,.conflict-notice:focus-visible,.conflict-recovery-notice:focus-visible { box-shadow:0 0 0 2px color-mix(in srgb,var(--primary) 55%,transparent); }
  .conflict-copy { min-width:0; }
  .conflict-panel strong { font-size:calc(0.75rem * var(--type-scale)); }
  .conflict-panel p,.conflict-panel li { color:var(--muted-foreground); font-size:calc(0.68rem * var(--type-scale)); line-height:1rem; }
  .conflict-panel p { margin-top:0.18rem; }
  .conflict-panel ul { display:grid; gap:0.1rem; margin-top:0.45rem; padding-left:1rem; }
  .conflict-actions { display:flex; flex-wrap:wrap; justify-content:flex-end; gap:0.4rem; }
  .conflict-recovery-notice { margin-bottom:1rem; border:1px solid color-mix(in srgb,var(--action-confirm) 45%,var(--border)); border-radius:0.5rem; background:color-mix(in srgb,var(--action-confirm) 8%,transparent); padding:0.6rem 0.7rem; color:var(--foreground); font-size:calc(0.68rem * var(--type-scale)); outline:0; }
  .access-details { display:grid; gap:0.75rem; padding:0; }
  .history-field { width:min(14rem,100%); }
  fieldset { display:grid; gap:0.4rem; }
  fieldset legend { margin-bottom:0.15rem; font-size:calc(0.7rem * var(--type-scale)); font-weight:650; }
  .capability-switches { grid-template-columns:repeat(2,minmax(0,1fr)); }
  .capability-switches legend { grid-column:1/-1; }
  .capability-switches span { display:grid; }
  .capability-switches strong { font-size:calc(0.68rem * var(--type-scale)); }
  .capability-switches small { margin-top:0.12rem; color:var(--muted-foreground); font-size:calc(0.62rem * var(--type-scale)); line-height:0.9rem; }
  .folder-row { display:grid; min-height:2.65rem; grid-template-columns:auto auto minmax(8rem,1fr) auto; align-items:center; gap:0.5rem; padding:0.35rem 0.2rem; }
  .folder-controls { display:flex; flex-wrap:wrap; align-items:center; justify-content:flex-end; gap:0.4rem; }
  .folder-name { display:grid; min-width:0; }
  .folder-name strong { overflow:hidden; font-size:calc(0.68rem * var(--type-scale)); text-overflow:ellipsis; white-space:nowrap; }
  .folder-name small { color:var(--muted-foreground); font-size:calc(0.6rem * var(--type-scale)); }
  .folder-name small[data-status="available"] { color:var(--action-confirm); }
  .default-target { display:flex; align-items:center; gap:0.25rem; color:var(--muted-foreground); font-size:calc(0.6rem * var(--type-scale)); }
  .link-button { color:var(--primary); font-size:calc(0.64rem * var(--type-scale)); font-weight:600; }
  .editor-footer { justify-content:space-between; gap:0.75rem; border-top:1px solid var(--border); padding:0.65rem 0.85rem; }
  .editor-footer > div { display:flex; min-width:0; align-items:center; gap:0.5rem; }
  .field-error { color:var(--destructive); font-size:calc(0.65rem * var(--type-scale)); }
  .empty-detail { display:grid; height:100%; place-items:center; align-content:center; gap:0.5rem; color:var(--muted-foreground); font-size:calc(0.73rem * var(--type-scale)); }
  @media (max-width:900px) { .field-grid.compact { grid-template-columns:repeat(2,minmax(0,1fr)); }.folder-row { grid-template-columns:auto auto minmax(7rem,1fr); }.folder-controls { grid-column:3; justify-content:flex-start; } }
  @media (max-width:700px) { .directory-panel { border-right:0; border-bottom:1px solid var(--border); }.directory-list { grid-template-columns:repeat(auto-fill,minmax(11rem,1fr)); }.field-grid,.field-grid.compact,.capability-switches { grid-template-columns:1fr; }.capability-switches legend { grid-column:auto; }.conflict-panel { grid-template-columns:1fr; }.conflict-actions { justify-content:stretch; }.conflict-actions button { flex:1; }.editor-footer { align-items:stretch; flex-direction:column; }.editor-footer > div,.save-area { justify-content:space-between; }.save-area .primary-button { flex:1; } }
  @media (pointer:coarse) { .conflict-actions button { min-height:44px; } }

  .teammate-settings { display:grid; height:100%; min-height:0; grid-template-rows:auto minmax(0,1fr); gap:0.8rem; }
  .directory-header { display:flex; flex-wrap:wrap; align-items:start; justify-content:space-between; gap:0.75rem; padding-inline:0.25rem; }
  .directory-header h2 { font-size:calc(0.866667rem * var(--type-scale)); font-weight:600; }
  .header-actions { display:flex; align-items:center; gap:0.35rem; }
  .settings-button { display:inline-flex; min-height:1.9rem; align-items:center; justify-content:center; gap:0.35rem; border-radius:0.42rem; padding:0.3rem 0.65rem; font-size:calc(0.733333rem * var(--type-scale)); font-weight:600; line-height:1; white-space:nowrap; }
  .settings-button { border:1px solid var(--border); background:var(--background); color:var(--foreground); }
  .archive-filter { position:relative; display:grid; width:1.9rem; height:1.9rem; place-items:center; border-radius:0.42rem; color:var(--muted-foreground); }
  .archive-filter:hover,.archive-filter[aria-pressed="true"] { background:var(--accent); color:var(--foreground); }
  .archive-filter > span { position:absolute; right:0.08rem; bottom:0.08rem; display:grid; width:0.72rem; height:0.72rem; place-items:center; border-radius:50%; background:var(--background); }
  .directory-layout { display:grid; min-height:0; isolation:isolate; grid-template-columns:minmax(12.5rem,0.62fr) minmax(0,1.6fr); overflow:visible; border:0; border-radius:0; background:transparent; }
  .directory-panel { position:relative; z-index:1; display:grid; min-width:0; min-height:0; grid-template-rows:auto minmax(0,1fr); gap:0.45rem; border:0; padding:0 0.75rem 0 0; }
  .directory-search { display:flex; height:1.9rem; align-items:center; gap:0.35rem; border-bottom:1px solid var(--border); padding-inline:0.35rem; color:var(--muted-foreground); }
  .directory-search input { width:100%; min-width:0; background:transparent; color:var(--foreground); outline:0; font-size:calc(0.733333rem * var(--type-scale)); }
  .directory-list { gap:0; padding:0.15rem 0.2rem 0.15rem 0; }
  .directory-row { grid-template-columns:auto minmax(0,1fr); margin-inline:0.2rem; padding:0.6rem; }
  .directory-row:hover,.directory-row.active { background:var(--accent); color:var(--accent-foreground); }
  .directory-avatar { position:relative; display:grid; }
  .directory-summary { display:grid; min-width:0; }
  .directory-summary strong,.directory-summary small { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  .directory-summary strong { font-size:calc(0.8rem * var(--type-scale)); font-weight:600; }
  .directory-summary small { margin-top:0.05rem; color:var(--muted-foreground); font-size:calc(0.68rem * var(--type-scale)); }
  .detail-panel { position:relative; z-index:2; min-width:0; min-height:0; border-left:1px solid var(--border); padding-left:1rem; }
  .teammate-editor { grid-template-rows:minmax(0,1fr) auto; }
  .editor-scroll { display:grid; align-content:start; gap:1rem; padding:0.2rem 0.75rem 1rem 0; }
  .editor-section { display:grid; gap:0.75rem; border:0; padding:0; }
  .access-section { gap:0.7rem; }
  .access-section-heading { display:flex; min-width:0; align-items:center; gap:0.5rem; }
  .access-configure-button { display:inline-flex; min-height:1.25rem; align-items:center; justify-content:center; gap:0.22rem; padding:0.05rem 0.1rem; color:var(--muted-foreground); font-size:calc(0.666667rem * var(--type-scale)); font-weight:600; white-space:nowrap; }
  .access-configure-button:hover:not(:disabled),.access-configure-button[aria-expanded="true"] { color:var(--foreground); filter:none; }
  .access-summary-tree { display:grid; gap:0.8rem; padding-right:0.25rem; }
  .access-summary-group { display:grid; gap:0.45rem; }
  .access-summary-group > h5 { min-width:0; overflow:hidden; color:var(--foreground); font-size:calc(0.716667rem * var(--type-scale)); font-weight:650; text-overflow:ellipsis; white-space:nowrap; }
  .access-summary-project { display:grid; gap:0.05rem; padding-left:0.55rem; }
  .access-summary-project > h6 { min-width:0; overflow:hidden; margin-bottom:0.18rem; color:var(--foreground); font-size:calc(0.656667rem * var(--type-scale)); font-weight:600; text-overflow:ellipsis; white-space:nowrap; }
  .access-summary-channel { display:grid; min-width:0; }
  .access-summary-row { display:grid; min-width:0; min-height:1.7rem; grid-template-columns:minmax(6rem,1fr) minmax(0,8.75rem) minmax(0,8.75rem); align-items:center; gap:0.28rem; }
  .access-channel-toggle { display:flex; min-width:0; height:1.55rem; align-items:center; gap:0.28rem; overflow:hidden; border-radius:0.35rem; color:var(--muted-foreground); text-align:left; }
  .access-channel-toggle:hover { color:var(--foreground); }
  .access-channel-toggle :global(svg) { flex:0 0 auto; transition:transform 120ms ease; }
  .access-channel-toggle :global(svg.expanded) { transform:rotate(90deg); }
  .access-channel-toggle strong,.access-channel-name { min-width:0; overflow:hidden; color:inherit; font-size:calc(0.696667rem * var(--type-scale)); font-weight:600; text-overflow:ellipsis; white-space:nowrap; }
  .access-channel-name { display:block; padding-left:1.03rem; color:var(--muted-foreground); }
  .channel-primary-control { min-width:0; }
  .channel-primary-control :global(.scope-controls) { width:100%; justify-content:stretch; }
  .channel-primary-control :global(.scope-control) { width:100%; }
  .access-summary-row .channel-primary-control :global(.control-trigger) { width:100%; height:1.55rem; max-width:none; justify-content:flex-start; gap:0.25rem; border:1px solid color-mix(in srgb,var(--border) 75%,transparent); border-radius:0.35rem; background:transparent; padding:0.12rem 0.35rem; color:var(--muted-foreground); font-size:calc(0.636667rem * var(--type-scale)); font-weight:550; }
  .access-summary-row .channel-primary-control :global(.control-trigger:hover),.access-summary-row .channel-primary-control :global(.control-trigger[aria-expanded="true"]) { background:var(--accent); color:var(--foreground); }
  .access-summary-row .channel-primary-control :global(.control-trigger svg) { width:0.7rem; height:0.7rem; }
  .access-summary-details { padding:0.55rem 0 0.7rem 1.03rem; }
  .access-summary-empty { padding-inline:0.25rem; color:var(--muted-foreground); font-size:calc(0.66rem * var(--type-scale)); }
  .access-details { border:0; }
  .capability-switches { gap:0; }
  .capability-switches > div { display:grid; grid-template-columns:auto minmax(0,1fr); gap:0.5rem; border:0; border-bottom:1px solid color-mix(in srgb,var(--border) 55%,transparent); border-radius:0; padding:0.55rem 0.15rem; }
  .folder-row { grid-template-columns:auto auto minmax(7rem,1fr) auto; }
  .disclosure-button { display:inline-flex; width:max-content; align-items:center; gap:0.3rem; color:var(--muted-foreground); font-size:calc(0.68rem * var(--type-scale)); font-weight:600; }
  .disclosure-button:hover { color:var(--foreground); }
  .disclosure-button :global(svg) { transition:transform 120ms ease; }
  .disclosure-button :global(svg.expanded) { transform:rotate(90deg); }
  .advanced-fields { width:min(30rem,100%); }
  .editor-footer { padding:0.65rem 0.75rem 0.65rem 0; }
  .editor-footer button { white-space:nowrap; }
  .primary-button,.secondary-button,.danger-button { white-space:nowrap; }
  .teammate-settings input[type="radio"] { appearance:none; width:0.9rem; height:0.9rem; border:1px solid var(--border); border-radius:50%; background:var(--background); }
  .teammate-settings input[type="radio"]:checked { border:0.25rem solid var(--primary); }
  @media (max-width:700px) { .teammate-settings { grid-template-rows:auto minmax(0,1fr); }.directory-layout { grid-template-columns:1fr; grid-template-rows:minmax(7rem,28%) minmax(0,1fr); }.directory-panel { border-bottom:1px solid var(--border); padding:0 0 0.75rem; }.detail-panel { border-top:0; border-left:0; padding:0.8rem 0 0; }.directory-list { grid-template-columns:repeat(auto-fill,minmax(11rem,1fr)); }.field-grid,.field-grid.compact,.capability-switches { grid-template-columns:1fr; }.folder-row { grid-template-columns:auto auto minmax(7rem,1fr); }.folder-controls { grid-column:3; justify-content:flex-start; }.editor-footer { padding-right:0; } }
</style>
