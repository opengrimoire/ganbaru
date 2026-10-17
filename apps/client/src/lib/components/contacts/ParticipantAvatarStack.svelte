<script lang="ts">
  import type { ChatAiTeammateRead, ChatParticipantRead } from "$lib/chat/contracts";
  import { LOCAL_CHAT_PARTICIPANT_ID } from "$lib/chat/teammates/participant-display";
  import ChatParticipantAvatar from "$lib/components/chat/identity/ChatParticipantAvatar.svelte";
  import ProfileAvatar from "$lib/components/profile/ProfileAvatar.svelte";
  import { formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getPreferences } from "$lib/stores/preferences.svelte";

  /** A member shown in the stack; teammates pass their read so the model avatar resolves. */
  export interface ParticipantAvatarStackItem {
    participant: ChatParticipantRead;
    teammate?: ChatAiTeammateRead | null;
  }

  let {
    items,
    size = 22,
    max = 3,
  }: {
    items: readonly ParticipantAvatarStackItem[];
    size?: number;
    /** Avatars drawn after the local user before the remaining count collapses into one chip. */
    max?: number;
  } = $props();

  const preferences = getPreferences();
  const localization = getLocalization();
  const { t } = localization;
  const others = $derived(items.filter((item) => item.participant.id !== LOCAL_CHAT_PARTICIPANT_ID));
  const visible = $derived(others.slice(0, max));
  const overflow = $derived(others.length - visible.length);
  const overlap = $derived(Math.round(size * 0.3));
</script>

<span class="inline-flex items-center" style:--stack-overlap={`${overlap}px`}>
  <span class="avatar-slot">
    <ProfileAvatar
      displayName={preferences.profileDisplayName || t("contacts.you")}
      imagePath={preferences.profileAvatarImagePath}
      crop={preferences.profileImageCrop}
      color={preferences.profileColor}
      {size}
    />
  </span>
  {#each visible as item (item.participant.id)}
    <span class="avatar-slot">
      <ChatParticipantAvatar participant={item.participant} teammate={item.teammate ?? null} {size} />
    </span>
  {/each}
  {#if overflow > 0}
    <span
      class="avatar-slot flex items-center justify-center rounded-[22%] border border-border bg-muted text-[0.6rem] font-medium text-muted-foreground tabular-nums"
      style:width={`${size}px`}
      style:height={`${size}px`}
    >+{formatNumber(localization.locale, overflow)}</span>
  {/if}
</span>

<style>
  .avatar-slot {
    display: inline-grid;
    flex: 0 0 auto;
    border-radius: 22%;
    outline: 2px solid var(--background);
  }

  .avatar-slot + .avatar-slot {
    margin-left: calc(-1 * var(--stack-overlap));
  }
</style>
