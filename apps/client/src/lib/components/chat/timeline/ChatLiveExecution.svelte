<script lang="ts">
  import { readChatLiveExecution } from "$lib/api/chat";
  import type { ChatParticipantRead, ChatWorkAssignmentId, ChatWorkAssignmentRead } from "$lib/chat/contracts";
  import { liveExecutionShowsWholeMessages, liveExecutionTimelinePage } from "$lib/chat/timeline/live-execution";
  import { LiveExecutionFollower, type LiveExecutionView } from "$lib/chat/timeline/live-execution-follower";
  import ChatExecutionTimeline from "./ChatExecutionTimeline.svelte";
  import ChatRemoteWorkRow from "./ChatRemoteWorkRow.svelte";

  /**
   * Shows the run of work a linked desktop executes, read from that desktop over the LAN while this thread is open and
   * the app is in the foreground: rows render read-only as they happen and stay once the run settles. While the work
   * is active, the work row below keeps Stop, and its state while the rows do not already show it; while no row arrives, such as off the LAN, the work row
   * stands alone and the replies arrive by sync.
   */
  let {
    assignmentId,
    work,
    agent,
    agentName,
    onActionError,
    onShown,
  }: {
    assignmentId: ChatWorkAssignmentId;
    /** The work while it is active, which shows the work row; `null` once it finished. */
    work: Pick<ChatWorkAssignmentRead, "id" | "state" | "createdAt"> | null;
    agent: ChatParticipantRead | null;
    agentName: string;
    onActionError: (error: unknown) => void;
    /** Receives the run whose whole turn shows, so the thread can set aside the replies projected from it. */
    onShown?: (runId: string | null) => void;
  } = $props();

  let view = $state<LiveExecutionView>({ state: null, status: "connecting", claimExpected: true });
  let foreground = $state(typeof document === "undefined" || document.visibilityState === "visible");
  const follower = new LiveExecutionFollower({
    read: readChatLiveExecution,
    onUpdate: (next) => { view = next; },
  });
  const timelinePage = $derived(liveExecutionTimelinePage(view.state));
  const active = $derived(work !== null);
  const shownRunId = $derived(view.state && timelinePage && liveExecutionShowsWholeMessages(view.state)
    ? view.state.runId
    : null);

  $effect(() => {
    const updateForeground = (): void => { foreground = document.visibilityState === "visible"; };
    document.addEventListener("visibilitychange", updateForeground);
    return () => document.removeEventListener("visibilitychange", updateForeground);
  });

  $effect(() => {
    if (!foreground) return;
    follower.follow(assignmentId, active);
    return () => follower.pause();
  });

  $effect(() => {
    onShown?.(shownRunId);
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
{#if work}
  <ChatRemoteWorkRow {work} {agent} {agentName} {onActionError} identity={timelinePage === null}
    live={{ rowsShown: timelinePage !== null, claimExpected: view.claimExpected }}
  />
{/if}
