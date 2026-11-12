<script lang="ts">
  import ArrowUpRight from "@lucide/svelte/icons/arrow-up-right";
  import type { ChatDelegatedChildRead, ChatReplyThreadId } from "$lib/chat/contracts";
  import { chatParticipantDisplayName } from "$lib/chat/teammates/participant-display";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import ChatParticipantAvatar from "$lib/components/chat/identity/ChatParticipantAvatar.svelte";

  /** Lists the work teammates delegated from this thread's work, each with its live state. */
  let {
    delegatedChildren,
    onOpenThread,
  }: {
    delegatedChildren: readonly ChatDelegatedChildRead[];
    onOpenThread: (replyThreadId: ChatReplyThreadId) => void;
  } = $props();

  const { t } = getLocalization();
  const preferences = getPreferences();

  function teammateName(child: ChatDelegatedChildRead): string {
    return chatParticipantDisplayName(child.teammate, preferences.profileDisplayName, t("chat.timeline.you"));
  }
</script>

<section class="delegated-work" aria-label={t("chat.delegation.delegatedWork")} data-delegated-work>
  <h3>{t("chat.delegation.delegatedWork")}</h3>
  <ul>
    {#each delegatedChildren as child (child.assignmentId)}
      {@const name = teammateName(child)}
      <li data-delegated-child={child.assignmentId} data-state={child.state}>
        <ChatParticipantAvatar participant={child.teammate} size={22} shape="compact" />
        <div class="child-copy">
          <span class="child-heading"><strong>{name}</strong><span class="child-state">{t(`chat.delegation.states.${child.state}`)}</span></span>
          <p>{child.objective}</p>
          {#if child.stateReason}<small>{child.stateReason}</small>{/if}
        </div>
        <button type="button" class="open-child" aria-label={t("chat.delegation.openChildThread", name)} title={t("chat.delegation.openChildThread", name)} onclick={() => onOpenThread(child.replyThreadId)}><ArrowUpRight size={14} /></button>
      </li>
    {/each}
  </ul>
</section>

<style>
  .delegated-work { display:grid; gap:0.35rem; margin:0.5rem var(--chat-message-row-padding-inline,1rem); }
  h3 { color:var(--muted-foreground); font-size: calc(0.7rem * var(--type-scale)); font-weight:500; }
  ul { display:grid; gap:0.35rem; }
  li { display:grid; grid-template-columns:auto minmax(0,1fr) auto; align-items:start; gap:0.5rem; border:1px solid var(--border); border-radius:0.45rem; padding:0.45rem 0.5rem; }
  .child-copy { display:grid; min-width:0; gap:0.1rem; font-size: calc(0.75rem * var(--type-scale)); }
  .child-heading { display:flex; min-width:0; flex-wrap:wrap; align-items:baseline; gap:0.4rem; }
  .child-heading strong { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  .child-state { border-radius:999px; background:var(--accent); padding:0.05rem 0.4rem; color:var(--muted-foreground); font-size: calc(0.65rem * var(--type-scale)); }
  .child-copy p { display:-webkit-box; overflow:hidden; -webkit-box-orient:vertical; -webkit-line-clamp:3; line-clamp:3; overflow-wrap:anywhere; }
  .child-copy small { color:var(--muted-foreground); font-size: calc(0.666667rem * var(--type-scale)); overflow-wrap:anywhere; }
  .open-child { display:grid; min-width:1.75rem; min-height:1.75rem; place-items:center; border-radius:0.35rem; color:var(--muted-foreground); }
  .open-child:hover,.open-child:focus-visible { background:var(--accent); color:var(--foreground); }
  @media (pointer:coarse) { .open-child { min-width:2.75rem; min-height:2.75rem; } }
</style>
