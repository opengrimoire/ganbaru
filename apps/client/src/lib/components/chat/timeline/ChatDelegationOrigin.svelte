<script lang="ts">
  import CornerLeftUp from "@lucide/svelte/icons/corner-left-up";
  import type { ChatAssignmentDelegationRead, ChatReplyThreadId } from "$lib/chat/contracts";
  import { getLocalization } from "$lib/i18n/translator.svelte";

  /** Shows where delegated work came from and what it was asked to return. */
  let {
    delegation,
    onOpenThread,
  }: {
    delegation: ChatAssignmentDelegationRead;
    onOpenThread: (replyThreadId: ChatReplyThreadId) => void;
  } = $props();

  const { t } = getLocalization();
</script>

<section class="delegation-origin" aria-label={t("chat.delegation.delegatedFrom")} data-delegation-origin>
  <header>
    <strong>{t("chat.delegation.delegatedFrom")}</strong>
    {#if delegation.parentReplyThreadId}
      {@const parentReplyThreadId = delegation.parentReplyThreadId}
      <button type="button" class="open-parent" onclick={() => onOpenThread(parentReplyThreadId)}>
        <CornerLeftUp size={12} />{t("chat.delegation.openParentThread")}
      </button>
    {/if}
  </header>
  <p class="objective">{delegation.objective}</p>
  <p class="expected"><span>{t("chat.delegation.expectedResult")}</span>{delegation.expectedResult}</p>
</section>

<style>
  .delegation-origin { display:grid; gap:0.3rem; margin:0.35rem var(--chat-message-row-padding-inline,1rem); border-left:2px solid var(--primary); padding:0.2rem 0 0.2rem 0.6rem; font-size: calc(0.75rem * var(--type-scale)); }
  header { display:flex; flex-wrap:wrap; align-items:center; justify-content:space-between; gap:0.4rem; }
  header strong { color:var(--muted-foreground); font-size: calc(0.7rem * var(--type-scale)); font-weight:500; }
  .open-parent { display:inline-flex; min-height:1.75rem; align-items:center; gap:0.3rem; border-radius:0.35rem; padding-inline:0.4rem; color:color-mix(in srgb,var(--primary) 70%,var(--foreground)); font-size: calc(0.7rem * var(--type-scale)); }
  .open-parent:hover,.open-parent:focus-visible { background:var(--accent); }
  .objective, .expected { overflow-wrap:anywhere; white-space:pre-wrap; }
  .expected span { display:block; color:var(--muted-foreground); font-size: calc(0.666667rem * var(--type-scale)); }
  @media (pointer:coarse) { .open-parent { min-height:2.75rem; } }
</style>
