import * as chatApi from "$lib/api/chat";
import {
  copyModelOptionSelections,
  copyVersionedJson,
  defaultModelOptions,
} from "$lib/chat/composer/model";
import type {
  ChatAiAgentRead,
  ChatChannelRead,
  ChatFolderCapability,
  ChatAgentPolicyInput,
  ChatAgentAccessRead,
  ChatAgentChannelAccessInput,
  ModelOptionSelection,
  ReplaceChatAgentAccessRequest,
  SafetyMode,
  VersionedJson,
} from "$lib/chat/contracts";
import { chatErrorCode, chatErrorField, chatErrorMessage } from "$lib/chat/error-presentation";
import {
  impliedFolderGrantId,
  agentAccessConfirmationImpact,
  agentAccessDraftErrors,
  agentAccessDraftSnapshot,
  agentAccessNeedsConfirmation,
  type ChatAgentAccessConfirmationImpact,
} from "./access";
import { defaultChannelAccessInput } from "./channel-membership";
import { providerExecutesWithAsk } from "$lib/chat/composer/provider-authority";
import { firstPickerChoice } from "$lib/chat/composer/model-picker-model";
import { preferredProjectWorkingFolder } from "$lib/chat/workspace/working-folder-selection";
import { agentExecutionSummary, agentInstructionsTooLong, agentProfileDraftSnapshot } from "./draft";
import {
  cloneChatAgentStudioDraft,
  compareChatAgentStudioDrafts,
  rebaseChatAgentStudioDraft,
  type ChatAgentStudioDraft,
} from "./access-conflict";
import type { Translate } from "$lib/i18n/translator.svelte";
import type { getChat } from "$lib/stores/chat.svelte";

export interface AgentEditorOptions {
  chat: Pick<
    ReturnType<typeof getChat>,
    "agents" | "archivedAgents" | "settings" | "refreshAgents" | "workingFolders" | "primaryWorkingFolder"
  >;
  t: Translate;
  initialChannelId: () => string | null;
  closeAccessPicker: () => void;
  setExpandedAccessChannel: (channelId: string | null) => void;
  focusConflict: () => Promise<void>;
  focusRecoveryNotice: () => void;
}

type ConflictRecoveryNotice = "rebased" | "reloaded";

interface AgentAccessConflictState {
  agentId: string;
  localDraft: ChatAgentStudioDraft;
  baselineDraft: ChatAgentStudioDraft;
  durableDraft: ChatAgentStudioDraft;
  durableAgent: ChatAiAgentRead;
  durableAccess: ChatAgentAccessRead;
}

