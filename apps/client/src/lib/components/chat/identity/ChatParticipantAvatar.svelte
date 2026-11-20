<script lang="ts">
  import type { ChatAiAgentRead, ChatParticipantRead } from "$lib/chat/contracts";
  import { chatParticipantDisplayName } from "$lib/chat/agents/participant-display";
  import { chatAgentCompany } from "$lib/chat/agents/identity";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { useChat } from "$lib/stores/chat.svelte";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import ProfileAvatar from "$lib/components/profile/ProfileAvatar.svelte";
  import ChatModelAvatar from "./ChatModelAvatar.svelte";

  let {
    participant,
    agent = null,
    size = 32,
    shape = "default",
  }: {
    participant: ChatParticipantRead;
    agent?: ChatAiAgentRead | null;
    size?: number;
    shape?: "default" | "compact";
  } = $props();
  const chat = useChat();
  const preferences = getPreferences();
  const { t } = getLocalization();
  const displayName = $derived(chatParticipantDisplayName(
    participant,
    preferences.profileDisplayName,
    t("chat.timeline.you"),
  ));
  const agentCompany = $derived(participant.kind === "ai_agent"
    ? chatAgentCompany(
      participant.id,
      agent ? [agent, ...chat.agentIdentities] : chat.agentIdentities,
      chat.settings,
    )
    : null);
</script>

<span
  class="participant-avatar"
  class:agent={participant.kind === "ai_agent"}
  class:local={participant.kind === "local_user"}
  style={`width:${size}px;height:${size}px;--chat-participant-avatar-radius:${shape === "compact" ? "0.2rem" : "22%"}`}
  aria-hidden="true"
>
  {#if participant.kind === "local_user"}
    <ProfileAvatar {displayName} imagePath={preferences.profileAvatarImagePath} crop={preferences.profileImageCrop} color={preferences.profileColor} {size} />
  {:else if participant.kind === "ai_agent"}
    {#if agentCompany}
      <ChatModelAvatar
        familyId={agentCompany.iconFamilyId}
        label={agentCompany.name}
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
