<script lang="ts">
  import * as chatApi from "$lib/api/chat";
  import type { ChatAppAgentSettingsRead } from "$lib/chat/contracts";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import ChatAgentScopeSettings, { type ChatAgentScopeValue } from "$lib/components/chat/agents/ChatAgentScopeSettings.svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getChat } from "$lib/stores/chat.svelte";

  /** The app's default agent access and instructions, which every project and channel inherits. */
  const chat = getChat();
  const { t } = getLocalization();
  let settings = $state<ChatAppAgentSettingsRead | null>(null);
  let loadError = $state<string | null>(null);
  let request = 0;

  async function load(): Promise<void> {
    const current = ++request;
    try {
      const read = await chatApi.readChatAppAgentSettings();
      if (current !== request) return;
      settings = read;
      loadError = null;
    } catch (cause: unknown) {
      if (current === request) loadError = chatErrorMessage(cause, t("chat.agentAccess.loadFailed"));
    }
  }

  async function save(value: ChatAgentScopeValue): Promise<void> {
    const current = settings;
    if (!current || value.access === null) return;
    try {
      const read = await chatApi.updateChatAppAgentSettings({
        access: value.access,
        instructions: value.instructions,
        expectedRevision: current.revision,
      });
      request += 1;
      settings = read;
      chat.agentSettingsChanged(null);
    } catch (cause: unknown) {
      await load();
      throw cause;
    }
  }

  $effect(() => {
    void chat.agentSettingsVersion;
    void load();
  });
</script>

<section class="flex flex-col gap-4" aria-label={t("settings.chat.behavior.agentsHeading")}>
  <div class="grid gap-1 px-1">
    <h2 class="text-[0.866667rem] font-semibold text-foreground">{t("settings.chat.behavior.agentsHeading")}</h2>
    <p class="m-0 text-[0.733333rem] text-muted-foreground">{t("chat.agentAccess.appScopeHint")}</p>
  </div>
  {#if loadError}
    <Notice class="px-1 text-[0.733333rem]" message={loadError} onRetry={load} copy={false} />
  {:else if settings}
    <div class="px-1">
      <ChatAgentScopeSettings
        access={settings.access}
        instructions={settings.instructions}
        markDefault
        idPrefix="app-agent-settings"
        onSave={save}
      />
    </div>
  {:else}
    <Loading layout="rows" rows={2} />
  {/if}
</section>
