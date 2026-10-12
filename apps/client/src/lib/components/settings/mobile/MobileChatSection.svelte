<script lang="ts">
  import { onMount } from "svelte";
  import type { ChatBehaviorPreferences } from "$lib/chat/contracts";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getChat } from "$lib/stores/chat.svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import SwitchField from "$lib/components/ui/SwitchField.svelte";

  const chat = getChat();
  const { t } = getLocalization();
  const behavior = $derived(chat.settings?.configuration.behavior ?? null);
  let loading = $state(!chat.loaded);
  let saving = $state(false);
  let error = $state<string | null>(null);

  onMount(() => {
    void initialize();
  });

  async function initialize(): Promise<void> {
    loading = true;
    error = null;
    try {
      await chat.ensureLoaded();
    } catch (cause: unknown) {
      error = chatErrorMessage(cause, t("settings.chat.loadFailed"));
    } finally {
      loading = false;
    }
  }

  /** Retries loading Chat settings and rejects when the load fails again. */
  async function retryInitialize(): Promise<void> {
    await initialize();
    if (error) throw new Error(error);
  }

  async function update(patch: Partial<ChatBehaviorPreferences>): Promise<void> {
    if (!behavior || saving) return;
    saving = true;
    error = null;
    try {
      await chat.updateBehavior({ ...behavior, ...patch });
    } catch (cause: unknown) {
      error = chatErrorMessage(cause, t("settings.chat.loadFailed"));
    } finally {
      saving = false;
    }
  }
</script>

<div class="flex flex-col gap-6">
  <section class="flex flex-col gap-4">
    <h2 class="px-1 text-[0.866667rem] font-semibold text-foreground">
      {t("settings.chat.behavior.heading")}
    </h2>

    {#if loading}
      <Loading layout="rows" rows={3} />
    {:else if behavior}
      <div class="flex flex-col gap-3">
        <SwitchField
          label={t("settings.chat.behavior.restoreThread")}
          description={t("settings.chat.behavior.restoreThreadDescription")}
          checked={behavior.restoreLastSelectedThread}
          disabled={saving}
          onChange={(value) => void update({ restoreLastSelectedThread: value })}
        />
        <SwitchField
          label={t("settings.chat.behavior.reasoning")}
          description={t("settings.chat.behavior.reasoningDescription")}
          checked={behavior.showReasoningSummaries}
          disabled={saving}
          onChange={(value) => void update({ showReasoningSummaries: value })}
        />
        <SwitchField
          label={t("settings.chat.behavior.foldWork")}
          description={t("settings.chat.behavior.foldWorkDescription")}
          checked={behavior.automaticallyFoldSettledWork}
          disabled={saving}
          onChange={(value) => void update({ automaticallyFoldSettledWork: value })}
        />
      </div>
    {/if}

    {#if error}
      <Notice class="px-1 text-[0.8rem]" message={error} onRetry={behavior ? undefined : retryInitialize} />
    {/if}
  </section>
</div>
