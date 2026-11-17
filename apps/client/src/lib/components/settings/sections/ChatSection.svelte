<script lang="ts">
  import { onMount } from "svelte";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getChat } from "$lib/stores/chat.svelte";
  import ChatBehaviorSettings from "$lib/components/settings/chat/ChatBehaviorSettings.svelte";
  import ChatBrowserPreviewSettings from "$lib/components/settings/chat/ChatBrowserPreviewSettings.svelte";
  import ChatDiagnosticsSettings from "$lib/components/settings/chat/ChatDiagnosticsSettings.svelte";
  import ChatQuietPeriodsSettings from "$lib/components/settings/chat/ChatQuietPeriodsSettings.svelte";
  import ChatSyncConflicts from "$lib/components/settings/chat/ChatSyncConflicts.svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";

  const { t } = getLocalization();
  const chat = getChat();
  let initializing = $state(!chat.loaded);
  let loadError = $state<string | null>(null);

  onMount(() => {
    void initialize();
  });

  async function initialize(): Promise<void> {
    initializing = !chat.loaded;
    loadError = null;
    try {
      await chat.ensureLoaded();
    } catch (error: unknown) {
      loadError = chatErrorMessage(error, t("settings.chat.loadFailed"));
    } finally {
      initializing = false;
    }
  }

  /** Retries loading Chat settings and rejects when the load fails again. */
  async function retryInitialize(): Promise<void> {
    await initialize();
    if (loadError) throw new Error(loadError);
  }
</script>

{#snippet separator()}
  <div class="h-px shrink-0 bg-border/50" aria-hidden="true"></div>
{/snippet}

{#if initializing}
  <Loading layout="fill" class="min-h-40" />
{:else if loadError}
  <div class="min-h-40 text-sm"><Notice layout="fill" message={loadError} onRetry={retryInitialize} /></div>
{:else}
  <div class="flex flex-col gap-6 pb-4">
    <ChatSyncConflicts />
    <ChatBehaviorSettings />
    {@render separator()}
    <ChatQuietPeriodsSettings />
    {@render separator()}
    <ChatBrowserPreviewSettings />
    {@render separator()}
    <ChatDiagnosticsSettings />
  </div>
{/if}
