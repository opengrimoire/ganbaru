<script lang="ts">
  import * as chatApi from "$lib/api/chat";
  import type { ChatAgentPolicyInput, ModelOptionSelection, ProviderInstanceId } from "$lib/chat/contracts";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import ChatModelControls from "$lib/components/chat/composer/ChatModelControls.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getChat } from "$lib/stores/chat.svelte";

  /**
   * The default model and model options of the agent this device runs through one provider installation. Channels
   * can override the model; everything else about the agent is set per app, project, or channel.
   */
  let { instanceId }: { instanceId: ProviderInstanceId } = $props();

  const { t } = getLocalization();
  const chat = getChat();
  let saving = $state(false);
  let error = $state<string | null>(null);

  const agent = $derived(chat.agents.find((entry) => entry.localProviderInstanceId === instanceId) ?? null);
  const policy = $derived(agent?.latestPolicy ?? null);
  const value = $derived({
    providerInstanceId: instanceId,
    modelId: policy?.modelId ?? null,
    providerManaged: policy?.providerManagedModel ?? true,
    options: policy?.modelOptions ?? [],
  });

  async function choose(selection: {
    providerInstanceId: string | null;
    modelId: string | null;
    providerManaged: boolean;
    options: ModelOptionSelection[];
  }): Promise<void> {
    const current = agent;
    if (!current || saving || selection.providerInstanceId !== instanceId) return;
    const next: ChatAgentPolicyInput = {
      providerManagedModel: selection.providerManaged,
      modelId: selection.modelId,
      modelOptions: selection.options.map((option) => ({ ...option })),
      providerOptions: policy?.providerOptions ?? { schemaVersion: 1, value: {} },
      interactionMode: policy?.interactionMode ?? "build",
    };
    saving = true;
    error = null;
    try {
      await chatApi.setChatAgentPolicy({ agentId: current.participant.id, policy: next });
      await chat.refreshAgents();
    } catch (cause: unknown) {
      error = chatErrorMessage(cause, t("settings.agents.defaultModel.saveFailed"));
    } finally {
      saving = false;
    }
  }
</script>

{#if agent}
  <section class="grid gap-1.5" aria-label={t("settings.agents.defaultModel.heading")}>
    <h4 class="text-[0.8rem] font-semibold">{t("settings.agents.defaultModel.heading")}</h4>
    <p class="m-0 text-panel-detail text-muted-foreground">{t("settings.agents.defaultModel.hint", agent.participant.displayName)}</p>
    <ChatModelControls {value} lockProvider disabled={saving} onChange={(selection) => void choose(selection)} />
    {#if error}<Notice class="text-panel-detail" message={error} copy={false} />{/if}
  </section>
{/if}
