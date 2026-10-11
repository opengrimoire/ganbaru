<script lang="ts">
  import type { NotesMentionTarget } from "$lib/notes/rich-text/core";
  import Bell from "@lucide/svelte/icons/bell";
  import CalendarDays from "@lucide/svelte/icons/calendar-days";
  import Database from "@lucide/svelte/icons/database";
  import FileText from "@lucide/svelte/icons/file-text";
  import Folder from "@lucide/svelte/icons/folder";
  import ListChecks from "@lucide/svelte/icons/list-checks";
  import Music from "@lucide/svelte/icons/music";
  import Timer from "@lucide/svelte/icons/timer";
  import UserRound from "@lucide/svelte/icons/user-round";

  /** The small glyph that identifies a mention suggestion's kind in mention menus. */
  let { target }: { target: NotesMentionTarget } = $props();
</script>

<span class="flex size-5 shrink-0 items-center justify-center rounded-floating-item bg-muted text-panel-detail text-muted-foreground">
  {#if target.kind === "date" && target.reminder}
    <Bell class="size-3.5" aria-hidden="true" />
  {:else if target.kind === "date"}
    <CalendarDays class="size-3.5" aria-hidden="true" />
  {:else if target.kind === "user"}
    <UserRound class="size-3.5" aria-hidden="true" />
  {:else if target.kind === "database"}
    <Database class="size-3.5" aria-hidden="true" />
  {:else if target.kind === "project"}
    <Folder class="size-3.5" aria-hidden="true" />
  {:else if target.kind === "project_task"}
    <ListChecks class="size-3.5" aria-hidden="true" />
  {:else if target.kind === "calendar_event"}
    <CalendarDays class="size-3.5" aria-hidden="true" />
  {:else if target.kind === "pomodoro_run"}
    <Timer class="size-3.5" aria-hidden="true" />
  {:else if target.kind === "music_item"}
    <Music class="size-3.5" aria-hidden="true" />
  {:else if target.iconText}
    <span aria-hidden="true">{target.iconText}</span>
  {:else}
    <FileText class="size-3.5" aria-hidden="true" />
  {/if}
</span>
