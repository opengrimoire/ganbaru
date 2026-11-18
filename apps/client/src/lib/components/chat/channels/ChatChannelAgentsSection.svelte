<script lang="ts">
  import X from "@lucide/svelte/icons/x";
  import * as chatApi from "$lib/api/chat";
  import type {
    ChatAgentResolvedAccessRead,
    ChatAiAgentRead,
    ChatChannelAgentSettingsRead,
    ChatChannelMembershipRemovalPreview,
    ChatChannelRead,
    ChatParticipantId,
    ProviderModel,
  } from "$lib/chat/contracts";
  import {
    channelModelOverride,
    chatAgentBehaviorLine,
    chatAgentDefaultModelName,
    ownAccessAvailable,
  } from "$lib/chat/agents/channel-access";
  import { formatModelDisplayName } from "$lib/chat/composer/model-company";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import ChatAgentScopeSettings, { type ChatAgentScopeValue } from "$lib/components/chat/agents/ChatAgentScopeSettings.svelte";
  import ChatParticipantAvatar from "$lib/components/chat/identity/ChatParticipantAvatar.svelte";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import Select from "$lib/components/ui/Select.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { useChat } from "$lib/stores/chat.svelte";
  import { portal } from "$lib/utils/portal";
  import ChatPushApprovalsList from "./ChatPushApprovalsList.svelte";

  /**
   * The Agents section of a channel's details: the access and instructions every agent in the channel works
   * under, each agent with what it does on this device and its model, and the channel's push approvals. It is
   * the one place that edits per-channel agent behavior.
   */
  let { channel }: { channel: ChatChannelRead } = $props();

  /** One agent row: the agent record joined with what it resolves to in this channel. */
  interface AgentRow {
    agent: ChatAiAgentRead;
    resolved: ChatAgentResolvedAccessRead;
    behavior: string;
    modelOptions: { value: string; label: string }[];
    modelValue: string;
  }

  const AVATAR_SIZE = 26;
  const chat = useChat();
  const { t } = getLocalization();

  let settings = $state<ChatChannelAgentSettingsRead | null>(null);
  let loadError = $state<string | null>(null);
  let rowError = $state<string | null>(null);
  let busyAgentId = $state<ChatParticipantId | null>(null);
  let removal = $state<{ agent: ChatAiAgentRead; preview: ChatChannelMembershipRemovalPreview } | null>(null);
  let request = 0;

  const rows = $derived.by((): AgentRow[] => {
    const current = settings;
    if (!current) return [];
    return current.agents.flatMap((resolved) => {
      const agent = chat.agents.find((entry) => entry.participant.id === resolved.agentId);
      if (!agent) return [];
      const catalog = chat.settings?.providerInstances.find((provider) => (
        provider.configuration.instanceId === agent.localProviderInstanceId
      ))?.modelCatalog ?? null;
      const behavior = chatAgentBehaviorLine(t, resolved, agent.participant.displayName);
      if (agent.agentKind !== "provider") {
        /* Model line and named agents keep their own model, so a channel cannot override it. */
        const fixedLabel = agent.agentKind === "model"
          ? t("chat.agentAccess.newestOfLine", agent.participant.displayName)
          : t("chat.agentAccess.fixedModel", formatModelDisplayName(chatAgentDefaultModelName(t, agent.latestPolicy, catalog)));
        return [{ agent, resolved, behavior, modelOptions: [{ value: "", label: fixedLabel }], modelValue: "" }];
      }
      const override = channelModelOverride(current.modelOverrides, agent.participant.id);
      const defaultName = chatAgentDefaultModelName(t, agent.latestPolicy, catalog);
      const models: readonly Pick<ProviderModel, "id" | "displayName">[] = catalog?.models ?? [];
      const modelOptions = [
        { value: "", label: t("chat.agentAccess.defaultModel", formatModelDisplayName(defaultName)) },
        ...models.map((model) => ({ value: model.id, label: formatModelDisplayName(model.displayName) })),
      ];
      if (override && !models.some((model) => model.id === override)) modelOptions.push({ value: override, label: override });
      return [{
        agent,
        resolved,
        behavior,
        modelOptions,
        modelValue: override ?? "",
      }];
    });
  });

  async function load(channelId: string): Promise<void> {
    const current = ++request;
    try {
      const read = await chatApi.readChatChannelAgentSettings(channelId);
      if (current !== request) return;
      settings = read;
      loadError = null;
    } catch (cause: unknown) {
      if (current === request) loadError = chatErrorMessage(cause, t("chat.agentAccess.loadFailed"));
    }
  }

  /** Applies a fresh settings read a command returned and tells other views of the channel to reread. */
  function accept(read: ChatChannelAgentSettingsRead): void {
    request += 1;
    settings = read;
    chat.agentSettingsChanged(read.channelId);
  }

  async function saveScope(value: ChatAgentScopeValue): Promise<void> {
    const current = settings;
    if (!current) return;
    try {
      accept(await chatApi.updateChatChannelAgentSettings({
        channelId: current.channelId,
        access: value.access,
        instructions: value.instructions,
        expectedRevision: current.revision,
      }));
    } catch (cause: unknown) {
      await load(current.channelId);
      throw cause;
    }
  }

  async function runForAgent(agentId: ChatParticipantId, action: () => Promise<void>): Promise<void> {
    if (busyAgentId) return;
    busyAgentId = agentId;
    rowError = null;
    try {
      await action();
    } catch (cause: unknown) {
      rowError = chatErrorMessage(cause, t("chat.agentAccess.saveFailed"));
      await load(channel.id);
    } finally {
      busyAgentId = null;
    }
  }

  function chooseModel(agentId: ChatParticipantId, value: string): void {
    void runForAgent(agentId, async () => {
      accept(await chatApi.setChatChannelModelOverride({
        channelId: channel.id,
        agentId,
        model: value ? { providerManagedModel: false, modelId: value, modelOptions: [] } : null,
      }));
    });
  }

  function requestRemoval(agent: ChatAiAgentRead): void {
    void runForAgent(agent.participant.id, async () => {
      const preview = await chatApi.previewChatChannelMembershipRemoval({ agentId: agent.participant.id, channelId: channel.id });
      if (preview.willRevokeActiveWork) removal = { agent, preview };
      else await remove(agent.participant.id);
    });
  }

  async function remove(agentId: ChatParticipantId): Promise<void> {
    accept(await chatApi.removeChatChannelAgent({ channelId: channel.id, agentId }));
    await chat.channelAgentsChanged(channel.id);
  }

  function confirmRemoval(): void {
    const pending = removal;
    removal = null;
    if (pending) void runForAgent(pending.agent.participant.id, () => remove(pending.agent.participant.id));
  }

  function agentName(agentId: ChatParticipantId): string {
    return chat.agents.find((agent) => agent.participant.id === agentId)?.participant.displayName ?? t("chat.organization.agent");
  }

  $effect(() => {
    void chat.agentSettingsVersion;
    void channel.memberships;
    void load(channel.id);
  });
