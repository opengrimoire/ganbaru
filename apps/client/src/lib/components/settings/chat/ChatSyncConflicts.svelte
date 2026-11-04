<script lang="ts">
  import { onMount } from "svelte";
  import {
    loadChatConflicts,
    resolveChatConflict,
    type ChatConflict,
    type ChatConflictChoice,
    type ChatConflictVersion,
  } from "$lib/api/chat/conflicts";
  import { listenSyncApplied } from "$lib/api/sync";
  import SyncConflictCard from "$lib/components/sync/SyncConflictCard.svelte";
  import SyncConflictChoice from "$lib/components/sync/SyncConflictChoice.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { chatErrorCode } from "$lib/chat/error-presentation";
  import { notifyChatSyncChanged } from "$lib/chat/sync-events";
  import { CHAT_SYNC_TABLES } from "$lib/chat/sync-presentation";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { previewVersionText } from "$lib/projects/conflicts";
  import { formatSyncTime, syncDeviceName } from "$lib/sync/status";

  /**
   * Chat rows edited on two linked devices, each with its versions to choose from. Renders nothing while there are
   * none.
   */
  const localization = getLocalization();
  const { t } = localization;
  let conflicts = $state<ChatConflict[]>([]);
  let loadError = $state<string | null>(null);
  /** The `target:id:version` whose choice is running, or `null` when none is. */
  let pendingKey = $state<string | null>(null);
  const busy = $derived(pendingKey !== null);
  /** Outcome of the last choice, by `target:id`. */
  let messages = $state<Record<string, { text: string; kind: "resolved" | "error" }>>({});
  let loadRequest = 0;

  onMount(() => {
    void load();
    const stop = listenSyncApplied((tables) => {
      if (tables.some((table) => CHAT_SYNC_TABLES.includes(table))) void load();
    });
    return () => {
      void stop.then((unlisten) => unlisten()).catch((error: unknown) => {
        console.warn("Chat conflicts could not stop listening for sync", error);
      });
    };
  });

  async function load(): Promise<void> {
    const request = ++loadRequest;
    try {
      const next = await loadChatConflicts();
      if (request !== loadRequest) return;
      conflicts = next;
      loadError = null;
    } catch (error: unknown) {
      if (request !== loadRequest) return;
      console.warn("Chat conflicts could not be loaded", error);
      loadError = t("chat.conflict.loadFailed");
    }
  }

  /** Reloads the conflicts and rejects when the load fails again. */
  async function retryLoad(): Promise<void> {
    await load();
    if (loadError) throw new Error(loadError);
  }

  function rowKey(conflict: ChatConflict): string {
    return `${conflict.target}:${conflict.id}`;
  }

  function title(conflict: ChatConflict): string {
    switch (conflict.target) {
      case "participant": return t("chat.conflict.participantTitle");
      case "teammate": return t("chat.conflict.teammateTitle");
      case "channel": return t("chat.conflict.channelTitle");
      case "message": return t("chat.conflict.messageTitle");
    }
  }

  function source(version: ChatConflictVersion): string {
    return t(
      "chat.conflict.versionFrom",
      syncDeviceName(version.device, t("chat.conflict.thisDevice"), t("chat.conflict.otherDevice")),
      formatSyncTime(t, localization.locale, version.editedAtMs, Date.now()),
    );
  }

  async function resolve(conflict: ChatConflict, version: ChatConflictVersion): Promise<void> {
    if (busy) return;
    const key = rowKey(conflict);
    const choice: ChatConflictChoice = version.displayed
      ? { kind: "displayed" }
      : { kind: "version", version: version.version };
    pendingKey = `${key}:${version.version}`;
    messages = Object.fromEntries(Object.entries(messages).filter(([entry]) => entry !== key));
    try {
      await resolveChatConflict({ target: conflict.target, id: conflict.id, choice });
      notifyChatSyncChanged();
    } catch (error: unknown) {
      if (chatErrorCode(error) === "not_found") {
        messages = { ...messages, [key]: { text: t("chat.conflict.resolved"), kind: "resolved" } };
      } else {
        console.warn("Chat conflict could not be resolved", error);
        messages = { ...messages, [key]: { text: t("chat.conflict.failed"), kind: "error" } };
      }
    } finally {
      pendingKey = null;
      await load();
    }
  }
</script>

{#if loadError}
  <div class="text-sm"><Notice message={loadError} onRetry={retryLoad} /></div>
{:else if conflicts.length > 0}
  <div class="flex flex-col gap-2" data-chat-settings-conflicts>
    {#each conflicts as conflict (rowKey(conflict))}
      {@const message = messages[rowKey(conflict)]}
      <SyncConflictCard
        title={title(conflict)}
        hint={conflict.context === null ? t("chat.conflict.hint") : t("chat.conflict.messageIn", conflict.context)}
        message={message?.text ?? ""}
        messageKind={message?.kind ?? "error"}
      >
        {#each conflict.versions as version (version.version)}
          <div class="flex min-w-0 flex-col gap-1 rounded-md bg-black/5 px-2 py-1.5 dark:bg-white/5">
            <div class="max-h-32 overflow-y-auto whitespace-pre-wrap wrap-anywhere">
              {#if version.text.trim()}
                {previewVersionText(version.text)}
              {:else}
                <span class="text-muted-foreground">{t("chat.conflict.empty")}</span>
              {/if}
            </div>
            {#if version.detail?.trim()}
              <div class="max-h-24 overflow-y-auto whitespace-pre-wrap text-muted-foreground wrap-anywhere">
                {previewVersionText(version.detail)}
              </div>
            {/if}
            <div class="flex flex-wrap items-baseline gap-x-2 text-muted-foreground">
              <span>{source(version)}</span>
              {#if version.displayed}<span>{t("chat.conflict.shown")}</span>{/if}
            </div>
            <div class="flex flex-wrap items-center gap-1">
              <SyncConflictChoice
                disabled={busy}
                pending={pendingKey === `${rowKey(conflict)}:${version.version}`}
                onclick={() => void resolve(conflict, version)}
              >{version.displayed ? t("chat.conflict.keepThis") : t("chat.conflict.useOther")}</SyncConflictChoice>
            </div>
          </div>
        {/each}
      </SyncConflictCard>
    {/each}
  </div>
{/if}
