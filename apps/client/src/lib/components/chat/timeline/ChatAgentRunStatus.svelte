<script lang="ts">
  import type { ChatAgentRunRead, ChatMessageRead, ChatTimelinePageRead } from "$lib/chat/contracts";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import { chatFailureTechnicalDetail } from "$lib/chat/timeline/agent-update";
  import { agentRunIsActive, agentRunOutcome, agentRunShowedOutput } from "$lib/chat/timeline/reply-thread-model";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import ChatStopWorkButton from "./ChatStopWorkButton.svelte";

  /**
   * Closes a run in a reply thread: a Stop working control while the run works, or how the run ended when its exact
   * turn alone would not show it, such as a run that failed before any output. The explanation is localized from the
   * stored error code, and the diagnostic text is only reachable through the copy button.
   */
  let {
    run,
    agentName,
    timelinePage,
    replies,
    fallbackDetail = null,
    failureShownElsewhere = false,
    onRetryFailure,
    onActionError,
  }: {
    run: ChatAgentRunRead;
    /** Display name of the agent that ran, or `null` when the thread does not name one. */
    agentName: string | null;
    /** The run's exact provider turn page, when it had one and it is loaded. */
    timelinePage: ChatTimelinePageRead | null;
    replies: readonly ChatMessageRead[];
    /** Diagnostic text to offer when the run's turn has none, such as the assignment's state reason. */
    fallbackDetail?: string | null;
    /** Whether a visible failure message of the agent already explains how the run failed. */
    failureShownElsewhere?: boolean;
    /** Retries the failed work, offered on the status line that still stands for it. */
    onRetryFailure?: () => unknown;
    /** Reports a failed stop request. */
    onActionError: (error: unknown) => void;
  } = $props();

  const { t } = getLocalization();
  const turn = $derived(timelinePage?.turns.find((entry) => entry.turnId === run.providerExecutionTurnId) ?? null);
  const outcome = $derived(agentRunOutcome(
    run,
    turn,
    agentRunShowedOutput(run, timelinePage, replies),
    fallbackDetail,
  ));
  const name = $derived(agentName ?? t("chat.runs.outcome.agentFallback"));
</script>

{#if agentRunIsActive(run)}
  <div class="run-outcome run-stop">
    <ChatStopWorkButton assignmentId={run.assignmentId} agentName={name} {onActionError} />
  </div>
{:else if outcome?.kind === "failed" && !failureShownElsewhere}
  <div class="run-outcome" data-run-outcome="failed">
    <Notice
      title={outcome.beforeOutput ? t("chat.runs.outcome.failedBeforeOutput", name) : t("chat.runs.outcome.failedAfterOutput", name)}
      message={outcome.errorCode ? chatErrorMessage({ code: outcome.errorCode }) : t("chatErrors.generic")}
      copy={chatFailureTechnicalDetail(outcome.errorCode, outcome.detail) ?? false}
      onRetry={onRetryFailure}
      retryLabel={onRetryFailure ? t("chat.organization.retry") : undefined}
      role="status"
    />
  </div>
{:else if outcome?.kind === "stopped_before_output"}
  <p class="run-outcome run-outcome-stopped" role="status" data-run-outcome="stopped">{t("chat.runs.outcome.stoppedBeforeOutput", name)}</p>
{/if}

<style>
  .run-outcome { margin:0.25rem 0.75rem; }
  .run-outcome[data-run-outcome="failed"] { font-size:var(--chat-organizational-font-size,calc(0.875rem * var(--type-scale))); }
  .run-outcome[data-run-outcome="failed"] :global(.notice) { line-height:var(--chat-organizational-line-height,calc(1.3125rem * var(--type-scale))); }
  .run-outcome-stopped { color:var(--muted-foreground); font-size: calc(0.65rem * var(--type-scale)); }
  /* Lines up with the message text column: the row inset, the 34px avatar, and the avatar gap. */
  .run-stop { margin-left:calc(var(--chat-message-row-padding-inline,1rem) + 34px + 0.65rem); }
</style>
