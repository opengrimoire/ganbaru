import * as chatApi from "$lib/api/chat";
import {
  copyModelOptionSelections,
  copyVersionedJson,
  resolveDefaultProviderModel,
} from "$lib/chat/composer/model";
import type {
  ChatAccessProfileRead,
  ChatAiTeammateRead,
  ChatChannelRead,
  ChatFolderCapability,
  ChatRuntimeApprovalPolicy,
  ChatTeammatePolicyInput,
  ChatTeammateAccessRead,
  ChatTeammateChannelAccessInput,
  ModelOptionSelection,
  ReplaceChatTeammateAccessRequest,
  SafetyMode,
  VersionedJson,
} from "$lib/chat/contracts";
import { chatErrorCode, chatErrorField, chatErrorMessage } from "$lib/chat/error-presentation";
import {
  teammateAccessConfirmationImpact,
  teammateAccessDraftErrors,
  teammateAccessDraftSnapshot,
  teammateAccessNeedsConfirmation,
  type ChatTeammateAccessConfirmationImpact,
} from "./access";
import { conversationAccessProfile, defaultChannelAccessInput } from "./channel-membership";
import { teammateExecutionSummary, teammateProfileDraftSnapshot } from "./draft";
import {
  cloneChatTeammateStudioDraft,
  compareChatTeammateStudioDrafts,
  rebaseChatTeammateStudioDraft,
  type ChatTeammateStudioDraft,
} from "./access-conflict";
import type { Translate } from "$lib/i18n/translator.svelte";
import type { getChat } from "$lib/stores/chat.svelte";

export interface TeammateEditorOptions {
  chat: Pick<
    ReturnType<typeof getChat>,
    "teammates" | "archivedTeammates" | "settings" | "refreshTeammates"
  >;
  t: Translate;
  initialChannelId: () => string | null;
  closeAccessPicker: () => void;
  setExpandedAccessChannel: (channelId: string | null) => void;
  focusConflict: () => Promise<void>;
  focusRecoveryNotice: () => void;
}

type ConflictRecoveryNotice = "rebased" | "reloaded";

interface TeammateAccessConflictState {
  teammateId: string;
  localDraft: ChatTeammateStudioDraft;
  baselineDraft: ChatTeammateStudioDraft;
  durableDraft: ChatTeammateStudioDraft;
  durableTeammate: ChatAiTeammateRead;
  durableAccess: ChatTeammateAccessRead;
}

