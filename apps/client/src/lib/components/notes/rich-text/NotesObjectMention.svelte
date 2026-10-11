<script lang="ts">
  import type { NotesExternalMentionRichText } from "$lib/notes/rich-text/core";
  import { getNavigation } from "$lib/stores/navigation.svelte";
  import { getProjects } from "$lib/stores/projects.svelte";
  import ProjectIcon from "$lib/components/projects/ProjectIcon.svelte";
  import Bell from "@lucide/svelte/icons/bell";
  import CalendarDays from "@lucide/svelte/icons/calendar-days";
  import Folder from "@lucide/svelte/icons/folder";
  import ListChecks from "@lucide/svelte/icons/list-checks";
  import Music from "@lucide/svelte/icons/music";
  import Timer from "@lucide/svelte/icons/timer";
  import UserRound from "@lucide/svelte/icons/user-round";

  interface MentionAttributes {
    class?: string;
    style?: string;
    [name: `data-${string}`]: string | undefined;
  }

  const POINTER_DRAG_DISTANCE_PX = 5;
  const PROJECT_ICON_SIZE_PX = 14;

  let { mention, text, showIcon = true, attributes = {} }: {
    mention: NotesExternalMentionRichText;
    text: string;
    showIcon?: boolean;
    attributes?: MentionAttributes;
  } = $props();

  const navigation = getNavigation();
  const projects = getProjects();
  const objectMention = $derived(mention.mention.type === "ganbaru_object" ? mention.mention.ganbaru_object : null);
  const project = $derived(objectMention?.type === "project" ? projects.projectById(objectMention.id) : undefined);
  const navigable = $derived(objectMention?.type === "project"
    || objectMention?.type === "project_task"
    || objectMention?.type === "calendar_event");
  const mentionType = $derived(objectMention?.type ?? mention.mention.type);
  const reminder = $derived(mention.mention.type === "date" && mention.mention.date.ganbaru_reminder?.enabled === true);
  let element = $state<HTMLSpanElement | null>(null);
  let pointerStart: { x: number; y: number } | null = null;
  let opening = false;

  /** Resolve a task's owner from loaded summaries before asking the backend. */
  async function taskProjectId(taskId: string): Promise<string> {
    const loaded = projects.taskById(taskId)?.projectId;
    if (loaded) return loaded;
    const api = await import("$lib/api/projects");
    return (await api.loadProjectTaskDetail(taskId)).task.projectId;
  }

  async function open(): Promise<void> {
    if (!objectMention || !navigable || opening) return;
    opening = true;
    try {
      if (objectMention.type === "calendar_event") {
        navigation.navigate("calendar");
        return;
      }
      const projectId = objectMention.type === "project" ? objectMention.id : await taskProjectId(objectMention.id);
      await projects.selectProject(projectId);
      navigation.navigate("projects");
    } catch (error: unknown) {
      console.warn("Notes mention navigation failed", { type: objectMention.type, id: objectMention.id, error });
    } finally {
      opening = false;
    }
  }

  function handleClick(event: MouseEvent): void {
    if (!navigable) return;
    event.preventDefault();
    event.stopPropagation();
    const moved = pointerStart !== null
      && Math.hypot(event.clientX - pointerStart.x, event.clientY - pointerStart.y) >= POINTER_DRAG_DISTANCE_PX;
    pointerStart = null;
    const selection = element?.ownerDocument.getSelection();
    if (moved || (selection && !selection.isCollapsed && !event.ctrlKey && !event.metaKey)) return;
    void open();
  }

  /** Track mouse and pen drags while allowing touch taps recognized by the browser. */
  function handlePointerDown(event: PointerEvent): void {
    pointerStart = event.pointerType === "touch" ? null : { x: event.clientX, y: event.clientY };
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.isComposing || event.key !== "Enter" || !navigable) return;
    event.preventDefault();
    event.stopPropagation();
    void open();
  }

  /** Handle native clicks before an editing host can treat them as caret placement only. */
  function mentionEvents(node: HTMLSpanElement) {
    node.addEventListener("click", handleClick);
    node.addEventListener("keydown", handleKeydown);
    node.addEventListener("pointerdown", handlePointerDown);
    return { destroy() {
      node.removeEventListener("click", handleClick);
      node.removeEventListener("keydown", handleKeydown);
      node.removeEventListener("pointerdown", handlePointerDown);
    } };
  }

  const focusable = $derived(navigable && element !== null && !element.closest("[contenteditable='true']"));
</script>

{#snippet icon()}<span class="notes-mention-chip-icon" data-notes-editor-sentinel="decoration" aria-hidden="true">{#if mention.mention.type === "user"}<UserRound size="1em" strokeWidth={1.75} />{:else if mention.mention.type === "date"}{#if reminder}<Bell size="1em" strokeWidth={1.75} />{:else}<CalendarDays size="1em" strokeWidth={1.75} />{/if}{:else if objectMention?.type === "project"}{#if project?.icon}<ProjectIcon name={project.icon} size={PROJECT_ICON_SIZE_PX} />{:else}<Folder size="1em" strokeWidth={1.75} />{/if}{:else if objectMention?.type === "project_task"}<ListChecks size="1em" strokeWidth={1.75} />{:else if objectMention?.type === "calendar_event"}<CalendarDays size="1em" strokeWidth={1.75} />{:else if objectMention?.type === "pomodoro_run"}<Timer size="1em" strokeWidth={1.75} />{:else}<Music size="1em" strokeWidth={1.75} />{/if}</span>{/snippet}

{#if navigable}<span
  {...attributes}
  bind:this={element}
  use:mentionEvents
  class={`notes-mention-chip notes-mention-chip-link ${attributes.class ?? ""}`}
  role="link"
  tabindex={focusable ? 0 : undefined}
  data-notes-mention-chip="true"
  data-notes-mention-type={mentionType}
>{#if showIcon}{@render icon()}{/if}{text}</span>{:else}<span
  {...attributes}
  class={`notes-mention-chip ${attributes.class ?? ""}`}
  data-notes-mention-chip="true"
  data-notes-mention-type={mentionType}
>{#if showIcon}{@render icon()}{/if}{text}</span>{/if}
