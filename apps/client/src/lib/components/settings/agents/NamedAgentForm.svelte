<script lang="ts">
  import * as chatApi from "$lib/api/chat";
  import { agentNameProblem, MAX_AGENT_NAME_CHARACTERS, type AgentNameProblem } from "$lib/chat/agents/agent-names";
  import { resolveAgentSelection } from "$lib/chat/composer/model-picker-model";
  import type { ModelOptionSelection, ProviderFamilyId, ProviderInstanceId } from "$lib/chat/contracts";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import ChatModelControls from "$lib/components/chat/composer/ChatModelControls.svelte";
  import { SETTINGS_PRIMARY_BUTTON_CLASS, SETTINGS_SECONDARY_BUTTON_CLASS } from "$lib/components/settings/action-buttons";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getChat } from "$lib/stores/chat.svelte";

  /**
   * Adds an agent a person names, such as `@Reviewer`, that always runs one model of the provider with the options
   * chosen here. The effort starts on medium, else the model's highest.
   */
  let {
    familyId,
    instanceId,
    onClose,
  }: { familyId: ProviderFamilyId; instanceId: ProviderInstanceId; onClose: () => void } = $props();

  interface ModelSelection {
    providerInstanceId: string | null;
    modelId: string | null;
    providerManaged: boolean;
    options: ModelOptionSelection[];
  }

  const localization = getLocalization();
  const { t } = localization;
  const chat = getChat();
  let creating = $state(false);
  let error = $state<string | null>(null);
  let name = $state("");
  let nameTouched = $state(false);
  let chosen = $state<ModelSelection | null>(null);

  const provider = $derived(chat.settings?.providerInstances.find((entry) => (
    entry.configuration.instanceId === instanceId
  )) ?? null);
  const selection = $derived(resolveAgentSelection(
    provider,
    chosen ?? { providerInstanceId: instanceId, modelId: null, providerManaged: true, options: [] },
  ));
  const nameProblem = $derived(agentNameProblem(
    name,
    [
      ...chat.agentIdentities.map((agent) => agent.participant.displayName),
      ...(chat.settings?.providerFamilies ?? []).flatMap((family) => [family.familyId, family.displayName]),
    ],
  ));
  const visibleProblem = $derived(nameTouched || name.trim() ? nameProblem : null);

  function problemMessage(problem: AgentNameProblem): string {
    if (problem === "tooLong") {
      return t("settings.agents.roster.nameProblems.tooLong", formatNumber(localization.locale, MAX_AGENT_NAME_CHARACTERS));
    }
    return t(`settings.agents.roster.nameProblems.${problem}`);
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
          providerManagedModel: selection.providerManaged,
          modelId: selection.modelId,
          modelOptions: selection.options.map((option) => ({ ...option })),
          providerOptions: { schemaVersion: 1, value: {} },
          interactionMode: "build",
        },
      });
      await chat.refreshAgents();
      onClose();
    } catch (cause: unknown) {
      error = chatErrorMessage(cause, t("settings.agents.roster.createFailed"));
    } finally {
      creating = false;
    }
  }
</script>

<form class="grid gap-2 border-b border-border px-1 py-2.5" onsubmit={(event) => void create(event)}>
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
    <ChatModelControls value={selection} lockProvider disabled={creating} onChange={(next) => { chosen = next; }} />
  </div>
  <div class="flex flex-wrap justify-end gap-2">
    <button type="button" class={SETTINGS_SECONDARY_BUTTON_CLASS} disabled={creating} onclick={onClose}>{t("settings.agents.roster.cancel")}</button>
    <button type="submit" class={SETTINGS_PRIMARY_BUTTON_CLASS} disabled={creating || (nameTouched && nameProblem !== null)}>{t("settings.agents.roster.create")}</button>
  </div>
  {#if error}<Notice class="text-panel-detail" message={error} copy={false} />{/if}
</form>
