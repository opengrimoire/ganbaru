<script lang="ts">
  import Square from "@lucide/svelte/icons/square";
  import type { ChatAgentRunRead, ChatMessageRead, ChatTimelinePageRead } from "$lib/chat/contracts";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import { agentRunIsActive, agentRunOutcome, agentRunShowedOutput } from "$lib/chat/timeline/reply-thread-model";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { formatShortcut } from "$lib/keyboard-shortcuts";
  import { useChat } from "$lib/stores/chat.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";

  /**
   * Closes a run in a reply thread: a Stop working control while the run works, or how the run ended when its exact
   * turn alone would not show it, such as a run that failed before any output. The explanation is localized from the
   * stored error code, and the diagnostic text is only reachable through the copy button.
   */
  let {
    run,
    teammateName,
    timelinePage,
    replies,
    fallbackDetail = null,
    onActionError,
  }: {
    run: ChatAgentRunRead;
    /** Display name of the teammate that ran, or `null` when the thread does not name one. */
    teammateName: string | null;
    /** The run's exact provider turn page, when it had one and it is loaded. */
    timelinePage: ChatTimelinePageRead | null;
    replies: readonly ChatMessageRead[];
    /** Diagnostic text to offer when the run's turn has none, such as the assignment's state reason. */
    fallbackDetail?: string | null;
    /** Reports a failed stop request. */
    onActionError: (error: unknown) => void;
  } = $props();

  const chat = useChat();
  const { t } = getLocalization();
  const stopShortcut = formatShortcut("Mod + .");
  let stopping = $state(false);
  const turn = $derived(timelinePage?.turns.find((entry) => entry.turnId === run.providerExecutionTurnId) ?? null);
  const outcome = $derived(agentRunOutcome(
    run,
    turn,
    agentRunShowedOutput(run, timelinePage, replies),
    fallbackDetail,
  ));
  const name = $derived(teammateName ?? t("chat.runs.outcome.teammateFallback"));

  async function stop(): Promise<void> {
    if (stopping) return;
    stopping = true;
    try {
      await chat.cancelAssignment(run.assignmentId);
    } catch (cause: unknown) {
      onActionError(cause);
    } finally {
      stopping = false;
    }
  }
</script>

{#if agentRunIsActive(run)}
  <div class="run-outcome run-stop">
    <button
      type="button"
      data-action="stop-run"
      disabled={stopping}
      aria-busy={stopping || undefined}
      title={t("chat.runs.stopWorkingShortcut", name, stopShortcut)}
      onclick={() => { void stop(); }}
    ><Square size={12} aria-hidden="true" />{t("chat.runs.stopWorking")}</button>
  </div>
{:else if outcome?.kind === "failed"}
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
  .run-stop button { display:inline-flex; min-height:1.75rem; align-items:center; gap:0.35rem; border-radius:var(--floating-item-radius); background:var(--accent); padding:0.25rem 0.55rem; color:var(--foreground); font-size:var(--panel-detail-font-size); }
  .run-stop button:hover:not(:disabled) { background:color-mix(in srgb,var(--destructive) 14%,var(--accent)); color:var(--destructive); }
  .run-stop button:disabled { opacity:0.45; }
  @media (pointer:coarse) { .run-stop button { min-height:2.75rem; padding-inline:0.85rem; } }
</style>
