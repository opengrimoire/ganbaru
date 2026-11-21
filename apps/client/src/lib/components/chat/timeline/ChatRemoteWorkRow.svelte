<script lang="ts">
  import Clock from "@lucide/svelte/icons/clock";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import MessageCircleQuestionMark from "@lucide/svelte/icons/message-circle-question-mark";
  import ShieldQuestionMark from "@lucide/svelte/icons/shield-question-mark";
  import type { ChatParticipantRead, ChatWorkAssignmentRead } from "$lib/chat/contracts";
  import { remoteWorkStatus, remoteWorkStatusIcon, type RemoteWorkLiveView } from "$lib/chat/timeline/remote-work-status";
  import ChatIdentityButton from "$lib/components/chat/identity/ChatIdentityButton.svelte";
  import { formatDateTime } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import ChatStopWorkButton from "./ChatStopWorkButton.svelte";

  /**
   * Stands for active work whose run this device does not hold, such as work a linked desktop runs for a message posted
   * here: the agent, when the work started, what state it is in as a step like the run's own, and a Stop working
   * control, laid out like the agent's own reply so the reply takes its place when it arrives.
   */
  let {
    work,
    agent,
    agentName,
    onActionError,
    identity = true,
    live = { rowsShown: false, claimExpected: false },
  }: {
    work: Pick<ChatWorkAssignmentRead, "id" | "state" | "createdAt">;
    /** The agent doing the work, or `null` when the thread does not name one. */
    agent: ChatParticipantRead | null;
    agentName: string;
    /** Reports a failed stop request. */
    onActionError: (error: unknown) => void;
    /** Whether the row shows the agent and start time; the live execution above it shows them instead. */
    identity?: boolean;
    /** What the live execution above the row shows of the run, which may stand for the state instead. */
    live?: RemoteWorkLiveView;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const startedAt = $derived(Date.parse(work.createdAt));
  const status = $derived(remoteWorkStatus(work.state, live));
  const icon = $derived(status ? remoteWorkStatusIcon(status) : "none");
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
    {#if status}
      <p class="remote-work-state" class:active={icon === "progress"} role="status">
        <span class="remote-work-icon" aria-hidden="true">{#if icon === "progress"}<LoaderCircle size={15} class="animate-spin motion-reduce:animate-none" />{:else if icon === "queued"}<Clock size={15} />{:else if icon === "question"}<MessageCircleQuestionMark size={15} />{:else if icon === "approval"}<ShieldQuestionMark size={15} />{/if}</span><span class="remote-work-label">{status === "starting" ? t("chat.delegation.starting") : t(`chat.delegation.states.${status}`)}</span>
      </p>
    {/if}
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
  /* Matches a run's process step: the icon column, its gap, and the step type. */
  .remote-work-state { display:grid; grid-template-columns:1rem minmax(0,1fr); align-items:start; gap:0.45rem; margin:0; color:var(--muted-foreground); font-size:var(--chat-process-font-size,calc(0.8125rem * var(--type-scale))); line-height:var(--chat-process-line-height,calc(1.1875rem * var(--type-scale))); }
  .remote-work-icon { display:flex; height:var(--chat-process-line-height,calc(1.1875rem * var(--type-scale))); align-items:center; }
  .remote-work-state.active { color:color-mix(in srgb, var(--muted-foreground) 78%, var(--foreground)); }
  .remote-work-state.active > .remote-work-label {
    animation: remote-work-shimmer 6s ease-in-out infinite;
    background: linear-gradient(100deg, var(--muted-foreground) 0%, var(--muted-foreground) 42%, var(--foreground) 50%, var(--muted-foreground) 58%, var(--muted-foreground) 100%);
    background-repeat: no-repeat;
    background-size: 320% 100%;
    background-clip: text;
    color: transparent;
    -webkit-background-clip: text;
  }
  @keyframes remote-work-shimmer { 0%, 8% { background-position: 100% 0; } 65%, 100% { background-position: 0% 0; } }
  @media (prefers-reduced-motion: reduce) { .remote-work-state.active > .remote-work-label { animation:none; background:none; color:inherit; -webkit-text-fill-color:currentColor; } }
</style>
