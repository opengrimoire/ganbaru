<script lang="ts">
  import ChatTimeline from "./ChatTimeline.svelte";
  import type { ChatParticipantRead, ChatThreadShellRead, ChatTimelinePageRead, ChatTurnId } from "$lib/chat/contracts";

  let {
    bottomInsetPx = 0,
    embedded = false,
    hideUserMessages = false,
    teammate = null,
    turnId = null,
    timelinePage = null,
    executionThread = null,
  }: {
    bottomInsetPx?: number;
    embedded?: boolean;
    hideUserMessages?: boolean;
    teammate?: ChatParticipantRead | null;
    turnId?: ChatTurnId | null;
    timelinePage?: ChatTimelinePageRead | null;
    executionThread?: ChatThreadShellRead | null;
  } = $props();
</script>

<ChatTimeline {bottomInsetPx} {embedded} {hideUserMessages} {teammate} {turnId} {timelinePage} {executionThread} />