</script>

<section class="grid gap-3 px-2 pt-1 pb-2" aria-label={t("chat.agentAccess.title")}>
  {#if loadError}
    <Notice class="text-panel-detail" message={loadError} onRetry={() => load(channel.id)} copy={false} />
  {:else if settings}
    <ChatAgentScopeSettings
      access={settings.access}
      effective={settings.effective}
      instructions={settings.instructions}
      ownAvailable={ownAccessAvailable(settings.agents)}
      idPrefix={`channel-${settings.channelId}`}
      onSave={saveScope}
    />

    {#if rows.length === 0}
      <p class="section-detail">{t("chat.agentAccess.noAgents")}</p>
    {:else}
      <ul class="grid gap-1">
        {#each rows as row (row.agent.participant.id)}
          <li class="agent-row">
            <ChatParticipantAvatar participant={row.agent.participant} agent={row.agent} size={AVATAR_SIZE} />
            <div class="grid min-w-0 flex-1 gap-1">
              <div class="flex min-w-0 items-center gap-1">
                <span class="agent-name" dir="auto">{row.agent.participant.displayName}</span>
                <button
                  type="button"
                  class="agent-remove"
                  aria-label={t("chat.organization.removeAgent", row.agent.participant.displayName)}
                  disabled={busyAgentId !== null}
                  onclick={() => requestRemoval(row.agent)}
                ><X size={14} /></button>
              </div>
              <p class="section-detail">{row.behavior}</p>
              {#if row.modelOptions.length > 1}
                <Select
                  inline
                  class="w-full"
                  value={row.modelValue}
                  options={row.modelOptions}
                  ariaLabel={t("chat.agentAccess.modelFor", row.agent.participant.displayName)}
                  disabled={busyAgentId !== null}
                  onChange={(value) => chooseModel(row.agent.participant.id, value)}
                />
              {:else}
                <p class="section-detail">{row.modelOptions[0]?.label}</p>
              {/if}
            </div>
          </li>
        {/each}
      </ul>
    {/if}
    {#if rowError}<Notice class="text-panel-detail" message={rowError} copy={false} />{/if}

    {#if settings.pushApprovals.length > 0}
      <div class="grid gap-1">
        <span class="section-label">{t("chat.agentAccess.pushApprovals")}</span>
        <p class="section-detail">{t("chat.agentAccess.pushApprovalsHint")}</p>
        <ChatPushApprovalsList approvals={settings.pushApprovals} {agentName} onChanged={() => load(channel.id)} />
      </div>
    {/if}
  {/if}
</section>

{#if removal}
  <div class="contents" use:portal>
    <ConfirmDialog
      title={t("chat.organization.removeAgentTitle", removal.agent.participant.displayName)}
      message={t("chat.organization.removeAgentImpact", removal.preview.activeAssignmentCount, removal.preview.activeAuthorizationCount)}
      confirmLabel={t("chat.organization.removeAgentConfirm")}
      cancelLabel={t("common.cancel")}
      onConfirm={confirmRemoval}
      onCancel={() => { removal = null; }}
    />
  </div>
{/if}

<style>
  .section-label { color: var(--muted-foreground); font-size: var(--panel-detail-font-size); font-weight: 500; line-height: 1.4; }
  .section-detail { margin: 0; color: var(--muted-foreground); font-size: var(--panel-detail-font-size); line-height: 1.4; overflow-wrap: anywhere; }
  .agent-row { display: flex; align-items: flex-start; gap: 0.6rem; padding-block: 0.25rem; }
  .agent-name { min-width: 0; flex: 1; overflow: hidden; font-weight: 500; text-overflow: ellipsis; white-space: nowrap; }
  .agent-remove { display: grid; width: 1.75rem; height: 1.75rem; flex: 0 0 auto; place-items: center; border-radius: var(--floating-item-radius); color: var(--muted-foreground); }
  .agent-remove:hover:not(:disabled) { background: var(--accent); color: var(--foreground); }
  .agent-remove:disabled { opacity: 0.45; }
  @media (pointer: coarse) { .agent-remove { width: 2.5rem; height: 2.5rem; } }
</style>
