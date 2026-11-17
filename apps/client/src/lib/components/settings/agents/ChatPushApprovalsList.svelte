<script lang="ts">
  import * as chatApi from "$lib/api/chat";
  import type { ChatParticipantId, ChatPushApproval } from "$lib/chat/contracts";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";

  /**
   * One agent's standing push approvals, which a person makes by choosing not to be asked again on a push
   * request. Revoking one makes the next matching push ask again on every linked device. `refreshKey` reloads
   * the list when the agent's access changes, since narrowing it revokes approvals.
   */
  let { agentId, refreshKey }: { agentId: ChatParticipantId; refreshKey?: unknown } = $props();

  const { t } = getLocalization();
  let approvals = $state<ChatPushApproval[] | null>(null);
  let error = $state<string | null>(null);
  let revokeError = $state<string | null>(null);
  let revoking = $state<string | null>(null);
  let request = 0;

  async function load(id: ChatParticipantId): Promise<void> {
    const current = ++request;
    error = null;
    try {
      const read = await chatApi.listChatPushApprovals(id);
      if (current === request) approvals = read;
    } catch (cause) {
      if (current === request) error = chatErrorMessage(cause, t("settings.agents.agents.pushApprovalsLoadFailed"));
    }
  }

  async function revoke(approval: ChatPushApproval): Promise<void> {
    if (revoking) return;
    revoking = approval.approvalId;
    revokeError = null;
    try {
      await chatApi.revokeChatPushApproval(approval.approvalId);
      await load(agentId);
    } catch (cause) {
      revokeError = chatErrorMessage(cause, t("settings.agents.agents.revokePushApprovalFailed"));
    } finally {
      revoking = null;
    }
  }

  $effect(() => {
    void refreshKey;
    void load(agentId);
  });
</script>

<div class="push-approvals">
  {#if error}
    <Notice message={error} onRetry={() => load(agentId)} copy={false} />
  {:else if approvals && approvals.length === 0}
    <p class="push-approvals-empty">{t("settings.agents.agents.pushApprovalsEmpty")}</p>
  {:else if approvals}
    <ul>
      {#each approvals as approval (approval.approvalId)}
        <li>
          <div class="push-approval-text">
            <strong>{approval.workingFolderName}</strong>
            <span>{t("settings.agents.agents.pushApprovalTarget", approval.pushUrl, approval.destinationBranch)}</span>
          </div>
          <button
            type="button"
            class="push-approval-revoke"
            disabled={revoking !== null}
            aria-busy={revoking === approval.approvalId}
            aria-label={t("settings.agents.agents.revokePushApproval", approval.workingFolderName, approval.destinationBranch)}
            onclick={() => void revoke(approval)}
          >{t("settings.agents.agents.revokePushApprovalAction")}</button>
        </li>
      {/each}
    </ul>
  {/if}
  {#if revokeError}<Notice message={revokeError} copy={false} />{/if}
</div>

<style>
  .push-approvals {
    font-size: calc(0.733333rem * var(--type-scale));
    display: grid;
    gap: 0.4rem;
  }

  .push-approvals ul {
    display: grid;
    gap: 0.4rem;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .push-approvals li {
    display: flex;
    align-items: center;
    gap: 0.75rem;
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

  .push-approval-text span,
  .push-approvals-empty {
    color: var(--muted-foreground);
    overflow-wrap: anywhere;
  }

  .push-approvals-empty {
    margin: 0;
    font-size: calc(0.733333rem * var(--type-scale));
  }

  .push-approval-revoke {
    flex-shrink: 0;
    min-height: var(--control-height);
    border: 1px solid var(--border);
    border-radius: 0.375rem;
    padding: 0 0.65rem;
    font-size: calc(0.733333rem * var(--type-scale));
  }

  .push-approval-revoke:disabled {
    cursor: not-allowed;
    opacity: 0.5;
  }
</style>
