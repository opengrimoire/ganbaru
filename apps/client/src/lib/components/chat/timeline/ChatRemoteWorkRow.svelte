<script lang="ts">
  import type { ChatParticipantRead, ChatWorkAssignmentRead } from "$lib/chat/contracts";
  import ChatIdentityButton from "$lib/components/chat/identity/ChatIdentityButton.svelte";
  import { formatDateTime } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import ChatStopWorkButton from "./ChatStopWorkButton.svelte";

  /**
   * Stands for active work whose run this device does not hold, such as work a linked desktop runs for a message posted
   * here: the agent, when the work started, what state it is in, and a Stop working control, laid out like the agent's
   * own reply so the reply takes its place when it arrives.
   */
  let {
    work,
    agent,
    agentName,
    onActionError,
    identity = true,
  }: {
    work: Pick<ChatWorkAssignmentRead, "id" | "state" | "createdAt">;
    /** The agent doing the work, or `null` when the thread does not name one. */
    agent: ChatParticipantRead | null;
    agentName: string;
    /** Reports a failed stop request. */
    onActionError: (error: unknown) => void;
    /** Whether the row shows the agent and start time; the live execution above it shows them instead. */
    identity?: boolean;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const startedAt = $derived(Date.parse(work.createdAt));
</script>

<div class="remote-work" data-work-state={work.state}>
  <div class="avatar-cell">
    {#if identity && agent}<ChatIdentityButton participant={agent} presentation="avatar" size={34} currentResponseSettings />{/if}
  </div>
  <div class="remote-work-body">
    {#if identity}<header>
      <strong>{#if agent}<ChatIdentityButton participant={agent} presentation="name" triggerLabel={agentName} currentResponseSettings />{:else}{agentName}{/if}</strong>
      {#if Number.isFinite(startedAt)}<time datetime={work.createdAt}>{formatDateTime(localization.locale, startedAt, { timeStyle: "short" })}</time>{/if}
    </header>{/if}
    <p class="remote-work-state" class:active={work.state === "working"} role="status"><span>{t(`chat.delegation.states.${work.state}`)}</span></p>
    <ChatStopWorkButton assignmentId={work.id} {agentName} {onActionError} />
  </div>
</div>

<style>
  /* Matches the agent message row: the row inset, the 34px avatar, and the avatar gap. */
  .remote-work { display:grid; grid-template-columns:34px minmax(0,1fr); gap:0.65rem; padding:var(--chat-conversation-entry-space,0.45rem) var(--chat-message-row-padding-inline,1rem); }
  .remote-work-body { display:grid; justify-items:start; gap:0.25rem; min-width:0; transform:translateY(-4px); }
  header { display:flex; min-width:0; min-height:1.3rem; align-items:baseline; gap:0.4rem; }
  header strong { min-width:0; overflow:hidden; font-size:var(--chat-organizational-font-size,calc(0.875rem * var(--type-scale))); text-overflow:ellipsis; white-space:nowrap; }
  time { flex:0 0 auto; color:var(--muted-foreground); font-size:var(--chat-organizational-time-font-size,calc(0.6875rem * var(--type-scale))); }
  .remote-work-state { margin:0; color:var(--muted-foreground); font-size:var(--chat-process-font-size,calc(0.8125rem * var(--type-scale))); line-height:var(--chat-process-line-height,calc(1.1875rem * var(--type-scale))); }
  .remote-work-state.active > span {
    animation: remote-work-shimmer 6s ease-in-out infinite;
    background: linear-gradient(100deg, var(--muted-foreground) 0%, var(--muted-foreground) 42%, var(--foreground) 50%, var(--muted-foreground) 58%, var(--muted-foreground) 100%);
    background-repeat: no-repeat;
    background-size: 320% 100%;
    background-clip: text;
    color: transparent;
    -webkit-background-clip: text;
  }
  @keyframes remote-work-shimmer { 0%, 8% { background-position: 100% 0; } 65%, 100% { background-position: 0% 0; } }
  @media (prefers-reduced-motion: reduce) { .remote-work-state.active > span { animation:none; background:none; color:inherit; -webkit-text-fill-color:currentColor; } }
</style>
