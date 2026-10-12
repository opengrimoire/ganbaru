<script lang="ts">
  import type { ChatMessageActionTarget } from "$lib/chat/timeline/message-action-target";
  import type { ChatMessageReaction } from "$lib/chat/timeline/organizational-message-model";
  import { LOCAL_CHAT_PARTICIPANT_ID } from "$lib/chat/teammates/participant-display";
  import ProjectIcon from "$lib/components/projects/ProjectIcon.svelte";
  import { formatList } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getChat } from "$lib/stores/chat.svelte";
  import { getPreferences } from "$lib/stores/preferences.svelte";

  let { target }: { target: ChatMessageActionTarget } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const chat = getChat();
  const preferences = getPreferences();
  const reactions = $derived(chat.messageReactionsFor(target.reactionKey));
  const localDisplayName = $derived(preferences.profileDisplayName.trim() || t("chat.timeline.you"));

  function toggleReaction(value: string): void {
    chat.toggleSessionMessageReaction(target.reactionKey, value, localDisplayName);
  }

  function isSelectedByLocalUser(reaction: ChatMessageReaction): boolean {
    return reaction.participants.some(({ participantId }) => participantId === LOCAL_CHAT_PARTICIPANT_ID);
  }

  function reactionTooltip(reaction: ChatMessageReaction): string {
    const names = reaction.participants.map((participant) => participant.participantId === LOCAL_CHAT_PARTICIPANT_ID
      ? localDisplayName
      : participant.displayName);
    return t("chat.organization.reactedBy", formatList(localization.locale, names), names.length);
  }
</script>

{#if reactions.length > 0}
  <div class="message-reaction-list reaction-list">
    {#each reactions as reaction (reaction.value)}
      <button
        type="button"
        class="message-reaction-chip reaction-chip"
        class:selected={isSelectedByLocalUser(reaction)}
        aria-pressed={isSelectedByLocalUser(reaction)}
        aria-label={reactionTooltip(reaction)}
        data-app-tooltip={reactionTooltip(reaction)}
        onclick={() => toggleReaction(reaction.value)}
      >
        <ProjectIcon name={reaction.value} size={12} />
        <span>{reaction.participants.length}</span>
      </button>
    {/each}
  </div>
{/if}

<style>
  .message-reaction-list { display:flex; max-width:100%; flex-wrap:wrap; gap:0.25rem; margin-top:var(--chat-message-reaction-margin-top,0.4rem); }
  .message-reaction-chip { display:inline-flex; min-width:2.35rem; height:1.5rem; align-items:center; justify-content:center; gap:0.3rem; border:1px solid transparent; border-radius:0.4rem; background:color-mix(in srgb,var(--accent) 76%,transparent); padding-inline:0.35rem; color:var(--muted-foreground); font-size: calc(0.7rem * var(--type-scale)); font-variant-numeric:tabular-nums; }
  .message-reaction-chip:hover,.message-reaction-chip:focus-visible { border-color:var(--border); background:var(--accent); color:var(--foreground); }
  .message-reaction-chip.selected { border-color:color-mix(in srgb,var(--primary) 60%,var(--border)); background:color-mix(in srgb,var(--primary) 13%,transparent); color:var(--primary); }
</style>
