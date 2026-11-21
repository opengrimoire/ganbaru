<script lang="ts">
  import * as chatApi from "$lib/api/chat";
  import type { ChatChannelRememberedApprovalRead, ChatParticipantId } from "$lib/chat/contracts";
  import { rememberedApprovalSubject } from "$lib/chat/agents/remembered-approvals";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";

  /**
   * A channel's remembered approvals, which a person makes by choosing "Allow and don't ask again" on an
   * agent request. Revoking one makes the next matching request ask again on every linked device; `onChanged`
   * rereads the list.
   */
  let {
    approvals,
    agentName,
    onChanged,
  }: {
    approvals: readonly ChatChannelRememberedApprovalRead[];
    agentName: (agentId: ChatParticipantId) => string;
    onChanged: () => Promise<void>;
  } = $props();

  const { t } = getLocalization();
  let revokeError = $state<string | null>(null);
  let revoking = $state<string | null>(null);

  async function revoke(approval: ChatChannelRememberedApprovalRead): Promise<void> {
    if (revoking) return;
    revoking = approval.approvalId;
    revokeError = null;
    try {
      await chatApi.revokeChatRememberedApproval(approval.approvalId);
      await onChanged();
    } catch (cause: unknown) {
      revokeError = chatErrorMessage(cause, t("chat.agentAccess.revokeRememberedApprovalFailed"));
    } finally {
      revoking = null;
    }
  }
</script>

<div class="remembered-approvals">
  <ul>
    {#each approvals as approval (approval.approvalId)}
      {@const subject = rememberedApprovalSubject(approval)}
      <li>
        <div class="remembered-approval-text">
          <strong>{agentName(approval.agentId)}</strong>
          <span class:command={approval.kind === "command"}>{subject}</span>
        </div>
        <button
          type="button"
          class="remembered-approval-revoke"
          disabled={revoking !== null}
          aria-busy={revoking === approval.approvalId}
          aria-label={t("chat.agentAccess.revokeRememberedApproval", agentName(approval.agentId), subject)}
          onclick={() => void revoke(approval)}
        >{t("chat.agentAccess.revokePushApprovalAction")}</button>
      </li>
    {/each}
  </ul>
  {#if revokeError}<Notice message={revokeError} copy={false} />{/if}
</div>

<style>
  .remembered-approvals,
  .remembered-approvals ul {
    display: grid;
    gap: 0.4rem;
  }

  .remembered-approvals ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .remembered-approvals li {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    min-width: 0;
  }

  .remembered-approval-text {
    display: grid;
    flex: 1;
    min-width: 0;
    font-size: calc(0.733333rem * var(--type-scale));
  }

  .remembered-approval-text strong {
    font-weight: 600;
    overflow-wrap: anywhere;
  }

  .remembered-approval-revoke {
    flex-shrink: 0;
    min-height: 1.9rem;
    border: 1px solid var(--border);
    border-radius: var(--floating-item-radius);
    padding: 0.2rem 0.7rem;
    font-size: calc(0.733333rem * var(--type-scale));
  }

  .remembered-approval-revoke:hover:not(:disabled) { background: var(--accent); }
  .remembered-approval-revoke:disabled { cursor: not-allowed; opacity: 0.5; }

  @media (pointer: coarse) {
    .remembered-approval-revoke { min-height: 2.75rem; }
  }

  .remembered-approval-text span {
    color: var(--muted-foreground);
    overflow-wrap: anywhere;
  }

  .remembered-approval-text .command {
    font-family: var(--font-mono, ui-monospace, monospace);
  }
</style>
