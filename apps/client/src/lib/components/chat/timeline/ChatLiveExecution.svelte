<script lang="ts">
  import { readChatLiveExecution } from "$lib/api/chat";
  import type { ChatParticipantRead, ChatWorkAssignmentRead } from "$lib/chat/contracts";
  import { liveExecutionTimelinePage } from "$lib/chat/timeline/live-execution";
  import { LiveExecutionFollower, type LiveExecutionView } from "$lib/chat/timeline/live-execution-follower";
  import ChatExecutionTimeline from "./ChatExecutionTimeline.svelte";
  import ChatRemoteWorkRow from "./ChatRemoteWorkRow.svelte";

  /**
   * Shows work a linked desktop runs as it happens, read from that desktop over the LAN while this thread is open and
   * the app is in the foreground. The run's rows render read-only above the work row, which keeps the state and Stop
   * working; while no row arrives, such as off the LAN, the work row stands alone and the reply arrives by sync.
   */
  let {
    work,
    agent,
    agentName,
    onActionError,
  }: {
    work: Pick<ChatWorkAssignmentRead, "id" | "state" | "createdAt">;
    agent: ChatParticipantRead | null;
    agentName: string;
    onActionError: (error: unknown) => void;
  } = $props();

  let view = $state<LiveExecutionView>({ state: null, status: "connecting" });
  let foreground = $state(typeof document === "undefined" || document.visibilityState === "visible");
  const follower = new LiveExecutionFollower({
    read: readChatLiveExecution,
    onUpdate: (next) => { view = next; },
  });
  const timelinePage = $derived(liveExecutionTimelinePage(view.state));

  $effect(() => {
    const updateForeground = (): void => { foreground = document.visibilityState === "visible"; };
    document.addEventListener("visibilitychange", updateForeground);
    return () => document.removeEventListener("visibilitychange", updateForeground);
  });

  $effect(() => {
    if (!foreground) return;
    follower.follow(work.id);
    return () => follower.pause();
  });
</script>

{#if timelinePage}
  <ChatExecutionTimeline
    embedded
    readOnly
    hideUserMessages
    {agent}
    turnId={view.state?.turnId ?? null}
    {timelinePage}
    providerFamilyId={view.state?.providerFamilyId ?? null}
  />
{/if}
<ChatRemoteWorkRow {work} {agent} {agentName} {onActionError} identity={timelinePage === null} />
