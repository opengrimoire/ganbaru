<script lang="ts">
  import * as chatApi from "$lib/api/chat";
  import type { ChatProjectAgentSettingsRead } from "$lib/chat/contracts";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { useChat } from "$lib/stores/chat.svelte";
  import ChatAgentScopeSettings, { type ChatAgentScopeValue } from "./ChatAgentScopeSettings.svelte";

  /**
   * A project's agent access and instructions, which its channels inherit until they set their own. The access
   * follows the app settings until the project chooses one.
   */
  let { projectId }: { projectId: string } = $props();

  const chat = useChat();
  const { t } = getLocalization();
  let settings = $state<ChatProjectAgentSettingsRead | null>(null);
  let loadError = $state<string | null>(null);
  let request = 0;

  async function load(id: string): Promise<void> {
    const current = ++request;
    try {
      const read = await chatApi.readChatProjectAgentSettings(id);
      if (current !== request) return;
      settings = read;
      loadError = null;
    } catch (cause: unknown) {
      if (current === request) loadError = chatErrorMessage(cause, t("chat.agentAccess.loadFailed"));
    }
  }

  async function save(value: ChatAgentScopeValue): Promise<void> {
    const current = settings;
    if (!current) return;
    try {
      const read = await chatApi.updateChatProjectAgentSettings({
        projectId: current.projectId,
        access: value.access,
        instructions: value.instructions,
        expectedRevision: current.revision,
      });
      request += 1;
      settings = read;
      chat.agentSettingsChanged(null);
    } catch (cause: unknown) {
      await load(current.projectId);
      throw cause;
    }
  }

  $effect(() => {
    void chat.agentSettingsVersion;
    void load(projectId);
  });
</script>

<section class="grid gap-2" aria-label={t("chat.agentAccess.title")}>
  <span class="scope-heading">{t("chat.agentAccess.title")}</span>
  <p class="scope-hint">{t("chat.agentAccess.projectScopeHint")}</p>
  {#if loadError}
    <Notice class="text-panel-detail" message={loadError} onRetry={() => load(projectId)} copy={false} />
  {:else if settings}
    <ChatAgentScopeSettings
      access={settings.access}
      effective={settings.effective}
      instructions={settings.instructions}
      scope="project"
      idPrefix={`project-${settings.projectId}`}
      onSave={save}
    />
  {/if}
</section>

<style>
  .scope-heading { font-size: var(--panel-detail-font-size); font-weight: 600; line-height: 1.4; }
  .scope-hint { margin: 0; color: var(--muted-foreground); font-size: var(--panel-detail-font-size); line-height: 1.4; }
</style>
