<script lang="ts">
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import ShieldAlert from "@lucide/svelte/icons/shield-alert";
  import { resolveChatRemoteApproval } from "$lib/api/chat";
  import type { AnswerableRemoteApproval, ChatRemoteApprovalChoice } from "$lib/chat/timeline/agent-update";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import ChatApprovalSubject from "./ChatApprovalSubject.svelte";

  /**
   * The decisions for an approval that work a linked desktop runs holds open, sent to that desktop. The approval
   * message itself stays in the thread; this panel only answers it.
   */
  let {
    approval,
    agentName,
    onAnswered,
  }: {
    approval: AnswerableRemoteApproval;
    agentName: string;
    /** Hides the approval once the desktop resolved it or it was no longer open there. */
    onAnswered: (requestId: string) => void;
  } = $props();

  const { t } = getLocalization();
  let resolving = $state(false);
  let error = $state<string | null>(null);

  async function answer(choice: ChatRemoteApprovalChoice): Promise<void> {
    if (resolving) return;
    resolving = true;
    error = null;
    const requestId = approval.requestId;
    try {
      const outcome = await resolveChatRemoteApproval({
        assignmentId: approval.assignmentId,
        requestId,
        decision: choice.decision,
        providerOptionId: choice.id,
        commandId: crypto.randomUUID(),
      });
      switch (outcome.kind) {
        case "resolved":
        case "stale":
          onAnswered(requestId);
          break;
        case "refused":
          error = t("chat.remoteApproval.refused");
          break;
        case "notServed":
          error = t("chat.remoteApproval.notServed");
          break;
        case "unreachable":
          error = t("chat.remoteApproval.unreachable");
          break;
      }
    } catch (cause: unknown) {
      error = chatErrorMessage(cause, t("chatErrors.actions.requestAction"));
    } finally {
      resolving = false;
    }
  }
</script>

<section class="remote-approval" data-remote-approval>
  <header><ShieldAlert size={16} /><strong>{t("chat.remoteApproval.title", agentName)}</strong></header>
  {#if approval.subject}
    <ChatApprovalSubject subject={approval.subject} />
    {#if approval.subject.truncated}<p class="no-subject">{t("chat.remoteApproval.truncatedSubject")}</p>{/if}
  {:else}
    <p class="no-subject">{t("chat.remoteApproval.noSubject")}</p>
  {/if}
  <div class="request-actions">
    {#each approval.choices as choice (choice.id)}
      <button type="button" disabled={resolving} onclick={() => void answer(choice)}>{choice.label}</button>
    {/each}
  </div>
  {#if resolving}<p class="resolving"><LoaderCircle size={13} class="animate-spin" />{t("chat.composer.resolving")}</p>{/if}
  {#if error}<Notice class="text-xs" message={error} />{/if}
</section>

<style>
  .remote-approval { display: grid; gap: 0.65rem; border: 1px solid color-mix(in oklab, var(--status-tentative) 50%, var(--border)); border-radius: 0.5rem; background: var(--background); padding: 0.7rem; }
  header { display: flex; align-items: center; gap: 0.5rem; }
  header strong { min-width: 0; font-size: calc(0.8rem * var(--type-scale)); overflow-wrap: anywhere; }
  .no-subject { color: var(--muted-foreground); font-size: calc(0.7rem * var(--type-scale)); }
  .request-actions { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: 0.4rem; }
  .request-actions button { min-height: 2.75rem; border: 1px solid var(--border); border-radius: 0.35rem; padding: 0.3rem 0.75rem; font-size: calc(0.8rem * var(--type-scale)); }
  .request-actions button:hover:not(:disabled) { background: var(--accent); }
  .resolving { display: inline-flex; align-items: center; gap: 0.35rem; color: var(--muted-foreground); font-size: calc(0.7rem * var(--type-scale)); }
</style>
