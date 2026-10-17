<script lang="ts">
  import type { EventAttendee, EventOrganizer, EventSurfaceStatus, GeoCoordinates } from "$lib/calendar/types";
  import { panelInputKeydown } from "$lib/calendar/event-panel-utils";
  import { createSmoothScroll } from "$lib/calendar/utils";
  import DescriptionEditor from "./DescriptionEditor.svelte";
  import { slide } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import Users from "@lucide/svelte/icons/users";
  import MapPin from "@lucide/svelte/icons/map-pin";
  import Video from "@lucide/svelte/icons/video";
  import Check from "@lucide/svelte/icons/check";
  import CircleHelp from "@lucide/svelte/icons/circle-help";
  import Minus from "@lucide/svelte/icons/minus";
  import X from "@lucide/svelte/icons/x";
  import Flag from "@lucide/svelte/icons/flag";
  import Plus from "@lucide/svelte/icons/plus";
  import Pencil from "@lucide/svelte/icons/pencil";
  import UserPlus from "@lucide/svelte/icons/user-plus";
  import Eye from "@lucide/svelte/icons/eye";
  import { formatList } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import ParticipantPicker from "$lib/components/contacts/ParticipantPicker.svelte";

  let {
    enabled,
    url = $bindable(""),
    location = $bindable(""),
    geo,
    attendees = $bindable([]),
    localParticipationStatus = $bindable(undefined),
    guestCanModify = $bindable(false),
    guestCanInviteOthers = $bindable(true),
    guestCanSeeOtherGuests = $bindable(true),
    organizer,
    selfEmail,
    eventTitle = "",
    description,
    readOnly = false,
    allowReadOnlyExpand = false,
    expanded,
    onChange,
    onDescriptionChange,
    onExpand,
    onSurfaceStatusChange,
    onToggle,
  }: {
    enabled: boolean;
    url: string;
    location: string;
    geo?: GeoCoordinates;
    attendees: EventAttendee[];
    localParticipationStatus?: EventAttendee["status"];
    guestCanModify: boolean;
    guestCanInviteOthers: boolean;
    guestCanSeeOtherGuests: boolean;
    organizer?: EventOrganizer;
    selfEmail?: string;
    /** Names the event when guests are picked from contacts or invited. */
    eventTitle?: string;
    description: string;
    readOnly?: boolean;
    allowReadOnlyExpand?: boolean;
    expanded: boolean;
    onChange: () => void;
    onDescriptionChange: (html: string) => void;
    onExpand: () => void;
    onSurfaceStatusChange?: (status: EventSurfaceStatus | undefined) => void;
    onToggle: () => void;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const locale = $derived(localization.locale);

  const hasContent = $derived(
    attendees.length > 0 || !!organizer || !!url || !!location,
  );

  const organizerEmail = $derived(organizer?.email.toLowerCase() ?? "");
  const normalizedSelfEmail = $derived(selfEmail?.toLowerCase() ?? "");
  const isOrganizerSelf = $derived(!!normalizedSelfEmail && organizerEmail === normalizedSelfEmail);
  const visibleAttendees = $derived(
    organizerEmail
      ? attendees.filter((attendee) => attendee.email.toLowerCase() !== organizerEmail)
      : attendees,
  );
  const selfAttendee = $derived(
    normalizedSelfEmail
      ? visibleAttendees.find((attendee) => attendee.email.toLowerCase() === normalizedSelfEmail)
      : undefined,
  );
  const guestAttendees = $derived(
    selfAttendee
      ? visibleAttendees.filter((attendee) => attendee.id !== selfAttendee.id)
      : visibleAttendees,
  );
  const showLocalSelfRow = $derived(enabled && !organizer && !selfAttendee);
  const effectiveLocalSelfStatus = $derived(localParticipationStatus ?? "accepted");
  const canEditGuests = $derived(!readOnly && !organizer);
  let attendeePickerButton = $state<HTMLButtonElement | null>(null);
  let attendeePickerOpen = $state(false);
  const attendeePickerSpace = $derived({ kind: "event" as const, name: eventTitle.trim() || t("calendar.event.noTitle") });
  const surfaceStatus = $derived.by<EventSurfaceStatus | undefined>(() => {
    if (!enabled) return undefined;
    if (selfAttendee) return selfAttendee.status;
    if (showLocalSelfRow) return effectiveLocalSelfStatus;
    if (isOrganizerSelf) return "accepted";
    return undefined;
  });

  /** Collapsed summary: attendee count, location, URL host, joined with middle dots. */
  const summary = $derived.by(() => {
    if (!enabled) return "";
    const parts: string[] = [];
    const localSelfCount = showLocalSelfRow ? 1 : 0;
    const peopleCount = visibleAttendees.length + (organizer ? 1 : 0) + localSelfCount;
    if (peopleCount > 0) {
      parts.push(
        organizer
          ? t("calendar.meeting.people", peopleCount)
          : t("calendar.meeting.attendees", peopleCount),
      );
    }
    if (location) parts.push(location);
    if (url) {
      let host = "";
      try { host = new URL(url).host; } catch { host = ""; }
      if (host) parts.push(host);
    }
    return formatList(locale, parts);
  });

  let urlInput: HTMLInputElement | undefined = $state();

  // When the section opens into an empty state, focus the URL input so the
  // "start a meeting" shortcut drops the caret on the first field the user sees.
  let wasExpanded = false;
  $effect(() => {
    const nowExpanded = expanded;
    if (nowExpanded && !wasExpanded && !hasContent && !readOnly) {
      setTimeout(() => urlInput?.focus(), 220);
    }
    wasExpanded = nowExpanded;
  });

  let attendeeInput = $state("");

  $effect(() => {
    onSurfaceStatusChange?.(surfaceStatus);
  });

  function addAttendee() {
    const email = attendeeInput.trim();
    const normalized = email.toLowerCase();
    if (!email || organizerEmail === normalized || attendees.some((a) => a.email.toLowerCase() === normalized)) return;
    attendees = [...attendees, {
      id: crypto.randomUUID(),
      email,
      role: "req-participant",
      status: "needs-action",
      rsvp: true,
    }];
    attendeeInput = "";
    onChange();
  }

  function removeAttendee(id: string) {
    attendees = attendees.filter((a) => a.id !== id);
    onChange();
  }

  function toggleAttendeeOptional(id: string) {
    attendees = attendees.map((a) =>
      a.id === id
        ? { ...a, role: a.role === "opt-participant" ? "req-participant" : "opt-participant" }
        : a,
    );
    onChange();
  }

  function attendeeStatusLabel(status: EventAttendee["status"]): string {
    if (status === "needs-action") return t("calendar.meeting.statusPending");
    if (status === "accepted") return t("calendar.meeting.statusAccepted");
    if (status === "tentative") return t("calendar.meeting.statusTentative");
    return t("calendar.meeting.statusDeclined");
  }

  function optionalActionClass(disabled: boolean, optional: boolean): string {
    if (disabled) return "cursor-not-allowed rounded p-0.5 text-muted-foreground/20";
    return `rounded p-0.5 active:scale-75 ${optional ? "text-muted-foreground/40" : "text-foreground"}`;
  }

  function removeActionClass(disabled: boolean): string {
    if (disabled) return "cursor-not-allowed rounded p-0.5 text-muted-foreground/20";
    return "rounded p-0.5 text-muted-foreground active:scale-75 hover:text-destructive";
  }

  function nextRsvpStatus(status: EventAttendee["status"]): EventAttendee["status"] {
    if (status === "accepted") return "tentative";
    if (status === "tentative") return "declined";
    if (status === "declined") return "needs-action";
    return "accepted";
  }

  function toggleLocalSelfRsvp() {
    const nextStatus = nextRsvpStatus(effectiveLocalSelfStatus);
    localParticipationStatus = nextStatus === "accepted" ? undefined : nextStatus;
    onChange();
  }

  let scrollEl: HTMLDivElement | undefined = $state();
  let fadeTop = $state(false);
  let fadeBottom = $state(false);

  function updateFade() {
    const el = scrollEl;
    if (!el) { fadeTop = false; fadeBottom = false; return; }
    fadeTop = el.scrollTop > 2;
    fadeBottom = el.scrollTop + el.clientHeight < el.scrollHeight - 2;
  }

  function observeResize(node: HTMLElement) {
    const observer = new ResizeObserver(() => updateFade());
    observer.observe(node);
    return { destroy: () => observer.disconnect() };
  }

  const onWheel = createSmoothScroll(() => scrollEl, 2, 8);
</script>

<div class="flex flex-col rounded-none overflow-hidden" style="background-color: var(--panel-contrast);">
  <div class="section-header flex items-stretch">
    <button onclick={onToggle}
      disabled={readOnly}
      class="flex w-10 shrink-0 items-center justify-center
        {enabled ? 'bg-black/3 dark:bg-black/30 text-foreground' : 'text-muted-foreground/50'}">
      <Users size={14} />
    </button>
    <button onclick={onExpand}
      disabled={readOnly && !allowReadOnlyExpand}
      class="flex flex-1 items-center gap-2.5 px-3 py-2 text-left {allowReadOnlyExpand ? 'readonly-interactive' : ''}">
      <span class="translate-y-[1.13px] text-[0.8rem] {enabled ? 'text-foreground' : 'text-muted-foreground'}">{t("calendar.meeting.title")}</span>
      {#if enabled && summary}
        <span class="ml-auto translate-y-[1.13px] truncate text-[0.733333rem] text-muted-foreground">{summary}</span>
      {/if}
    </button>
  </div>
  {#if expanded}
    <div transition:slide={{ duration: 180, easing: cubicOut }} data-section="meeting" class="flex flex-col gap-2.5 px-3.5 pb-3 pt-3" style="background-color: var(--panel-bg);">
      <!-- URL -->
      <div class="meeting-detail-row flex items-center gap-3 text-[0.8rem] leading-none">
        <Video size={14} class="shrink-0 text-foreground" />
        <input bind:this={urlInput} type="url" bind:value={url} placeholder={t("calendar.meeting.addCallLink")}
          disabled={readOnly}
          class="min-w-0 flex-1 bg-transparent leading-none text-foreground outline-none placeholder:text-muted-foreground/40"
          oninput={onChange} onkeydown={panelInputKeydown} />
      </div>
      <!-- Location -->
      <div class="meeting-detail-row flex items-center gap-3 text-[0.8rem] leading-none">
        <MapPin size={14} class="shrink-0 text-foreground" />
        <input type="text" bind:value={location} placeholder={t("calendar.meeting.addLocation")}
          disabled={readOnly}
          class="min-w-0 flex-1 bg-transparent leading-none text-foreground outline-none placeholder:text-muted-foreground/40"
          oninput={onChange} onkeydown={panelInputKeydown} />
        {#if geo}
          <span class="shrink-0 text-[0.733333rem] text-muted-foreground/60">({geo.lat.toFixed(2)}, {geo.lng.toFixed(2)})</span>
        {/if}
      </div>
      <!-- Description -->
      <div class="meeting-detail-row flex items-center">
        <div class="min-w-0 flex-1">
          <DescriptionEditor {description} {readOnly} onChange={onDescriptionChange} />
        </div>
      </div>
      <!-- Guests divider -->
      <div class="-mx-3.5 border-t border-border/40"></div>
      <!-- Guests -->
      <div class="flex flex-col">
        <div class="flex items-center gap-2 pb-1">
          <span class="text-[0.666667rem] uppercase tracking-wider text-muted-foreground">{t("calendar.meeting.guests")}</span>
          {#if canEditGuests && guestAttendees.length > 0}
            <div class="ml-auto flex flex-wrap items-center justify-end gap-1">
              {#each [
                { icon: Pencil, label: t("calendar.meeting.edit"), title: t("calendar.meeting.modifyEvent"), get: () => guestCanModify, set: (v: boolean) => { guestCanModify = v; onChange(); } },
                { icon: UserPlus, label: t("calendar.meeting.invite"), title: t("calendar.meeting.inviteOthers"), get: () => guestCanInviteOthers, set: (v: boolean) => { guestCanInviteOthers = v; onChange(); } },
                { icon: Eye, label: t("calendar.meeting.seeList"), title: t("calendar.meeting.seeGuestList"), get: () => guestCanSeeOtherGuests, set: (v: boolean) => { guestCanSeeOtherGuests = v; onChange(); } },
              ] as permission}
                <button onclick={() => permission.set(!permission.get())}
                  title={permission.title}
                  aria-label={permission.title}
                  class="flex items-center gap-1 rounded px-1.5 py-0.5 active:scale-95
                    {permission.get() ? 'bg-foreground/10 text-foreground' : 'bg-foreground/5 text-muted-foreground/30 hover:text-muted-foreground/50'}">
                  <permission.icon size={12} strokeWidth={2} />
                  <span class="text-[0.666667rem] max-[320px]:hidden">{permission.label}</span>
                </button>
              {/each}
            </div>
          {/if}
        </div>
        {#if organizer}
          <div class="flex items-center gap-2.5 py-0.5 text-[0.8rem]">
            <span class="flex h-4 w-4 shrink-0 items-center justify-center rounded-[3px] bg-status-accepted"><Check size={11} strokeWidth={2.5} class="block text-status-accepted-foreground" /></span>
            <span class="min-w-0 flex-1 truncate text-foreground">
              {isOrganizerSelf ? t("calendar.meeting.youWithEmail", organizer.email) : organizer.name ?? organizer.email}
            </span>
            <span class="shrink-0 text-[0.733333rem] text-muted-foreground/60">{t("calendar.meeting.organizer")}</span>
            <div class="flex shrink-0 items-center gap-0.5">
              <button
                disabled
                title={t("calendar.meeting.organizerCannotBeOptional")}
                aria-label={t("calendar.meeting.organizerCannotBeOptional")}
                class={optionalActionClass(true, false)}>
                <Flag size={12} />
              </button>
              <button
                disabled
                title={t("calendar.meeting.organizerCannotBeRemoved")}
                aria-label={t("calendar.meeting.organizerCannotBeRemoved")}
                class={removeActionClass(true)}>
                <X size={12} />
              </button>
            </div>
          </div>
        {/if}
        {#if selfAttendee}
          {@const selfStatus = selfAttendee.status}
          {@const selfBg = selfStatus === "accepted" ? "bg-status-accepted" : selfStatus === "tentative" ? "bg-status-tentative" : selfStatus === "declined" ? "bg-status-declined" : "bg-muted-foreground/30"}
          {@const selfFg = selfStatus === "accepted" ? "text-status-accepted-foreground" : selfStatus === "tentative" ? "text-status-tentative-foreground" : selfStatus === "declined" ? "text-status-declined-foreground" : "text-foreground"}
          {@const SelfIcon = selfStatus === "accepted" ? Check : selfStatus === "tentative" ? CircleHelp : selfStatus === "declined" ? X : Minus}
          {@const selfStatusLabel = attendeeStatusLabel(selfStatus)}
          <div class="flex items-center gap-2.5 py-0.5 text-[0.8rem]">
            <span
              class="flex h-4 w-4 shrink-0 items-center justify-center rounded-[3px] {selfBg}">
              <SelfIcon size={11} strokeWidth={2.5} class="block {selfFg}" />
            </span>
            <span class="min-w-0 flex-1 truncate text-foreground">{t("calendar.meeting.youWithEmail", selfAttendee.email)}</span>
            <span class="shrink-0 text-[0.733333rem] text-muted-foreground/60">{selfStatusLabel}</span>
            <div class="flex shrink-0 items-center gap-0.5">
              <button
                disabled
                title={t("calendar.meeting.importedAttendeeCannotBeOptional")}
                aria-label={t("calendar.meeting.importedAttendeeCannotBeOptional")}
                class={optionalActionClass(true, selfAttendee.role === "opt-participant")}>
                <Flag size={12} />
              </button>
              <button
                disabled
                title={t("calendar.meeting.importedAttendeeCannotBeRemoved")}
                aria-label={t("calendar.meeting.importedAttendeeCannotBeRemoved")}
                class={removeActionClass(true)}>
                <X size={12} />
              </button>
            </div>
          </div>
        {/if}
        {#if showLocalSelfRow}
          {@const localSelfBg = effectiveLocalSelfStatus === "accepted" ? "bg-status-accepted" : effectiveLocalSelfStatus === "tentative" ? "bg-status-tentative" : effectiveLocalSelfStatus === "declined" ? "bg-status-declined" : "bg-muted-foreground/30"}
          {@const localSelfFg = effectiveLocalSelfStatus === "accepted" ? "text-status-accepted-foreground" : effectiveLocalSelfStatus === "tentative" ? "text-status-tentative-foreground" : effectiveLocalSelfStatus === "declined" ? "text-status-declined-foreground" : "text-foreground"}
          {@const LocalSelfIcon = effectiveLocalSelfStatus === "accepted" ? Check : effectiveLocalSelfStatus === "tentative" ? CircleHelp : effectiveLocalSelfStatus === "declined" ? X : Minus}
          {@const localSelfStatusLabel = attendeeStatusLabel(effectiveLocalSelfStatus)}
          <div class="flex items-center gap-2.5 py-0.5 text-[0.8rem]">
            <button
              type="button"
              onclick={toggleLocalSelfRsvp}
              disabled={readOnly}
              class="flex h-4 w-4 shrink-0 items-center justify-center rounded-[3px] {localSelfBg} {readOnly ? 'cursor-not-allowed opacity-60' : 'active:scale-90'}"
              title={t("calendar.meeting.toggleLocalRsvp", localSelfStatusLabel)}
              aria-label={t("calendar.meeting.toggleLocalRsvp", localSelfStatusLabel)}>
              <LocalSelfIcon size={11} strokeWidth={2.5} class="block {localSelfFg}" />
            </button>
            <span class="min-w-0 flex-1 truncate text-foreground">{t("calendar.meeting.youLocalNoEmail")}</span>
            <span class="shrink-0 text-[0.733333rem] text-muted-foreground/60">{localSelfStatusLabel}</span>
            <div class="flex shrink-0 items-center gap-0.5">
              <button
                disabled
                title={t("calendar.meeting.localUserCannotBeOptional")}
                aria-label={t("calendar.meeting.localUserCannotBeOptional")}
                class={optionalActionClass(true, false)}>
                <Flag size={12} />
              </button>
              <button
                disabled
                title={t("calendar.meeting.localUserCannotBeRemoved")}
                aria-label={t("calendar.meeting.localUserCannotBeRemoved")}
                class={removeActionClass(true)}>
                <X size={12} />
              </button>
            </div>
          </div>
        {/if}
        {#if guestAttendees.length > 0}
          <!-- svelte-ignore binding_property_non_reactive -->
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div bind:this={scrollEl} onscroll={updateFade} onwheel={onWheel}
            use:observeResize
            class="relative max-h-18 overflow-y-auto"
            style:mask-image={fadeTop && fadeBottom ? 'linear-gradient(to bottom, transparent, black 10px, black calc(100% - 10px), transparent)' : fadeTop ? 'linear-gradient(to bottom, transparent, black 10px)' : fadeBottom ? 'linear-gradient(to bottom, black calc(100% - 10px), transparent)' : 'none'}
            style:-webkit-mask-image={fadeTop && fadeBottom ? 'linear-gradient(to bottom, transparent, black 10px, black calc(100% - 10px), transparent)' : fadeTop ? 'linear-gradient(to bottom, transparent, black 10px)' : fadeBottom ? 'linear-gradient(to bottom, black calc(100% - 10px), transparent)' : 'none'}>
            {#each guestAttendees as attendee (attendee.id)}
              {@const statusBgClass = attendee.status === "accepted" ? "bg-status-accepted" : attendee.status === "tentative" ? "bg-status-tentative" : attendee.status === "declined" ? "bg-status-declined" : "bg-muted-foreground/30"}
              {@const statusFgClass = attendee.status === "accepted" ? "text-status-accepted-foreground" : attendee.status === "tentative" ? "text-status-tentative-foreground" : attendee.status === "declined" ? "text-status-declined-foreground" : "text-foreground"}
              {@const StatusIcon = attendee.status === "accepted" ? Check : attendee.status === "tentative" ? CircleHelp : attendee.status === "declined" ? X : Minus}
              {@const statusLabel = attendeeStatusLabel(attendee.status)}
              {@const guestActionsDisabled = !canEditGuests}
              <div class="flex items-center gap-2.5 py-0.5 text-[0.8rem]">
                <span
                  class="flex h-4 w-4 shrink-0 items-center justify-center rounded-[3px] {statusBgClass}">
                  <StatusIcon size={11} strokeWidth={2.5} class="block {statusFgClass}" />
                </span>
                <span class="min-w-0 flex-1 truncate text-foreground">{attendee.name ?? attendee.email}</span>
                <span class="shrink-0 text-[0.733333rem] text-muted-foreground/60">{statusLabel}</span>
                {#if attendee.role === "opt-participant"}
                  <span class="shrink-0 text-[0.733333rem] text-muted-foreground/60 italic">{t("calendar.meeting.optional")}</span>
                {/if}
                <div class="flex shrink-0 items-center gap-0.5">
                  <button onclick={() => toggleAttendeeOptional(attendee.id)}
                    disabled={guestActionsDisabled}
                    title={guestActionsDisabled ? t("calendar.meeting.attendeeRolesReadOnly") : attendee.role === "opt-participant" ? t("calendar.meeting.markRequired") : t("calendar.meeting.markOptional")}
                    aria-label={guestActionsDisabled ? t("calendar.meeting.attendeeRolesReadOnly") : attendee.role === "opt-participant" ? t("calendar.meeting.markRequired") : t("calendar.meeting.markOptional")}
                    class={optionalActionClass(guestActionsDisabled, attendee.role === "opt-participant")}>
                    <Flag size={12} />
                  </button>
                  <button onclick={() => removeAttendee(attendee.id)}
                    disabled={guestActionsDisabled}
                    title={guestActionsDisabled ? t("calendar.meeting.attendeesCannotBeRemoved") : t("calendar.meeting.removeAttendee")}
                    aria-label={guestActionsDisabled ? t("calendar.meeting.attendeesCannotBeRemoved") : t("calendar.meeting.removeAttendee")}
                    class={removeActionClass(guestActionsDisabled)}>
                    <X size={12} />
                  </button>
                </div>
              </div>
            {/each}
          </div>
        {/if}
        {#if canEditGuests}
          <div class="flex items-center gap-2.5 py-1">
            <span class="h-4 w-4 shrink-0"></span>
            <input type="email" bind:value={attendeeInput}
              placeholder={t("calendar.meeting.addEmail")}
              class="min-w-0 flex-1 bg-transparent text-[0.8rem] leading-none text-foreground outline-none placeholder:text-muted-foreground/40"
              onkeydown={(e) => {
                e.stopPropagation();
                if (e.key === "Enter") { e.preventDefault(); addAttendee(); }
              }} />
            {#if attendeeInput.trim()}
              <button onclick={addAttendee}
                class="shrink-0 rounded p-0.5 text-muted-foreground hover:text-foreground">
                <Plus size={12} />
              </button>
            {/if}
            <button bind:this={attendeePickerButton} type="button"
              class="shrink-0 rounded p-0.5 text-muted-foreground hover:text-foreground"
              aria-label={t("calendar.meeting.pickAttendees")}
              data-app-tooltip={t("calendar.meeting.pickAttendees")}
              aria-haspopup="dialog"
              aria-expanded={attendeePickerOpen}
              onclick={() => { attendeePickerOpen = true; }}>
              <UserPlus size={13} />
            </button>
          </div>
        {/if}
      </div>
    </div>
  {/if}
</div>

{#if attendeePickerOpen}
  <ParticipantPicker
    anchor={attendeePickerButton}
    title={t("calendar.meeting.pickAttendees")}
    includeContacts
    includeTeammates={false}
    space={attendeePickerSpace}
    onClose={() => { attendeePickerOpen = false; }}
  />
{/if}
