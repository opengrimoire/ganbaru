<script lang="ts">
  import type { ChatAiTeammateRead, ChatParticipantRead } from "$lib/chat/contracts";
  import { chatParticipantDisplayName } from "$lib/chat/teammates/participant-display";
  import { chatTeammateModelParticipant } from "$lib/chat/teammates/identity";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getChat } from "$lib/stores/chat.svelte";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import ProfileAvatar from "$lib/components/profile/ProfileAvatar.svelte";
  import ChatModelAvatar from "./ChatModelAvatar.svelte";

  let {
    participant,
    teammate = null,
    size = 32,
    shape = "default",
  }: {
    participant: ChatParticipantRead;
    teammate?: ChatAiTeammateRead | null;
    size?: number;
    shape?: "default" | "compact";
  } = $props();
  const chat = getChat();
  const preferences = getPreferences();
  const { t } = getLocalization();
  const displayName = $derived(chatParticipantDisplayName(
    participant,
    preferences.profileDisplayName,
    t("chat.timeline.you"),
  ));
  const teammateIdentity = $derived(participant.kind === "ai_teammate"
    ? chatTeammateModelParticipant(
      participant.id,
      teammate ? [teammate, ...chat.teammateIdentities] : chat.teammateIdentities,
      chat.settings,
    )
    : null);
</script>

<span
  class="participant-avatar"
  class:agent={participant.kind === "ai_teammate"}
  class:local={participant.kind === "local_user"}
  style={`width:${size}px;height:${size}px;--chat-participant-avatar-radius:${shape === "compact" ? "0.2rem" : "22%"}`}
  aria-hidden="true"
>
  {#if participant.kind === "local_user"}
    <ProfileAvatar {displayName} imagePath={preferences.profileAvatarImagePath} crop={preferences.profileImageCrop} color={preferences.profileColor} {size} />
  {:else if participant.kind === "ai_teammate"}
    {#if teammateIdentity}
      <ChatModelAvatar
        familyId={teammateIdentity.company.iconFamilyId}
        label={teammateIdentity.company.name}
        {size}
      />
    {:else}
      <span class="agent-fallback">{displayName.slice(0, 1).toLocaleUpperCase()}</span>
    {/if}
  {:else}
    {displayName.slice(0, 1).toLocaleUpperCase()}
  {/if}
</span>

<style>
  .participant-avatar { display:inline-grid; flex:0 0 auto; place-items:center; border:1px solid color-mix(in srgb,var(--border) 75%,transparent); border-radius:var(--chat-participant-avatar-radius); background:var(--accent); color:var(--foreground); font-size: calc(0.72rem * var(--type-scale)); font-weight:700; }
  .participant-avatar.local { overflow:hidden; border:0; background:transparent; }
  .participant-avatar.local :global(.profile-avatar) { display:grid; }
  .participant-avatar.agent { overflow:hidden; border:0; background:transparent; }
  .agent-fallback { display:grid; width:100%; height:100%; place-items:center; border:1px solid color-mix(in srgb,var(--border) 75%,transparent); border-radius:var(--chat-participant-avatar-radius); background:var(--accent); color:var(--muted-foreground); }
</style>
