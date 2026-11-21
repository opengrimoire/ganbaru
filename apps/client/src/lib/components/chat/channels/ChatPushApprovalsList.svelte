<script lang="ts">
  import * as chatApi from "$lib/api/chat";
  import type { ChatChannelPushApprovalRead, ChatParticipantId } from "$lib/chat/contracts";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";

  /**
   * A channel's standing push approvals, which a person makes by choosing not to be asked again on a push
   * request. Revoking one makes the next matching push ask again on every linked device; `onChanged` rereads
   * the channel settings that carry the list.
   */
  let {
    approvals,
    agentName,
    onChanged,
  }: {
    approvals: readonly ChatChannelPushApprovalRead[];
    agentName: (agentId: ChatParticipantId) => string;
    onChanged: () => Promise<void>;
  } = $props();

  const { t } = getLocalization();
  let revokeError = $state<string | null>(null);
  let revoking = $state<string | null>(null);

  async function revoke(approval: ChatChannelPushApprovalRead): Promise<void> {
    if (revoking) return;
    revoking = approval.approvalId;
    revokeError = null;
    try {
      await chatApi.revokeChatPushApproval(approval.approvalId);
      await onChanged();
    } catch (cause: unknown) {
      revokeError = chatErrorMessage(cause, t("chat.agentAccess.revokePushApprovalFailed"));
    } finally {
      revoking = null;
    }
  }
</script>

<div class="push-approvals">
  <ul>
    {#each approvals as approval (approval.approvalId)}
      <li>
        <div class="push-approval-text">
          <strong>{agentName(approval.agentId)} · {approval.workingFolderName}</strong>
          <span>{t("chat.agentAccess.pushApprovalTarget", approval.pushUrl, approval.destinationBranch)}</span>
        </div>
        <button
          type="button"
          class="push-approval-revoke"
          disabled={revoking !== null}
          aria-busy={revoking === approval.approvalId}
          aria-label={t("chat.agentAccess.revokePushApproval", approval.workingFolderName, approval.destinationBranch)}
          onclick={() => void revoke(approval)}
        >{t("chat.agentAccess.revokePushApprovalAction")}</button>
      </li>
    {/each}
  </ul>
  {#if revokeError}<Notice message={revokeError} copy={false} />{/if}
</div>

<style>
  .push-approvals,
  .push-approvals ul {
    display: grid;
    gap: 0.4rem;
  }

  .push-approvals ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .push-approvals li {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    min-width: 0;
  }

  .push-approval-text {
    display: grid;
    flex: 1;
    min-width: 0;
    font-size: calc(0.733333rem * var(--type-scale));
  }

  .push-approval-text strong {
    font-weight: 600;
    overflow-wrap: anywhere;
  }

  .push-approval-revoke {
    flex-shrink: 0;
    min-height: 1.9rem;
    border: 1px solid var(--border);
    border-radius: var(--floating-item-radius);
    padding: 0.2rem 0.7rem;
    font-size: calc(0.733333rem * var(--type-scale));
  }

  .push-approval-revoke:hover:not(:disabled) { background: var(--accent); }
  .push-approval-revoke:disabled { cursor: not-allowed; opacity: 0.5; }

  @media (pointer: coarse) {
    .push-approval-revoke { min-height: 2.75rem; }
  }

  .push-approval-text span {
    color: var(--muted-foreground);
    overflow-wrap: anywhere;
  }
</style>
