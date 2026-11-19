<script lang="ts">
  import Square from "@lucide/svelte/icons/square";
  import type { ChatWorkAssignmentId } from "$lib/chat/contracts";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { formatShortcut } from "$lib/keyboard-shortcuts";
  import { useChat } from "$lib/stores/chat.svelte";

  /** Stop working control for active work in a reply thread, which cancels the work wherever it runs. */
  let {
    assignmentId,
    agentName,
    onActionError,
  }: {
    assignmentId: ChatWorkAssignmentId;
    /** Display name of the agent that works, for the tooltip. */
    agentName: string;
    /** Reports a failed stop request. */
    onActionError: (error: unknown) => void;
  } = $props();

  const chat = useChat();
  const { t } = getLocalization();
  const stopShortcut = formatShortcut("Mod + .");
  let stopping = $state(false);

  async function stop(): Promise<void> {
    if (stopping) return;
    stopping = true;
    try {
      await chat.cancelAssignment(assignmentId);
    } catch (cause: unknown) {
      onActionError(cause);
    } finally {
      stopping = false;
    }
  }
</script>

<button
  type="button"
  class="stop-work"
  data-action="stop-run"
  disabled={stopping}
  aria-busy={stopping || undefined}
  title={t("chat.runs.stopWorkingShortcut", agentName, stopShortcut)}
  onclick={() => { void stop(); }}
><Square size={12} aria-hidden="true" />{t("chat.runs.stopWorking")}</button>

<style>
  .stop-work { display:inline-flex; min-height:1.75rem; align-items:center; gap:0.35rem; border-radius:var(--floating-item-radius); background:var(--accent); padding:0.25rem 0.55rem; color:var(--foreground); font-size:var(--panel-detail-font-size); }
  .stop-work:hover:not(:disabled) { background:color-mix(in srgb,var(--destructive) 14%,var(--accent)); color:var(--destructive); }
  .stop-work:disabled { opacity:0.45; }
  @media (pointer:coarse) { .stop-work { min-height:2.75rem; padding-inline:0.85rem; } }
</style>