/** Owns one mounted teammate editor's drafts, revision checks, and save/conflict workflow. */
export function createTeammateEditorController(options: TeammateEditorOptions) {
  const { chat, t } = options;
  let selectedId = $state<string | null>(null);
  let creating = $state(false);
  let showArchived = $state(false);
  let archivedTeammates = $state<ChatAiTeammateRead[]>([]);
  let navigationChannels = $state<ChatChannelRead[]>([]);
  let accessProfiles = $state<ChatAccessProfileRead[]>([]);
  let loadingDirectory = $state(true);
  let loadingAccess = $state(false);
  let saving = $state(false);
  let error = $state<string | null>(null);
  let errorField = $state<string | null>(null);
  let displayName = $state("");
  let role = $state("");
  let instructions = $state("");
  let providerId = $state("");
  let safetyMode = $state<SafetyMode>("ask_for_approval");
  let modelId = $state("");
  let providerManagedModel = $state(false);
  let modelOptions = $state<ModelOptionSelection[]>([]);
  let effort = $state<string | null>(null);
  let speed = $state<string | null>(null);
  let providerOptions = $state<VersionedJson>({ schemaVersion: 1, value: {} });
  let teammateDefaultRuntimeApproval = $state<ChatRuntimeApprovalPolicy>("ask");
  let profileExpectedRevision = $state(0);
  let profileAvatar = $state<VersionedJson>({ schemaVersion: 1, value: { kind: "initials" } });
  let accessRevision = $state(0);
  let accessDraft = $state<ChatTeammateChannelAccessInput[]>([]);
  let profileBaseline = $state<string | null>(null);
  let accessBaseline = $state<string | null>(null);
  let studioDraftBaseline = $state<ChatTeammateStudioDraft | null>(null);
  let accessConfirmationImpact = $state<ChatTeammateAccessConfirmationImpact | null>(null);
  let accessConfirmationSnapshot = $state<string | null>(null);
  let accessConflict = $state<TeammateAccessConflictState | null>(null);
  let conflictPendingDraft = $state<ChatTeammateStudioDraft | null>(null);
  let conflictLoading = $state(false);
  let conflictError = $state<string | null>(null);
  let conflictRecoveryNotice = $state<ConflictRecoveryNotice | null>(null);

  let accessLoadRequest = 0;
  let conflictLoadRequest = 0;
  let lifecycleError = $state<string | null>(null);
  let preserveStudioDraftForId: string | null = null;

  const allDirectoryTeammates = $derived(showArchived
    ? [...chat.teammates, ...archivedTeammates]
    : chat.teammates);
  const allTeammates = $derived([...chat.teammates, ...archivedTeammates]);
  const selected = $derived(allDirectoryTeammates.find((entry) => entry.participant.id === selectedId) ?? null);
  const archivedMode = $derived(Boolean(selected?.participant.archivedAt));
  const providers = $derived(chat.settings?.providerInstances ?? []);
  const selectedProvider = $derived(providers.find((entry) => entry.configuration.instanceId === providerId) ?? null);
  const providerAuthoritySupport = $derived(selectedProvider?.lastProbe?.authoritySupport ?? null);
  const availableModels = $derived(selectedProvider?.modelCatalog?.models.filter((model) => model.availability !== "deprecated") ?? []);
  const selectedModel = $derived(availableModels.find((model) => model.id === modelId) ?? null);
  const modelSelectionValid = $derived(providerManagedModel !== Boolean(modelId));
  const normalizedDisplayName = $derived(displayName.trim().toLocaleLowerCase());
  const nameTaken = $derived(Boolean(normalizedDisplayName && allTeammates.some((teammate) => (
    teammate.participant.id !== selectedId
      && teammate.participant.displayName.trim().toLocaleLowerCase() === normalizedDisplayName
  ))));
  const profileCeilings = $derived(new Map(accessProfiles.map((profile) => [
    profile.id,
    profile.latestRevision.maximumFolderCapability,
  ])));
  const accessErrors = $derived(teammateAccessDraftErrors(accessDraft, profileCeilings));
  const providerResourceBlockers = $derived(accessDraft
    .flatMap(providerResourceIssuesForChannel)
    .filter((issue, index, issues) => issues.indexOf(issue) === index));
  const currentProfileSnapshot = $derived(teammateProfileDraftSnapshot({
    displayName,
    role,
    instructions,
    providerId,
    safetyMode,
    modelId,
    providerManagedModel,
    modelOptions,
    effort,
    speed,
  }));
  const currentAccessSnapshot = $derived(JSON.stringify({
    teammateDefaultRuntimeApproval,
    channels: teammateAccessDraftSnapshot(accessDraft),
  }));
  const currentDraftSnapshot = $derived(JSON.stringify({
    profile: currentProfileSnapshot,
    providerOptions: JSON.stringify(providerOptions),
    access: currentAccessSnapshot,
  }));
  const conflictComparison = $derived(accessConflict
    ? compareChatTeammateStudioDrafts(captureStudioDraft(), accessConflict.durableDraft)
    : null);
  const profileDirty = $derived(profileBaseline !== null && currentProfileSnapshot !== profileBaseline);
  const accessDirty = $derived(accessBaseline !== null && currentAccessSnapshot !== accessBaseline);
  const dirty = $derived(profileDirty || accessDirty);
  const canSave = $derived(Boolean(
    displayName.trim()
      && role.trim()
      && providerId
      && modelSelectionValid
      && !nameTaken
      && accessErrors.length === 0
      && providerResourceBlockers.length === 0
      && dirty
      && !loadingAccess
      && !accessConflict
      && !conflictLoading
      && !conflictError
      && !archivedMode,
  ));

  /** Retains successful directory reads even when another independent read fails. */
  async function loadDirectoryData(): Promise<void> {
    loadingDirectory = true;
    error = null;
    const [archivedResult, channelResult, profileResult] = await Promise.allSettled([
      chatApi.listChatTeammates(true),
      chatApi.listChatNavigationChannels(),
      chatApi.listChatAccessProfiles(false),
    ]);
    if (archivedResult.status === "fulfilled") {
      archivedTeammates = archivedResult.value;
      chat.archivedTeammates = archivedResult.value;
    }
    if (channelResult.status === "fulfilled") navigationChannels = channelResult.value;
    if (profileResult.status === "fulfilled") accessProfiles = profileResult.value;
    const failure = [archivedResult, channelResult, profileResult]
      .find((result) => result.status === "rejected");
    if (failure?.status === "rejected") {
      error = chatErrorMessage(failure.reason, t("settings.chat.teammates.loadFailed"));
    }
    loadingDirectory = false;
  }

  function captureStudioDraft(): ChatTeammateStudioDraft {
    return {
      profile: {
        displayName,
        role,
        instructions,
        providerId,
        safetyMode,
        modelId,
        providerManagedModel,
        modelOptions: copyModelOptionSelections(modelOptions),
        effort,
        speed,
        providerOptions: copyVersionedJson(providerOptions),
      },
      teammateDefaultRuntimeApproval,
      channels: copyChannelAccessInputs(accessDraft),
    };
  }

  function draftFromReads(
    teammate: ChatAiTeammateRead,
    access: ChatTeammateAccessRead,
  ): ChatTeammateStudioDraft {
    const policy = teammate.latestPolicy;
    const options = copyModelOptionSelections(policy?.modelOptions ?? []);
    const provider = providers.find((entry) => (
      entry.configuration.instanceId === policy?.providerInstanceId
    )) ?? null;
    const model = provider?.modelCatalog?.models.find((entry) => entry.id === policy?.modelId) ?? null;
    const summary = teammateExecutionSummary(options, model);
    return {
      profile: {
        displayName: teammate.participant.displayName,
        role: teammate.role,
        instructions: teammate.instructions,
        providerId: policy?.providerInstanceId ?? "",
        safetyMode: policy?.safetyMode ?? "ask_for_approval",
        modelId: policy?.modelId ?? "",
        providerManagedModel: policy?.providerManagedModel ?? false,
        modelOptions: options,
        effort: summary.effort ?? policy?.effort ?? null,
        speed: summary.speed ?? policy?.speed ?? null,
        providerOptions: copyVersionedJson(
          policy?.providerOptions ?? { schemaVersion: 1, value: {} },
        ),
      },
      teammateDefaultRuntimeApproval: access.teammateDefaultRuntimeApproval,
      channels: access.channels
        .filter((channel) => channel.removedAt === null)
        .map((channel) => ({
          channelId: channel.channelId,
          accessProfileId: channel.accessProfileId,
          accessProfileRevision: channel.accessProfileRevision,
          capabilities: { ...channel.capabilities },
          historyBoundary: channel.historyBoundary.kind === "entire"
            ? { kind: "entire" as const }
            : { kind: "fromGrant" as const },
          runtimeApprovalOverride: channel.runtimeApprovalOverride,
          scratchRuntimeApprovalOverride: channel.scratchRuntimeApprovalOverride,
          folderGrants: channel.folderGrants
            .filter((grant) => grant.revokedAt === null)
            .map((grant) => ({
              workingFolderId: grant.workingFolderId,
              capability: grant.capability,
              isDefault: grant.isDefault,
              runtimeApprovalOverride: grant.runtimeApprovalOverride,
            })),
        })),
    };
  }

  function applyStudioDraft(draft: ChatTeammateStudioDraft): void {
    const copy = cloneChatTeammateStudioDraft(draft);
    displayName = copy.profile.displayName;
    role = copy.profile.role;
    instructions = copy.profile.instructions;
    providerId = copy.profile.providerId;
    safetyMode = copy.profile.safetyMode;
    modelId = copy.profile.modelId;
    providerManagedModel = copy.profile.providerManagedModel;
    modelOptions = copy.profile.modelOptions;
    effort = copy.profile.effort;
    speed = copy.profile.speed;
    providerOptions = copy.profile.providerOptions;
    teammateDefaultRuntimeApproval = copy.teammateDefaultRuntimeApproval;
    accessDraft = copy.channels;
  }

  function setStudioDraftBaseline(draft: ChatTeammateStudioDraft): void {
    const copy = cloneChatTeammateStudioDraft(draft);
    studioDraftBaseline = copy;
    profileBaseline = profileSnapshotForDraft(copy);
    accessBaseline = accessSnapshotForDraft(copy);
  }

  function profileSnapshotForDraft(draft: ChatTeammateStudioDraft): string {
    return teammateProfileDraftSnapshot(draft.profile);
  }

  function accessSnapshotForDraft(draft: ChatTeammateStudioDraft): string {
    return JSON.stringify({
      teammateDefaultRuntimeApproval: draft.teammateDefaultRuntimeApproval,
      channels: teammateAccessDraftSnapshot(draft.channels),
    });
  }

  function draftSnapshotForDraft(draft: ChatTeammateStudioDraft): string {
    return JSON.stringify({
      profile: profileSnapshotForDraft(draft),
      providerOptions: JSON.stringify(draft.profile.providerOptions),
      access: accessSnapshotForDraft(draft),
    });
  }

  function copyChannelAccessInputs(
    channels: readonly ChatTeammateChannelAccessInput[],
  ): ChatTeammateChannelAccessInput[] {
    return channels.map((channel) => ({
      ...channel,
      capabilities: { ...channel.capabilities },
      historyBoundary: channel.historyBoundary.kind === "entire"
        ? { kind: "entire" }
        : { kind: "fromGrant", lowerOrdinal: channel.historyBoundary.lowerOrdinal },
      folderGrants: channel.folderGrants.map((grant) => ({ ...grant })),
    }));
  }

  function clearAccessConflict(): void {
    conflictLoadRequest += 1;
    accessConflict = null;
    conflictPendingDraft = null;
    conflictLoading = false;
    conflictError = null;
    conflictRecoveryNotice = null;
  }

  function clearAccessConfirmation(): void {
    accessConfirmationImpact = null;
    accessConfirmationSnapshot = null;
  }

  /** Starts a local draft; creation stays inert until access is explicitly saved. */
  function beginCreate(channelId: string | null = null): void {
    clearAccessConflict();
    lifecycleError = null;
    options.closeAccessPicker();
    creating = true;
    selectedId = null;
    options.setExpandedAccessChannel(channelId);
    displayName = "";
    role = "";
    instructions = "";
    const resolved = resolveDefaultProviderModel(providers);
    providerId = resolved?.provider.configuration.instanceId ?? "";
    safetyMode = "ask_for_approval";
    modelId = resolved?.model?.id ?? "";
    providerManagedModel = resolved?.providerManaged ?? false;
    modelOptions = copyModelOptionSelections(resolved?.options ?? []);
    const summary = teammateExecutionSummary(modelOptions, resolved?.model ?? null);
    effort = summary.effort;
    speed = summary.speed;
    providerOptions = { schemaVersion: 1, value: {} };
    teammateDefaultRuntimeApproval = "ask";
    profileExpectedRevision = 0;
    profileAvatar = { schemaVersion: 1, value: { kind: "initials" } };
    accessRevision = 0;
    accessDraft = channelId ? [defaultChannelAccess(channelId)] : [];
    setStudioDraftBaseline(captureStudioDraft());
    clearAccessConfirmation();
    error = null;
    errorField = null;
  }

  function cancelCreate(): void {
    clearAccessConflict();
    lifecycleError = null;
    creating = false;
    selectedId = chat.teammates[0]?.participant.id ?? null;
    profileBaseline = null;
    accessBaseline = null;
    studioDraftBaseline = null;
    clearAccessConfirmation();
    options.closeAccessPicker();
  }

  /** Loads the selected teammate without resetting a draft preserved across this editor's own refresh. */
  async function initializeSelectedTeammate(teammate: ChatAiTeammateRead): Promise<void> {
    if (preserveStudioDraftForId === teammate.participant.id) {
      preserveStudioDraftForId = null;
      return;
    }
    clearAccessConflict();
    lifecycleError = null;
    options.closeAccessPicker();
    profileBaseline = null;
    accessBaseline = null;
    studioDraftBaseline = null;
    clearAccessConfirmation();
    error = null;
    errorField = null;
    accessDraft = [];
    options.setExpandedAccessChannel(null);
    displayName = teammate.participant.displayName;
    role = teammate.role;
    instructions = teammate.instructions;
    providerId = teammate.latestPolicy?.providerInstanceId ?? "";
    safetyMode = teammate.latestPolicy?.safetyMode ?? "ask_for_approval";
    modelId = teammate.latestPolicy?.modelId ?? "";
    providerManagedModel = teammate.latestPolicy?.providerManagedModel ?? false;
    modelOptions = copyModelOptionSelections(teammate.latestPolicy?.modelOptions ?? []);
    const summary = teammateExecutionSummary(modelOptions, selectedModel);
    effort = summary.effort ?? teammate.latestPolicy?.effort ?? null;
    speed = summary.speed ?? teammate.latestPolicy?.speed ?? null;
    providerOptions = copyVersionedJson(
      teammate.latestPolicy?.providerOptions ?? { schemaVersion: 1, value: {} },
    );
    profileExpectedRevision = teammate.participant.revision;
    profileAvatar = copyVersionedJson(teammate.participant.avatar);
    profileBaseline = currentProfileSnapshot;
    await loadTeammateAccess(teammate.participant.id);
  }

  async function loadTeammateAccess(teammateId: string): Promise<void> {
    const request = ++accessLoadRequest;
    loadingAccess = true;
    try {
      const access = await chatApi.readChatTeammateAccess(teammateId);
      if (request !== accessLoadRequest || selectedId !== teammateId) return;
      const teammate = allDirectoryTeammates.find((entry) => entry.participant.id === teammateId);
      if (!teammate) return;
      const durableDraft = draftFromReads(teammate, access);
      accessRevision = access.accessRevision;
      profileExpectedRevision = teammate.participant.revision;
      profileAvatar = copyVersionedJson(teammate.participant.avatar);
      teammateDefaultRuntimeApproval = durableDraft.teammateDefaultRuntimeApproval;
      accessDraft = copyChannelAccessInputs(durableDraft.channels);
      const initialChannelId = options.initialChannelId();
      options.setExpandedAccessChannel(initialChannelId);
      setStudioDraftBaseline(durableDraft);
      if (initialChannelId) {
        if (!accessDraft.some((channel) => channel.channelId === initialChannelId)) {
          accessDraft = [...accessDraft, defaultChannelAccess(initialChannelId)];
        }
      }
    } catch (cause: unknown) {
      if (request === accessLoadRequest) {
        error = chatErrorMessage(cause, t("settings.chat.teammates.accessLoadFailed"));
      }
    } finally {
      if (request === accessLoadRequest) loadingAccess = false;
    }
  }

  async function loadAccessConflict(
    teammateId: string,
    localDraft: ChatTeammateStudioDraft,
  ): Promise<void> {
    const request = ++conflictLoadRequest;
    const retainedLocalDraft = cloneChatTeammateStudioDraft(localDraft);
    const retainedBaseline = cloneChatTeammateStudioDraft(
      studioDraftBaseline ?? retainedLocalDraft,
    );
    conflictPendingDraft = retainedLocalDraft;
    accessConflict = null;
    conflictError = null;
    conflictRecoveryNotice = null;
    clearAccessConfirmation();
    conflictLoading = true;
    try {
      const [durableTeammate, durableAccess] = await Promise.all([
        chatApi.readChatTeammate(teammateId),
        chatApi.readChatTeammateAccess(teammateId),
      ]);
      if (request !== conflictLoadRequest || selectedId !== teammateId) return;
      accessConflict = {
        teammateId,
        localDraft: retainedLocalDraft,
        baselineDraft: retainedBaseline,
        durableDraft: draftFromReads(durableTeammate, durableAccess),
        durableTeammate,
        durableAccess,
      };
    } catch (cause: unknown) {
      if (request !== conflictLoadRequest) return;
      conflictError = chatErrorMessage(
        cause,
        t("settings.chat.teammates.conflict.loadFailed"),
      );
    } finally {
      if (request === conflictLoadRequest) {
        conflictLoading = false;
        await options.focusConflict();
      }
    }
  }

  function retryAccessConflict(): void {
    if (!selectedId || !conflictPendingDraft || conflictLoading) return;
    void loadAccessConflict(selectedId, conflictPendingDraft);
  }

  /** Replays local edits over current durable state and adopts its expected revisions. */
  function rebaseAccessConflict(): void {
    const conflict = accessConflict;
    if (!conflict) return;
    const rebased = rebaseChatTeammateStudioDraft(
      conflict.baselineDraft,
      captureStudioDraft(),
      conflict.durableDraft,
    );
    applyStudioDraft(rebased);
    accessRevision = conflict.durableAccess.accessRevision;
    profileExpectedRevision = conflict.durableTeammate.participant.revision;
    profileAvatar = copyVersionedJson(conflict.durableTeammate.participant.avatar);
    setStudioDraftBaseline(conflict.durableDraft);
    settleAccessConflict("rebased");
  }

  async function reloadCurrentAccessConflict(): Promise<void> {
    const conflict = accessConflict;
    if (!conflict) return;
    applyStudioDraft(conflict.durableDraft);
    accessRevision = conflict.durableAccess.accessRevision;
    profileExpectedRevision = conflict.durableTeammate.participant.revision;
    profileAvatar = copyVersionedJson(conflict.durableTeammate.participant.avatar);
    setStudioDraftBaseline(conflict.durableDraft);
    preserveStudioDraftForId = conflict.teammateId;
    settleAccessConflict("reloaded");
    try {
      await chat.refreshTeammates();
    } catch (cause: unknown) {
      error = chatErrorMessage(cause, t("settings.chat.teammates.loadFailed"));
    }
  }

  function settleAccessConflict(notice: ConflictRecoveryNotice): void {
    conflictLoadRequest += 1;
    accessConflict = null;
    conflictPendingDraft = null;
    conflictLoading = false;
    conflictError = null;
    conflictRecoveryNotice = notice;
    clearAccessConfirmation();
    error = null;
    errorField = null;
    options.focusRecoveryNotice();
  }

  function conversationProfile(): ChatAccessProfileRead | null {
    return conversationAccessProfile(accessProfiles);
  }

  function providerCapabilityIssue(capability: ChatFolderCapability): string | null {
    if (capability === "none") return null;
    const support = providerAuthoritySupport;
    if (!support) return t("settings.chat.teammates.providerAuthorityUnknown");
    if (capability === "read" && !support.internalHostTools && !support.readOnlyRoot) {
      return t("settings.chat.teammates.providerCannotRead");
    }
    if (capability === "edit" && !support.internalHostTools && !support.writableRoot) {
      return t("settings.chat.teammates.providerCannotEdit");
    }
    if (capability === "execute" && (
      !support.writableRoot
      || !support.confinedCommands
      || !support.networkBoundary
    )) {
      return t("settings.chat.teammates.providerCannotExecute");
    }
    if (capability === "publish" && (
      !support.writableRoot
      || !support.confinedCommands
      || !support.networkBoundary
      || !support.classifiedPublish
    )) {
      return t("settings.chat.teammates.providerCannotPublish");
    }
    return null;
  }

  function providerGrantIssue(
    capability: ChatFolderCapability,
    isDefault: boolean,
  ): string | null {
    const issue = providerCapabilityIssue(capability);
    if (issue) return issue;
    const support = providerAuthoritySupport;
    if (!isDefault || !support) return null;
    if (capability === "read" && (!support.readOnlyRoot || !support.denyShell)) {
      return t("settings.chat.teammates.providerCannotTargetReadOnly");
    }
    if (capability === "edit" && (!support.writableRoot || !support.denyShell)) {
      return t("settings.chat.teammates.providerCannotTargetWritable");
    }
    return null;
  }

  function providerResourceIssuesForChannel(channel: ChatTeammateChannelAccessInput): string[] {
    const grants = channel.folderGrants.filter((grant) => grant.capability !== "none");
    const issues = grants.flatMap((grant) => {
      const issue = providerGrantIssue(grant.capability, grant.isDefault);
      return issue ? [issue] : [];
    });
    if (grants.some((grant) => !grant.isDefault) && !providerAuthoritySupport?.internalHostTools) {
      issues.push(t("settings.chat.teammates.providerNeedsHostTools"));
    }
    return issues.filter((issue, index) => issues.indexOf(issue) === index);
  }

  function defaultChannelAccess(channelId: string): ChatTeammateChannelAccessInput {
    return defaultChannelAccessInput(channelId, conversationProfile());
  }

  function handleProfilesChange(nextProfiles: ChatAccessProfileRead[]): void {
    accessProfiles = nextProfiles;
    const teammateId = selected?.participant.id;
    if (teammateId) void loadTeammateAccess(teammateId);
  }

  function selectExecution(selection: {
    providerInstanceId: string | null;
    modelId: string | null;
    providerManaged: boolean;
    options: ModelOptionSelection[];
  }): void {
    providerId = selection.providerInstanceId ?? "";
    modelId = selection.modelId ?? "";
    providerManagedModel = selection.providerManaged;
    modelOptions = copyModelOptionSelections(selection.options);
    const provider = providers.find((entry) => entry.configuration.instanceId === providerId) ?? null;
    const model = provider?.modelCatalog?.models.find((entry) => entry.id === modelId) ?? null;
    const summary = teammateExecutionSummary(modelOptions, model);
    effort = summary.effort;
    speed = summary.speed;
  }

  function accessReplacementRequest(
    teammateId: string,
    policy: ChatTeammatePolicyInput,
    draft: ChatTeammateStudioDraft,
    includeProfile: boolean,
  ): ReplaceChatTeammateAccessRequest {
    return {
      teammateId,
      expectedAccessRevision: accessRevision,
      teammateDefaultRuntimeApproval: draft.teammateDefaultRuntimeApproval,
      channels: copyChannelAccessInputs(draft.channels),
      teammateProfile: includeProfile ? {
        teammateId,
        displayName: draft.profile.displayName.trim(),
        avatar: copyVersionedJson(profileAvatar),
        role: draft.profile.role.trim(),
        instructions: draft.profile.instructions.trim(),
        expectedRevision: profileExpectedRevision,
      } : null,
      policy: includeProfile ? policy : null,
    };
  }

  function policyForDraft(draft: ChatTeammateStudioDraft): ChatTeammatePolicyInput {
    return {
      providerInstanceId: draft.profile.providerId,
      safetyMode: draft.profile.safetyMode,
      providerManagedModel: draft.profile.providerManagedModel,
      modelId: draft.profile.modelId || null,
      modelOptions: copyModelOptionSelections(draft.profile.modelOptions),
      effort: draft.profile.effort,
      speed: draft.profile.speed,
      providerOptions: copyVersionedJson(draft.profile.providerOptions),
    };
  }

  /** Saves the captured draft, binding any access confirmation to that exact snapshot. */
  async function save(confirmAccess = false): Promise<void> {
    if (saving || !canSave) return;
    if (confirmAccess && (
      !accessConfirmationImpact
      || accessConfirmationSnapshot !== currentDraftSnapshot
    )) {
      clearAccessConfirmation();
      return;
    }
    if (confirmAccess) clearAccessConfirmation();
    const creatingAtStart = creating;
    let requestDraft = captureStudioDraft();
    let includesProfileChange = !creatingAtStart && profileDirty;
    let includesAccessChange = !creatingAtStart && accessDirty;
    let savedDraft: ChatTeammateStudioDraft | null = null;
    let teammateId = selected?.participant.id ?? null;
    saving = true;
    error = null;
    errorField = null;
    conflictRecoveryNotice = null;
    try {
      if (creatingAtStart) {
        const creationPolicy = policyForDraft(requestDraft);
        const created = await chatApi.createChatTeammate({
          teammateId: `participant:${crypto.randomUUID()}`,
          displayName: requestDraft.profile.displayName.trim(),
          avatar: { schemaVersion: 1, value: { kind: "initials" } },
          role: requestDraft.profile.role.trim(),
          instructions: requestDraft.profile.instructions.trim(),
          policy: creationPolicy,
        });
        teammateId = created.participant.id;
        creating = false;
        selectedId = teammateId;
        profileExpectedRevision = created.participant.revision;
        profileAvatar = copyVersionedJson(created.participant.avatar);
        const inertAccess = await chatApi.readChatTeammateAccess(teammateId);
        const durableInertDraft = draftFromReads(created, inertAccess);
        requestDraft = captureStudioDraft();
        accessRevision = inertAccess.accessRevision;
        setStudioDraftBaseline(durableInertDraft);
        includesProfileChange = profileSnapshotForDraft(requestDraft)
          !== profileSnapshotForDraft(durableInertDraft);
        includesAccessChange = accessSnapshotForDraft(requestDraft)
          !== accessSnapshotForDraft(durableInertDraft);
        preserveStudioDraftForId = teammateId;
        await chat.refreshTeammates();
        if (!includesProfileChange && !includesAccessChange) savedDraft = durableInertDraft;
      }

      if (teammateId && (includesProfileChange || includesAccessChange)) {
        const policy = policyForDraft(requestDraft);
        const request = accessReplacementRequest(teammateId, policy, requestDraft, includesProfileChange);
        const requestSnapshot = draftSnapshotForDraft(requestDraft);
        if (!creatingAtStart && !confirmAccess && includesAccessChange) {
          const impact = teammateAccessConfirmationImpact(
            studioDraftBaseline?.channels ?? [],
            requestDraft.channels,
          );
          if (teammateAccessNeedsConfirmation(impact)) {
            const preview = await chatApi.previewChatTeammateAccess(request);
            const issue = preview.issues[0];
            if (issue) {
              error = issue.message;
              errorField = issue.fieldPath;
              return;
            }
            accessConfirmationImpact = impact;
            accessConfirmationSnapshot = requestSnapshot;
            return;
          }
        }
        const replaced = await chatApi.replaceChatTeammateAccess(request);
        accessRevision = replaced.accessRevision;
        savedDraft = requestDraft;
        clearAccessConfirmation();
      }
      if (!teammateId || !savedDraft) return;
      preserveStudioDraftForId = teammateId;
      await chat.refreshTeammates();
      const refreshed = chat.teammates.find((teammate) => teammate.participant.id === teammateId);
      if (refreshed) {
        profileExpectedRevision = refreshed.participant.revision;
        profileAvatar = copyVersionedJson(refreshed.participant.avatar);
      }
      setStudioDraftBaseline(savedDraft);
    } catch (cause: unknown) {
      if (teammateId && chatErrorCode(cause) === "stale_revision") {
        error = null;
        errorField = null;
        await loadAccessConflict(teammateId, captureStudioDraft());
      } else {
        error = chatErrorMessage(cause, t("settings.chat.teammates.saveFailed"));
        errorField = chatErrorField(cause);
      }
    } finally {
      saving = false;
    }
  }

  function clearFieldError(field: string): void {
    if (errorField !== field) return;
    error = null;
    errorField = null;
  }

  return {
    get selectedId() { return selectedId; },
    set selectedId(value: typeof selectedId) { selectedId = value; },
    get creating() { return creating; },
    set creating(value: typeof creating) { creating = value; },
    get showArchived() { return showArchived; },
    set showArchived(value: typeof showArchived) { showArchived = value; },
    get archivedTeammates() { return archivedTeammates; },
    get navigationChannels() { return navigationChannels; },
    get accessProfiles() { return accessProfiles; },
    set accessProfiles(value: typeof accessProfiles) { accessProfiles = value; },
    get loadingDirectory() { return loadingDirectory; },
    get loadingAccess() { return loadingAccess; },
    get saving() { return saving; },
    get error() { return error; },
    set error(value: typeof error) { error = value; },
    get errorField() { return errorField; },
    set errorField(value: typeof errorField) { errorField = value; },
    get displayName() { return displayName; },
    set displayName(value: typeof displayName) { displayName = value; },
    get role() { return role; },
    set role(value: typeof role) { role = value; },
    get instructions() { return instructions; },
    set instructions(value: typeof instructions) { instructions = value; },
    get providerId() { return providerId; },
    get safetyMode() { return safetyMode; },
    set safetyMode(value: typeof safetyMode) { safetyMode = value; },
    get modelId() { return modelId; },
    get providerManagedModel() { return providerManagedModel; },
    get modelOptions() { return modelOptions; },
    get teammateDefaultRuntimeApproval() { return teammateDefaultRuntimeApproval; },
    get accessDraft() { return accessDraft; },
    set accessDraft(value: typeof accessDraft) { accessDraft = value; },
    get accessConfirmationImpact() { return accessConfirmationImpact; },
    get accessConfirmationSnapshot() { return accessConfirmationSnapshot; },
    get accessConflict() { return accessConflict; },
    get conflictLoading() { return conflictLoading; },
    get conflictError() { return conflictError; },
    get conflictRecoveryNotice() { return conflictRecoveryNotice; },
    get lifecycleError() { return lifecycleError; },
    set lifecycleError(value: typeof lifecycleError) { lifecycleError = value; },
    get allDirectoryTeammates() { return allDirectoryTeammates; },
    get selected() { return selected; },
    get archivedMode() { return archivedMode; },
    get selectedProvider() { return selectedProvider; },
    get modelSelectionValid() { return modelSelectionValid; },
    get selectedModel() { return selectedModel; },
    get nameTaken() { return nameTaken; },
    get profileCeilings() { return profileCeilings; },
    get accessErrors() { return accessErrors; },
    get providerResourceBlockers() { return providerResourceBlockers; },
    get currentDraftSnapshot() { return currentDraftSnapshot; },
    get conflictComparison() { return conflictComparison; },
    get dirty() { return dirty; },
    get canSave() { return canSave; },
    loadDirectoryData,
    clearAccessConfirmation,
    beginCreate,
    cancelCreate,
    initializeSelectedTeammate,
    retryAccessConflict,
    rebaseAccessConflict,
    reloadCurrentAccessConflict,
    defaultChannelAccess,
    handleProfilesChange,
    selectExecution,
    save,
    clearFieldError,
  };
}
