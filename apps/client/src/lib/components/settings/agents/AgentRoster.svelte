<script lang="ts">
  import Plus from "@lucide/svelte/icons/plus";
  import * as chatApi from "$lib/api/chat";
  import {
    agentNameProblem,
    familyModelLines,
    MAX_AGENT_NAME_CHARACTERS,
    type AgentNameProblem,
  } from "$lib/chat/agents/agent-names";
  import { chatAgentDefaultModelName } from "$lib/chat/agents/channel-access";
  import { formatModelDisplayName } from "$lib/chat/composer/model-company";
  import { resolveRecommendedSelection } from "$lib/chat/composer/model-picker-model";
  import type {
    ChatAiAgentRead,
    ModelOptionSelection,
    ProviderFamilyId,
    ProviderInstanceId,
  } from "$lib/chat/contracts";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import ChatModelControls from "$lib/components/chat/composer/ChatModelControls.svelte";
  import { SETTINGS_PRIMARY_BUTTON_CLASS, SETTINGS_SECONDARY_BUTTON_CLASS } from "$lib/components/settings/action-buttons";
  import Notice from "$lib/components/ui/Notice.svelte";
  import SwitchField from "$lib/components/ui/SwitchField.svelte";
  import { formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { BUILD_PLATFORM_PROFILE, platformHasCapability } from "$lib/platform";
  import { getChat } from "$lib/stores/chat.svelte";

  /**
   * The agents a provider family offers beyond its provider agent: one per model line, such as `@Opus`, that always
   * runs the line's newest model, and the agents a person named with a model and options of their own. A switch turns
   * each one on or off for channels and mentions.
   */
  let { familyId, instanceId }: { familyId: ProviderFamilyId; instanceId: ProviderInstanceId } = $props();

  interface ModelSelection {
    providerInstanceId: string | null;
    modelId: string | null;
    providerManaged: boolean;
    options: ModelOptionSelection[];
  }

  const localization = getLocalization();
  const { t } = localization;
  const chat = getChat();
  /** Named agents are created where providers run, so only a device with local execution offers the form. */
  const canCreate = platformHasCapability(BUILD_PLATFORM_PROFILE, "chat.local-execution");
  let busyAgentId = $state<string | null>(null);
  let error = $state<string | null>(null);
  let adding = $state(false);
  let creating = $state(false);
  let name = $state("");
  let nameTouched = $state(false);
  let selection = $state<ModelSelection>(emptySelection());

  const lines = $derived(familyModelLines(familyId));
  const familyAgents = $derived(chat.agentIdentities.filter((agent) => agent.providerFamilyId === familyId));
  const modelAgents = $derived(familyAgents
    .filter((agent) => agent.agentKind === "model")
    .sort((left, right) => lineIndex(left) - lineIndex(right)));
  const namedAgents = $derived(familyAgents
    .filter((agent) => agent.agentKind === "custom")
    .sort((left, right) => left.participant.displayName.localeCompare(right.participant.displayName, localization.locale)));
  const provider = $derived(chat.settings?.providerInstances.find((entry) => (
    entry.configuration.instanceId === instanceId
  )) ?? null);
  const catalog = $derived(provider?.modelCatalog ?? null);
  /** A named agent keeps one model, so a choice left on the recommendation is frozen to the model recommended now. */
  const chosenModel = $derived(resolveRecommendedSelection(provider, selection));
  const nameProblem = $derived(agentNameProblem(
    name,
    [
      ...chat.agentIdentities.map((agent) => agent.participant.displayName),
      ...(chat.settings?.providerFamilies ?? []).flatMap((family) => [family.familyId, family.displayName]),
    ],
  ));
  const visibleProblem = $derived(nameTouched || name.trim() ? nameProblem : null);

  function emptySelection(): ModelSelection {
    return { providerInstanceId: instanceId, modelId: null, providerManaged: true, options: [] };
  }

  function lineIndex(agent: ChatAiAgentRead): number {
    const index = lines.indexOf(agent.participant.displayName.toLowerCase());
    return index === -1 ? lines.length : index;
  }

  function problemMessage(problem: AgentNameProblem): string {
    if (problem === "tooLong") {
      return t("settings.agents.roster.nameProblems.tooLong", formatNumber(localization.locale, MAX_AGENT_NAME_CHARACTERS));
    }
    return t(`settings.agents.roster.nameProblems.${problem}`);
  }

  function agentDescription(agent: ChatAiAgentRead): string {
    if (agent.agentKind === "model") return t("chat.agentAccess.newestOfLine", agent.participant.displayName);
    return t("chat.agentAccess.fixedModel", formatModelDisplayName(chatAgentDefaultModelName(t, agent.latestPolicy, catalog)));
  }

  async function setEnabled(agent: ChatAiAgentRead, enabled: boolean): Promise<void> {
    if (busyAgentId) return;
    busyAgentId = agent.participant.id;
    error = null;
    try {
      await chatApi.archiveChatAgent(agent.participant.id, agent.participant.revision, !enabled);
      await chat.refreshAgents();
    } catch (cause: unknown) {
      error = chatErrorMessage(cause, t("settings.agents.roster.toggleFailed"));
    } finally {
      busyAgentId = null;
    }
  }

  function closeForm(): void {
    adding = false;
    name = "";
    nameTouched = false;
    selection = emptySelection();
  }

  async function create(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    nameTouched = true;
    if (nameProblem || creating) return;
    creating = true;
    error = null;
    try {
      await chatApi.createChatAgent({
        displayName: name.trim(),
        providerFamilyId: familyId,
        policy: {
          providerManagedModel: chosenModel.providerManaged,
          modelId: chosenModel.modelId,
          modelOptions: chosenModel.options.map((option) => ({ ...option })),
          providerOptions: { schemaVersion: 1, value: {} },
          interactionMode: "build",
        },
      });
      await chat.refreshAgents();
      closeForm();
    } catch (cause: unknown) {
      error = chatErrorMessage(cause, t("settings.agents.roster.createFailed"));
    } finally {
      creating = false;
    }
  }
</script>

{#snippet agentRow(agent: ChatAiAgentRead)}
  <SwitchField
    label={`@${agent.participant.displayName}`}
    description={agentDescription(agent)}
    checked={agent.participant.archivedAt === null}
    disabled={busyAgentId !== null}
    onChange={(enabled) => void setEnabled(agent, enabled)}
  />
{/snippet}

<section class="grid gap-1.5" aria-label={t("settings.agents.roster.heading")}>
  <h4 class="text-[0.8rem] font-semibold">{t("settings.agents.roster.heading")}</h4>
  <p class="m-0 text-panel-detail text-muted-foreground">{t("settings.agents.roster.hint")}</p>
  {#each modelAgents as agent (agent.participant.id)}{@render agentRow(agent)}{/each}
  {#each namedAgents as agent (agent.participant.id)}{@render agentRow(agent)}{/each}

  {#if adding}
    <form class="grid gap-2 pt-1" onsubmit={(event) => void create(event)}>
      <label class="grid gap-1">
        <span class="text-panel-detail text-muted-foreground">{t("settings.agents.roster.nameLabel")}</span>
        <span class="field flex min-w-0 items-center gap-0.5 px-2.5 py-1.5">
          <span class="text-muted-foreground">@</span>
          <input
            class="field-bare h-7 text-[0.866667rem] text-foreground"
            bind:value={name}
            placeholder={t("settings.agents.roster.namePlaceholder")}
            maxlength={MAX_AGENT_NAME_CHARACTERS}
            autocomplete="off"
            spellcheck="false"
            disabled={creating}
            aria-invalid={visibleProblem !== null}
            onblur={() => { nameTouched = true; }}
          />
        </span>
      </label>
      {#if visibleProblem}<p class="m-0 text-panel-detail text-destructive">{problemMessage(visibleProblem)}</p>{/if}
      <div class="grid gap-1">
        <span class="text-panel-detail text-muted-foreground">{t("settings.agents.roster.modelLabel")}</span>
        <ChatModelControls value={selection} lockProvider disabled={creating} onChange={(next) => { selection = next; }} />
      </div>
      <div class="flex flex-wrap justify-end gap-2">
        <button type="button" class={SETTINGS_SECONDARY_BUTTON_CLASS} disabled={creating} onclick={closeForm}>{t("settings.agents.roster.cancel")}</button>
        <button type="submit" class={SETTINGS_PRIMARY_BUTTON_CLASS} disabled={creating || (nameTouched && nameProblem !== null)}>{t("settings.agents.roster.create")}</button>
      </div>
    </form>
  {:else if canCreate}
    <div>
      <button type="button" class={SETTINGS_SECONDARY_BUTTON_CLASS} onclick={() => { adding = true; }}>
        <Plus size={13} />
        {t("settings.agents.roster.add")}
      </button>
    </div>
  {/if}
  {#if error}<Notice class="text-panel-detail" message={error} copy={false} />{/if}
</section>
