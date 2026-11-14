<script lang="ts">
  import type { ChatAgentRunRead, ChatMessageRead, ChatTimelinePageRead } from "$lib/chat/contracts";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import { agentRunOutcome, agentRunShowedOutput } from "$lib/chat/timeline/reply-thread-model";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";

  /**
   * Shows how a run in a reply thread ended when its exact turn alone would not show it, such as a run that failed
   * before any output. The explanation is localized from the stored error code, and the diagnostic text is only
   * reachable through the copy button.
   */
  let {
    run,
    teammateName,
    timelinePage,
    replies,
    fallbackDetail = null,
  }: {
    run: ChatAgentRunRead;
    /** Display name of the teammate that ran, or `null` when the thread does not name one. */
    teammateName: string | null;
    /** The run's exact provider turn page, when it had one and it is loaded. */
    timelinePage: ChatTimelinePageRead | null;
    replies: readonly ChatMessageRead[];
    /** Diagnostic text to offer when the run's turn has none, such as the assignment's state reason. */
    fallbackDetail?: string | null;
  } = $props();

  const { t } = getLocalization();
  const turn = $derived(timelinePage?.turns.find((entry) => entry.turnId === run.providerExecutionTurnId) ?? null);
  const outcome = $derived(agentRunOutcome(
    run,
    turn,
    agentRunShowedOutput(run, timelinePage, replies),
    fallbackDetail,
  ));
  const name = $derived(teammateName ?? t("chat.runs.outcome.teammateFallback"));
</script>

{#if outcome?.kind === "failed"}
  <div class="run-outcome" data-run-outcome="failed">
    <Notice
      title={outcome.beforeOutput ? t("chat.runs.outcome.failedBeforeOutput", name) : t("chat.runs.outcome.failedAfterOutput", name)}
      message={outcome.errorCode ? chatErrorMessage({ code: outcome.errorCode }) : t("chatErrors.generic")}
      copy={outcome.detail ?? undefined}
      role="status"
    />
  </div>
{:else if outcome?.kind === "stopped_before_output"}
  <p class="run-outcome run-outcome-stopped" role="status" data-run-outcome="stopped">{t("chat.runs.outcome.stoppedBeforeOutput", name)}</p>
{/if}

<style>
  .run-outcome { margin:0.25rem 0.75rem; }
  .run-outcome-stopped { color:var(--muted-foreground); font-size: calc(0.65rem * var(--type-scale)); }
</style>
