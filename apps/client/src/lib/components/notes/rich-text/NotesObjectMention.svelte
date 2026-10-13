<script lang="ts">
  import { onDestroy } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { NotesExternalMentionRichText } from "$lib/notes/rich-text/core";
  import type { NotesDateMentionValue } from "$lib/notes/types";
  import { notesCalendarEventPreviewWhen, notesDateMentionPreviewWhen } from "$lib/notes/rich-text/mention-preview";
  import { formatNotificationOffset } from "$lib/calendar/event-notifications";
  import { notesDateMentionReminderMinutes } from "$lib/notes/rich-text/date-mention-edit";
  import { getNavigation } from "$lib/stores/navigation.svelte";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { getProjects } from "$lib/stores/projects.svelte";
  import ProjectIcon from "$lib/components/projects/ProjectIcon.svelte";
  import Bell from "@lucide/svelte/icons/bell";
  import CalendarDays from "@lucide/svelte/icons/calendar-days";
  import Folder from "@lucide/svelte/icons/folder";
  import ListChecks from "@lucide/svelte/icons/list-checks";
  import UserRound from "@lucide/svelte/icons/user-round";
  import { NotesMentionPreviewHover } from "./mention-preview-hover.svelte";
  import NotesMentionPreviewCard from "./NotesMentionPreviewCard.svelte";

  interface MentionAttributes {
    class?: string;
    style?: string;
    [name: `data-${string}`]: string | undefined;
  }

  /** The parts of a mentioned task the preview and navigation need. */
  interface MentionedTask {
    projectId: string;
    title: string;
  }

  /** A mentioned Calendar event or series as read from Calendar storage. */
  interface MentionedEvent {
    calendarName: string | null;
    title: string;
    when: string | null;
    location: string | null;
  }

  const POINTER_DRAG_DISTANCE_PX = 5;
  const PROJECT_ICON_SIZE_PX = 14;
  const PREVIEW_ICON_SIZE_PX = 20;

  let { mention, text, showIcon = true, attributes = {}, onEdit }: {
    mention: NotesExternalMentionRichText;
    text: string;
    showIcon?: boolean;
    attributes?: MentionAttributes;
    /** Opens an editor for the mention from its chip; only date mentions take one. */
    onEdit?: (anchor: HTMLElement) => void;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const componentId = $props.id();
  const previewId = `${componentId}-mention-preview`;
  const navigation = getNavigation();
  const preferences = getPreferences();
  const projects = getProjects();
  const objectMention = $derived(mention.mention.type === "ganbaru_object" ? mention.mention.ganbaru_object : null);
  const objectKey = $derived(objectMention ? `${objectMention.type}:${objectMention.id}` : mention.mention.type);
  const project = $derived(objectMention?.type === "project" ? projects.projectById(objectMention.id) : undefined);
  const navigable = $derived(objectMention?.type === "project"
    || objectMention?.type === "project_task"
    || objectMention?.type === "calendar_event");
  const mentionType = $derived(objectMention?.type ?? mention.mention.type);
  const editable = $derived(mention.mention.type === "date" && onEdit !== undefined);
  const reminder = $derived(mention.mention.type === "date" && mention.mention.date.ganbaru_reminder?.enabled === true);
  let element = $state<HTMLSpanElement | null>(null);
  let task = $state<MentionedTask | null>(null);
  let calendarEvent = $state<MentionedEvent | null>(null);
  let previewStatus = $state<"idle" | "loading" | "ready" | "failed">("idle");
  let taskRequest: Promise<MentionedTask> | null = null;
  let pointerStart: { x: number; y: number } | null = null;
  let opening = false;
  let revision = 0;
  let disposed = false;
  const preview = new NotesMentionPreviewHover({
    anchor: () => element,
    canOpen: () => true,
    onOpen: () => void loadPreview(),
  });

  const loadedTask = $derived(objectMention?.type === "project_task" ? projects.taskById(objectMention.id) : undefined);
  const taskProject = $derived(projects.projectById(loadedTask?.projectId ?? task?.projectId));
  const previewHeading = $derived.by((): { route: string[]; title: string; details: string[] } => {
    const groupName = (owner: typeof project): string | undefined => projects.groupById(owner?.groupId)?.name;
    switch (objectMention?.type) {
      case "project":
        return { route: presentLines([groupName(project)]), title: project?.name.trim() || text, details: [] };
      case "project_task":
        return {
          route: presentLines([groupName(taskProject), taskProject?.name]),
          title: loadedTask?.title.trim() || task?.title.trim() || text,
          details: [],
        };
      case "calendar_event":
        return {
          route: calendarEvent?.calendarName ? [calendarEvent.calendarName] : [],
          title: calendarEvent?.title || text,
          details: presentLines([calendarEvent?.when, calendarEvent?.location]),
        };
    }
    if (mention.mention.type !== "date") return { route: [], title: text, details: [] };
    const spelled = notesDateMentionPreviewWhen(mention.mention.date, localization.locale, preferences.calendarTimeFormat);
    return {
      route: [],
      title: text,
      details: presentLines([spelled === text ? null : spelled, reminder ? reminderDetail(mention.mention.date) : null]),
    };
  });
  const previewStatusText = $derived(previewStatus === "failed" ? t("notes.databaseGalleryPreviewUnavailable") : null);

  /** The preview line for a date mention's reminder, naming how early it fires when it fires ahead of the date. */
  function reminderDetail(date: NotesDateMentionValue): string {
    const minutes = notesDateMentionReminderMinutes(date);
    return minutes > 0
      ? t("notes.reminderMentionBefore", formatNotificationOffset(minutes, t))
      : t("notes.reminderMention");
  }

  /** Drop missing and blank preview lines. */
  function presentLines(values: readonly (string | null | undefined)[]): string[] {
    return values.map((value) => value?.trim() ?? "").filter((value) => value.length > 0);
  }

  $effect(() => {
    void objectKey;
    revision += 1;
    preview.close();
    task = null;
    calendarEvent = null;
    taskRequest = null;
    previewStatus = "idle";
    pointerStart = null;
    return () => { revision += 1; preview.close(); };
  });

  onDestroy(() => { disposed = true; revision += 1; preview.dispose(); });

  /** Resolve a task's owner and title from loaded summaries before asking the backend, sharing one request. */
  async function resolveTask(taskId: string): Promise<MentionedTask> {
    const loaded = projects.taskById(taskId);
    if (loaded) return { projectId: loaded.projectId, title: loaded.title };
    if (task) return task;
    if (taskRequest) return taskRequest;
    const requestRevision = revision;
    const request = import("$lib/api/projects")
      .then((api) => api.loadProjectTaskDetail(taskId))
      .then(({ task: detail }) => ({ projectId: detail.projectId, title: detail.title }));
    taskRequest = request;
    try {
      const result = await request;
      if (!disposed && requestRevision === revision) task = result;
      return result;
    } finally {
      if (taskRequest === request) taskRequest = null;
    }
  }

  /** Read a mentioned event or series by id, independent of the range the Calendar has loaded. */
  async function readCalendarEvent(eventId: string): Promise<MentionedEvent | null> {
    const [{ loadPanelEvent }, { getCalendars }] = await Promise.all([
      import("$lib/stores/calendar/event-loaders"),
      import("$lib/stores/calendars.svelte"),
    ]);
    const event = await loadPanelEvent(eventId);
    if (!event) return null;
    return {
      calendarName: getCalendars().list.find((calendar) => calendar.id === event.calendarId)?.name.trim() || null,
      title: event.title.trim(),
      when: notesCalendarEventPreviewWhen({ ...event, recurring: Boolean(event.recurrence || event.rdate?.length) }, {
        locale: localization.locale,
        timeFormat: preferences.calendarTimeFormat,
        labels: {
          allDay: t("calendar.notification.allDay"),
          allDayOn: (date) => t("calendar.notification.allDayOn", date),
          datedTimeRange: (date, range) => t("calendar.notification.datedTimeRange", date, range),
        },
      }),
      location: event.location?.trim() || null,
    };
  }

  /** Load what the preview shows beyond the chip text, only for kinds that need a lookup. */
  async function loadPreview(): Promise<void> {
    if (!objectMention || previewStatus === "loading" || previewStatus === "ready") return;
    if (objectMention.type !== "project_task" && objectMention.type !== "calendar_event") return;
    const requestRevision = revision;
    const target = objectMention;
    previewStatus = "loading";
    try {
      if (target.type === "project_task") await resolveTask(target.id);
      else {
        const event = await readCalendarEvent(target.id);
        if (disposed || requestRevision !== revision) return;
        calendarEvent = event;
      }
      if (disposed || requestRevision !== revision) return;
      previewStatus = "ready";
    } catch (error: unknown) {
      if (disposed || requestRevision !== revision) return;
      previewStatus = "failed";
      console.warn("Notes mention preview failed", { type: target.type, id: target.id, error });
    }
  }

  async function open(): Promise<void> {
    if (!objectMention || !navigable || opening) return;
    opening = true;
    preview.close();
    try {
      if (objectMention.type === "calendar_event") {
        navigation.navigate("calendar");
        return;
      }
      const projectId = objectMention.type === "project" ? objectMention.id : (await resolveTask(objectMention.id)).projectId;
      await projects.selectProject(projectId);
      projects.showProjectView();
      navigation.navigate("projects");
    } catch (error: unknown) {
      console.warn("Notes mention navigation failed", { type: objectMention.type, id: objectMention.id, error });
    } finally {
      opening = false;
    }
  }

  function handleClick(event: MouseEvent): void {
    if (!navigable && !editable) return;
    event.preventDefault();
    event.stopPropagation();
    const moved = pointerStart !== null
      && Math.hypot(event.clientX - pointerStart.x, event.clientY - pointerStart.y) >= POINTER_DRAG_DISTANCE_PX;
    pointerStart = null;
    const selection = element?.ownerDocument.getSelection();
    if (moved || (selection && !selection.isCollapsed && !event.ctrlKey && !event.metaKey)) return;
    if (editable && element) {
      preview.close();
      onEdit?.(element);
    } else {
      void open();
    }
  }

  /** Track mouse and pen drags while allowing touch taps recognized by the browser. */
  function handlePointerDown(event: PointerEvent): void {
    pointerStart = event.pointerType === "touch" ? null : { x: event.clientX, y: event.clientY };
    preview.close();
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.isComposing) return;
    if (event.key === "Escape" && preview.pending) {
      event.preventDefault();
      event.stopPropagation();
      preview.close();
      return;
    }
    if (event.key !== "Enter" || !navigable) return;
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
</script>{#snippet mentionIcon(size: number | string, strokeWidth: number, projectIconSize: number)}{#if mention.mention.type === "user"}<UserRound {size} {strokeWidth} />{:else if mention.mention.type === "date"}{#if reminder}<Bell {size} {strokeWidth} />{:else}<CalendarDays {size} {strokeWidth} />{/if}{:else if objectMention?.type === "project"}{#if project?.icon}<ProjectIcon name={project.icon} size={projectIconSize} />{:else}<Folder {size} {strokeWidth} />{/if}{:else if objectMention?.type === "project_task"}<ListChecks {size} {strokeWidth} />{:else if objectMention?.type === "calendar_event"}<CalendarDays {size} {strokeWidth} />{/if}{/snippet}{#snippet icon()}<span class="notes-mention-chip-icon" data-notes-editor-sentinel="decoration" aria-hidden="true">{@render mentionIcon("1em", 1.75, PROJECT_ICON_SIZE_PX)}</span>{/snippet}{#snippet previewIcon()}{@render mentionIcon(PREVIEW_ICON_SIZE_PX, 1.5, PREVIEW_ICON_SIZE_PX)}{/snippet}{#if navigable}<span
  {...attributes}
  bind:this={element}
  use:mentionEvents
  class={`notes-mention-chip notes-mention-chip-link ${attributes.class ?? ""}`}
  role="link"
  tabindex={focusable ? 0 : undefined}
  data-notes-mention-chip="true"
  data-notes-mention-type={mentionType}
  aria-describedby={preview.open ? previewId : undefined}
  onpointerenter={(event) => preview.schedule(event)}
  onpointerleave={() => preview.scheduleClose()}
  onpointercancel={() => { pointerStart = null; preview.close(); }}
  onpointermove={(event) => { if (event.buttons) preview.close(); }}
>{#if showIcon}{@render icon()}{/if}{text}</span>{:else}<span
  {...attributes}
  bind:this={element}
  use:mentionEvents
  class={`notes-mention-chip${editable ? " cursor-pointer" : ""} ${attributes.class ?? ""}`}
  aria-haspopup={editable ? "dialog" : undefined}
  data-notes-mention-chip="true"
  data-notes-mention-type={mentionType}
  aria-describedby={preview.open ? previewId : undefined}
  onpointerenter={(event) => preview.schedule(event)}
  onpointerleave={() => preview.scheduleClose()}
  onpointercancel={() => { pointerStart = null; preview.close(); }}
  onpointermove={(event) => { if (event.buttons) preview.close(); }}
>{#if showIcon}{@render icon()}{/if}{text}</span>{/if}{#if preview.open && element}<NotesMentionPreviewCard
  anchor={element}
  id={previewId}
  route={previewHeading.route}
  title={previewHeading.title}
  details={previewHeading.details}
  status={previewStatusText}
  loading={previewStatus === "loading"}
  icon={previewIcon}
  onKeepOpen={() => preview.keepOpen()}
  onLeave={() => preview.scheduleClose()}
  onDismiss={() => preview.close()}
/>{/if}