/** Owns one mounted agent editor's drafts, revision checks, and save/conflict workflow. */
export function createAgentEditorController(options: AgentEditorOptions) {
  const { chat, t } = options;
  let selectedId = $state<string | null>(null);
  let creating = $state(false);
  let showArchived = $state(false);
  let archivedAgents = $state<ChatAiAgentRead[]>([]);
  let navigationChannels = $state<ChatChannelRead[]>([]);
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
  let profileExpectedRevision = $state(0);
  let profileAvatar = $state<VersionedJson>({ schemaVersion: 1, value: { kind: "initials" } });
  let accessRevision = $state(0);
  let accessDraft = $state<ChatAgentChannelAccessInput[]>([]);
  let untouchedDefaultAccess: string | null = null;
  let profileBaseline = $state<string | null>(null);
  let accessBaseline = $state<string | null>(null);
  let studioDraftBaseline = $state<ChatAgentStudioDraft | null>(null);
  let accessConfirmationImpact = $state<ChatAgentAccessConfirmationImpact | null>(null);
  let accessConfirmationSnapshot = $state<string | null>(null);
  let accessConflict = $state<AgentAccessConflictState | null>(null);
  let conflictPendingDraft = $state<ChatAgentStudioDraft | null>(null);
  let conflictLoading = $state(false);
  let conflictError = $state<string | null>(null);
  let conflictRecoveryNotice = $state<ConflictRecoveryNotice | null>(null);

  let accessLoadRequest = 0;
  let conflictLoadRequest = 0;
  let lifecycleError = $state<string | null>(null);
  let preserveStudioDraftForId: string | null = null;

  const allDirectoryAgents = $derived(showArchived
    ? [...chat.agents, ...archivedAgents]
    : chat.agents);
  const allAgents = $derived([...chat.agents, ...archivedAgents]);
  const selected = $derived(allDirectoryAgents.find((entry) => entry.participant.id === selectedId) ?? null);
  const archivedMode = $derived(Boolean(selected?.participant.archivedAt));
  const providers = $derived(chat.settings?.providerInstances ?? []);
  const selectedProvider = $derived(providers.find((entry) => entry.configuration.instanceId === providerId) ?? null);
  const providerAuthoritySupport = $derived(selectedProvider?.lastProbe?.authoritySupport ?? null);
  const availableModels = $derived(selectedProvider?.modelCatalog?.models.filter((model) => model.availability !== "deprecated") ?? []);
  const selectedModel = $derived(availableModels.find((model) => model.id === modelId) ?? null);
  const modelSelectionValid = $derived(providerManagedModel !== Boolean(modelId));
  const normalizedDisplayName = $derived(displayName.trim().toLocaleLowerCase());
  const instructionsTooLong = $derived(agentInstructionsTooLong(instructions));
  const nameTaken = $derived(Boolean(normalizedDisplayName && allAgents.some((agent) => (
    agent.participant.id !== selectedId
      && agent.participant.displayName.trim().toLocaleLowerCase() === normalizedDisplayName
  ))));
  const accessErrors = $derived(agentAccessDraftErrors(accessDraft));
  const providerResourceBlockers = $derived(accessDraft
    .flatMap(providerResourceIssuesForChannel)
    .filter((issue, index, issues) => issues.indexOf(issue) === index));
  const currentProfileSnapshot = $derived(agentProfileDraftSnapshot({
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
    channels: agentAccessDraftSnapshot(accessDraft),
  }));
  const currentDraftSnapshot = $derived(JSON.stringify({
    profile: currentProfileSnapshot,
    providerOptions: JSON.stringify(providerOptions),
    access: currentAccessSnapshot,
  }));
  const conflictComparison = $derived(accessConflict
    ? compareChatAgentStudioDrafts(captureStudioDraft(), accessConflict.durableDraft)
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
      && !instructionsTooLong
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
    const [archivedResult, channelResult] = await Promise.allSettled([
      chatApi.listChatAgents(true),
      chatApi.listChatNavigationChannels(),
    ]);
    if (archivedResult.status === "fulfilled") {
      archivedAgents = archivedResult.value;
      chat.archivedAgents = archivedResult.value;
    }
    if (channelResult.status === "fulfilled") navigationChannels = channelResult.value;
    const failure = [archivedResult, channelResult]
      .find((result) => result.status === "rejected");
    if (failure?.status === "rejected") {
      error = chatErrorMessage(failure.reason, t("settings.agents.agents.loadFailed"));
    }
    loadingDirectory = false;
  }

  function captureStudioDraft(): ChatAgentStudioDraft {
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
      channels: copyChannelAccessInputs(accessDraft),
    };
  }

  function draftFromReads(
    agent: ChatAiAgentRead,
    access: ChatAgentAccessRead,
  ): ChatAgentStudioDraft {
    const policy = agent.latestPolicy;
    const options = copyModelOptionSelections(policy?.modelOptions ?? []);
    const provider = providers.find((entry) => (
      entry.configuration.instanceId === policy?.providerInstanceId
    )) ?? null;
    const model = provider?.modelCatalog?.models.find((entry) => entry.id === policy?.modelId) ?? null;
    const summary = agentExecutionSummary(options, model);
    return {
      profile: {
        displayName: agent.participant.displayName,
        role: agent.role,
        instructions: agent.instructions,
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
      channels: access.channels
        .filter((channel) => channel.removedAt === null)
        .map((channel) => ({
          channelId: channel.channelId,
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

  function applyStudioDraft(draft: ChatAgentStudioDraft): void {
    const copy = cloneChatAgentStudioDraft(draft);
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
    accessDraft = copy.channels;
  }

  function setStudioDraftBaseline(draft: ChatAgentStudioDraft): void {
    const copy = cloneChatAgentStudioDraft(draft);
    studioDraftBaseline = copy;
    profileBaseline = profileSnapshotForDraft(copy);
    accessBaseline = accessSnapshotForDraft(copy);
  }

  function profileSnapshotForDraft(draft: ChatAgentStudioDraft): string {
    return agentProfileDraftSnapshot(draft.profile);
  }

  function accessSnapshotForDraft(draft: ChatAgentStudioDraft): string {
    return JSON.stringify({
      channels: agentAccessDraftSnapshot(draft.channels),
    });
  }

  function draftSnapshotForDraft(draft: ChatAgentStudioDraft): string {
    return JSON.stringify({
      profile: profileSnapshotForDraft(draft),
      providerOptions: JSON.stringify(draft.profile.providerOptions),
      access: accessSnapshotForDraft(draft),
    });
  }

  function copyChannelAccessInputs(
    channels: readonly ChatAgentChannelAccessInput[],
  ): ChatAgentChannelAccessInput[] {
    return channels.map((channel) => ({
      ...channel,
      capabilities: { ...channel.capabilities },
      historyBoundary: channel.historyBoundary.kind === "entire"
        ? { kind: "entire" }
        : { kind: "fromGrant", lowerOrderKey: channel.historyBoundary.lowerOrderKey },
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
    const resolved = firstPickerChoice(providers, chat.settings?.configuration.rememberedSelections ?? [], Date.now());
    providerId = resolved?.provider.configuration.instanceId ?? "";
    safetyMode = "ask_for_approval";
    modelId = resolved?.model?.id ?? "";
    providerManagedModel = resolved?.providerManaged ?? false;
    modelOptions = resolved?.model ? defaultModelOptions(resolved.model.options) : [];
    const summary = agentExecutionSummary(modelOptions, resolved?.model ?? null);
    effort = summary.effort;
    speed = summary.speed;
    providerOptions = { schemaVersion: 1, value: {} };
    profileExpectedRevision = 0;
    profileAvatar = { schemaVersion: 1, value: { kind: "initials" } };
    accessRevision = 0;
    accessDraft = channelId ? [defaultChannelAccess(channelId)] : [];
    untouchedDefaultAccess = currentAccessSnapshot;
    setStudioDraftBaseline(captureStudioDraft());
    clearAccessConfirmation();
    error = null;
    errorField = null;
  }

  function cancelCreate(): void {
    clearAccessConflict();
    lifecycleError = null;
    creating = false;
    selectedId = chat.agents[0]?.participant.id ?? null;
    profileBaseline = null;
    accessBaseline = null;
    studioDraftBaseline = null;
    clearAccessConfirmation();
    options.closeAccessPicker();
  }

  /** Loads the selected agent without resetting a draft preserved across this editor's own refresh. */
  async function initializeSelectedAgent(agent: ChatAiAgentRead): Promise<void> {
    if (preserveStudioDraftForId === agent.participant.id) {
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
    displayName = agent.participant.displayName;
    role = agent.role;
    instructions = agent.instructions;
    providerId = agent.latestPolicy?.providerInstanceId ?? "";
    safetyMode = agent.latestPolicy?.safetyMode ?? "ask_for_approval";
    modelId = agent.latestPolicy?.modelId ?? "";
    providerManagedModel = agent.latestPolicy?.providerManagedModel ?? false;
    modelOptions = copyModelOptionSelections(agent.latestPolicy?.modelOptions ?? []);
    const summary = agentExecutionSummary(modelOptions, selectedModel);
    effort = summary.effort ?? agent.latestPolicy?.effort ?? null;
    speed = summary.speed ?? agent.latestPolicy?.speed ?? null;
    providerOptions = copyVersionedJson(
      agent.latestPolicy?.providerOptions ?? { schemaVersion: 1, value: {} },
    );
    profileExpectedRevision = agent.participant.revision;
    profileAvatar = copyVersionedJson(agent.participant.avatar);
    profileBaseline = currentProfileSnapshot;
    await loadAgentAccess(agent.participant.id);
  }

  async function loadAgentAccess(agentId: string): Promise<void> {
    const request = ++accessLoadRequest;
    loadingAccess = true;
    try {
      const access = await chatApi.readChatAgentAccess(agentId);
      if (request !== accessLoadRequest || selectedId !== agentId) return;
      const agent = allDirectoryAgents.find((entry) => entry.participant.id === agentId);
      if (!agent) return;
      const durableDraft = draftFromReads(agent, access);
      accessRevision = access.accessRevision;
      profileExpectedRevision = agent.participant.revision;
      profileAvatar = copyVersionedJson(agent.participant.avatar);
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
        error = chatErrorMessage(cause, t("settings.agents.agents.accessLoadFailed"));
      }
    } finally {
      if (request === accessLoadRequest) loadingAccess = false;
    }
  }

  async function loadAccessConflict(
    agentId: string,
    localDraft: ChatAgentStudioDraft,
  ): Promise<void> {
    const request = ++conflictLoadRequest;
    const retainedLocalDraft = cloneChatAgentStudioDraft(localDraft);
    const retainedBaseline = cloneChatAgentStudioDraft(
      studioDraftBaseline ?? retainedLocalDraft,
    );
    conflictPendingDraft = retainedLocalDraft;
    accessConflict = null;
    conflictError = null;
    conflictRecoveryNotice = null;
    clearAccessConfirmation();
    conflictLoading = true;
    try {
      const [durableAgent, durableAccess] = await Promise.all([
        chatApi.readChatAgent(agentId),
        chatApi.readChatAgentAccess(agentId),
      ]);
      if (request !== conflictLoadRequest || selectedId !== agentId) return;
      accessConflict = {
        agentId,
        localDraft: retainedLocalDraft,
        baselineDraft: retainedBaseline,
        durableDraft: draftFromReads(durableAgent, durableAccess),
        durableAgent,
        durableAccess,
      };
    } catch (cause: unknown) {
      if (request !== conflictLoadRequest) return;
      conflictError = chatErrorMessage(
        cause,
        t("settings.agents.agents.conflict.loadFailed"),
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
    const rebased = rebaseChatAgentStudioDraft(
      conflict.baselineDraft,
      captureStudioDraft(),
      conflict.durableDraft,
    );
    applyStudioDraft(rebased);
    accessRevision = conflict.durableAccess.accessRevision;
    profileExpectedRevision = conflict.durableAgent.participant.revision;
    profileAvatar = copyVersionedJson(conflict.durableAgent.participant.avatar);
    setStudioDraftBaseline(conflict.durableDraft);
    settleAccessConflict("rebased");
  }

  async function reloadCurrentAccessConflict(): Promise<void> {
    const conflict = accessConflict;
    if (!conflict) return;
    applyStudioDraft(conflict.durableDraft);
    accessRevision = conflict.durableAccess.accessRevision;
    profileExpectedRevision = conflict.durableAgent.participant.revision;
    profileAvatar = copyVersionedJson(conflict.durableAgent.participant.avatar);
    setStudioDraftBaseline(conflict.durableDraft);
    preserveStudioDraftForId = conflict.agentId;
    settleAccessConflict("reloaded");
    try {
      await chat.refreshAgents();
    } catch (cause: unknown) {
      error = chatErrorMessage(cause, t("settings.agents.agents.loadFailed"));
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

  function providerCapabilityIssue(capability: ChatFolderCapability): string | null {
    if (capability === "none") return null;
    const support = providerAuthoritySupport;
    if (!support) return t("settings.agents.agents.providerAuthorityUnknown");
    if (capability === "read" && !support.internalHostTools && !support.readOnlyRoot) {
      return t("settings.agents.agents.providerCannotRead");
    }
    if (capability === "edit" && !support.internalHostTools && !support.writableRoot) {
      return t("settings.agents.agents.providerCannotEdit");
    }
    if (capability === "execute" && !providerExecutesWithAsk(support, safetyMode)) {
      return t("settings.agents.agents.providerCannotExecute");
    }
    if (capability === "publish" && (
      !support.writableRoot
      || !support.confinedCommands
      || !support.networkBoundary
      || !support.classifiedPublish
    )) {
      return t("settings.agents.agents.providerCannotPublish");
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
      return t("settings.agents.agents.providerCannotTargetReadOnly");
    }
    if (capability === "edit" && (!support.writableRoot || !support.denyShell)) {
      return t("settings.agents.agents.providerCannotTargetWritable");
    }
    return null;
  }

  /**
   * The working folder the send gate treats as the channel project's: the stored primary folder when it is known for
   * that project, else the project's managed folder.
   */
  function channelProjectWorkingFolderId(channelId: string): string | null {
    const projectId = navigationChannels.find((channel) => channel.id === channelId)?.projectId ?? null;
    if (projectId === null) return null;
    if (chat.primaryWorkingFolder?.projectId === projectId) return chat.primaryWorkingFolder.workingFolderId;
    return chat.workingFolders.find((folder) => (
      folder.workingFolder.projectId === projectId
        && folder.workingFolder.kind === "managed"
        && folder.workingFolder.archivedAt === null
    ))?.workingFolder.id ?? null;
  }

  function providerResourceIssuesForChannel(channel: ChatAgentChannelAccessInput): string[] {
    const grants = channel.folderGrants.filter((grant) => grant.capability !== "none");
    const impliedFolderId = impliedFolderGrantId(grants, channelProjectWorkingFolderId(channel.channelId));
    const issues = grants.flatMap((grant) => {
      const issue = providerGrantIssue(grant.capability, grant.workingFolderId === impliedFolderId);
      return issue ? [issue] : [];
    });
    if (grants.some((grant) => grant.workingFolderId !== impliedFolderId) && !providerAuthoritySupport?.internalHostTools) {
      issues.push(t("settings.agents.agents.providerNeedsHostTools"));
    }
    return issues.filter((issue, index) => issues.indexOf(issue) === index);
  }

  function defaultChannelAccess(channelId: string): ChatAgentChannelAccessInput {
    const projectId = navigationChannels.find((channel) => channel.id === channelId)?.projectId ?? null;
    const folder = projectId ? preferredProjectWorkingFolder(chat.workingFolders, projectId, null) : null;
    return defaultChannelAccessInput(
      channelId,
      { projectWorkingFolderId: folder?.workingFolder.id ?? null },
      { authority: providerAuthoritySupport, safetyMode },
    );
  }

  /** Rebuilds a new agent's untouched default access after its provider or approval changes. */
  function refreshUntouchedDefaultAccess(): void {
    if (!creating || untouchedDefaultAccess === null || currentAccessSnapshot !== untouchedDefaultAccess) return;
    accessDraft = accessDraft.map((channel) => defaultChannelAccess(channel.channelId));
    untouchedDefaultAccess = currentAccessSnapshot;
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
    const summary = agentExecutionSummary(modelOptions, model);
    effort = summary.effort;
    speed = summary.speed;
    refreshUntouchedDefaultAccess();
  }

  function accessReplacementRequest(
    agentId: string,
    policy: ChatAgentPolicyInput,
    draft: ChatAgentStudioDraft,
    includeProfile: boolean,
  ): ReplaceChatAgentAccessRequest {
    return {
      agentId,
      expectedAccessRevision: accessRevision,
      channels: copyChannelAccessInputs(draft.channels),
      agentProfile: includeProfile ? {
        agentId,
        displayName: draft.profile.displayName.trim(),
        avatar: copyVersionedJson(profileAvatar),
        role: draft.profile.role.trim(),
        instructions: draft.profile.instructions.trim(),
        expectedRevision: profileExpectedRevision,
      } : null,
      policy: includeProfile ? policy : null,
    };
  }

  function policyForDraft(draft: ChatAgentStudioDraft): ChatAgentPolicyInput {
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
    let savedDraft: ChatAgentStudioDraft | null = null;
    let agentId = selected?.participant.id ?? null;
    saving = true;
    error = null;
    errorField = null;
    conflictRecoveryNotice = null;
    try {
      if (creatingAtStart) {
        const creationPolicy = policyForDraft(requestDraft);
        const created = await chatApi.createChatAgent({
          agentId: `participant:${crypto.randomUUID()}`,
          displayName: requestDraft.profile.displayName.trim(),
          avatar: { schemaVersion: 1, value: { kind: "initials" } },
          role: requestDraft.profile.role.trim(),
          instructions: requestDraft.profile.instructions.trim(),
          policy: creationPolicy,
        });
        agentId = created.participant.id;
        creating = false;
        selectedId = agentId;
        profileExpectedRevision = created.participant.revision;
        profileAvatar = copyVersionedJson(created.participant.avatar);
        const inertAccess = await chatApi.readChatAgentAccess(agentId);
        const durableInertDraft = draftFromReads(created, inertAccess);
        requestDraft = captureStudioDraft();
        accessRevision = inertAccess.accessRevision;
        setStudioDraftBaseline(durableInertDraft);
        includesProfileChange = profileSnapshotForDraft(requestDraft)
          !== profileSnapshotForDraft(durableInertDraft);
        includesAccessChange = accessSnapshotForDraft(requestDraft)
          !== accessSnapshotForDraft(durableInertDraft);
        preserveStudioDraftForId = agentId;
        await chat.refreshAgents();
        if (!includesProfileChange && !includesAccessChange) savedDraft = durableInertDraft;
      }

      if (agentId && (includesProfileChange || includesAccessChange)) {
        const policy = policyForDraft(requestDraft);
        const request = accessReplacementRequest(agentId, policy, requestDraft, includesProfileChange);
        const requestSnapshot = draftSnapshotForDraft(requestDraft);
        if (!creatingAtStart && !confirmAccess && includesAccessChange) {
          const impact = agentAccessConfirmationImpact(
            studioDraftBaseline?.channels ?? [],
            requestDraft.channels,
          );
          if (agentAccessNeedsConfirmation(impact)) {
            const preview = await chatApi.previewChatAgentAccess(request);
            const issue = preview.issues[0];
            if (issue) {
              error = t(`chatErrors.accessIssues.${issue.code}`);
              errorField = issue.fieldPath;
              return;
            }
            accessConfirmationImpact = impact;
            accessConfirmationSnapshot = requestSnapshot;
            return;
          }
        }
        const replaced = await chatApi.replaceChatAgentAccess(request);
        accessRevision = replaced.accessRevision;
        savedDraft = requestDraft;
        clearAccessConfirmation();
      }
      if (!agentId || !savedDraft) return;
      preserveStudioDraftForId = agentId;
      await chat.refreshAgents();
      const refreshed = chat.agents.find((agent) => agent.participant.id === agentId);
      if (refreshed) {
        profileExpectedRevision = refreshed.participant.revision;
        profileAvatar = copyVersionedJson(refreshed.participant.avatar);
      }
      setStudioDraftBaseline(savedDraft);
    } catch (cause: unknown) {
      if (agentId && chatErrorCode(cause) === "stale_revision") {
        error = null;
        errorField = null;
        await loadAccessConflict(agentId, captureStudioDraft());
      } else {
        error = chatErrorMessage(cause, t("settings.agents.agents.saveFailed"));
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
    get archivedAgents() { return archivedAgents; },
    get navigationChannels() { return navigationChannels; },
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
    set safetyMode(value: typeof safetyMode) {
      safetyMode = value;
      refreshUntouchedDefaultAccess();
    },
    get modelId() { return modelId; },
    get providerManagedModel() { return providerManagedModel; },
    get modelOptions() { return modelOptions; },
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
    get allDirectoryAgents() { return allDirectoryAgents; },
    get selected() { return selected; },
    get archivedMode() { return archivedMode; },
    get selectedProvider() { return selectedProvider; },
    get modelSelectionValid() { return modelSelectionValid; },
    get selectedModel() { return selectedModel; },
    get nameTaken() { return nameTaken; },
    get instructionsTooLong() { return instructionsTooLong; },
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
    initializeSelectedAgent,
    retryAccessConflict,
    rebaseAccessConflict,
    reloadCurrentAccessConflict,
    defaultChannelAccess,
    selectExecution,
    save,
    clearFieldError,
  };
}
