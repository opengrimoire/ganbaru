<script lang="ts">
  import { onMount, tick } from "svelte";
  import {
    CalendarConflictError,
    type CalendarConflictChoice,
    type CalendarConflictVersion,
    type CalendarFieldConflict,
  } from "$lib/api/calendar-conflicts";
  import { resolveCalendarConflict } from "$lib/api/calendar-conflicts";
  import {
    previewConflictText,
    summarizeTiming,
  } from "$lib/calendar/conflicts";
  import SyncConflictCard from "$lib/components/sync/SyncConflictCard.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    activateModalFocus,
    activateModalKeyboardLayer,
    trapModalTabKey,
  } from "$lib/modal-focus";
  import { BUILD_PLATFORM_PROFILE, platformHasCapability } from "$lib/platform";
  import { calendarConflicts } from "$lib/stores/calendar-conflicts.svelte";
  import { getMobileBackStack } from "$lib/stores/mobile-back-stack.svelte";
  import { formatSyncTime, syncDeviceName } from "$lib/sync/status";
  import { createBackdropPress } from "$lib/utils/backdrop-press";

  let {
    eventId,
    onClose,
  }: {
    eventId: string;
    /** Called when the dialog closes; `resolved` is true when at least one conflict was resolved. */
    onClose: (resolved: boolean) => void;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const mobileBackStack = getMobileBackStack();
  const androidSystemBackAvailable = platformHasCapability(BUILD_PLATFORM_PROFILE, "system.android-back");
  const backdropPress = createBackdropPress();

  const conflicts = $derived(calendarConflicts.forEvent(eventId));
  let element = $state<HTMLDivElement | undefined>();
  let closeButton = $state<HTMLButtonElement | null>(null);
  let busy = $state(false);
  let resolvedAny = $state(false);
  let messages = $state<Record<string, { text: string; kind: "resolved" | "error" }>>({});

  $effect(() => {
    void calendarConflicts.revision;
    void calendarConflicts.ensure();
  });

  $effect(() => {
    if (resolvedAny && conflicts.length === 0) onClose(true);
  });

  function close(): void {
    onClose(resolvedAny);
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key === "Tab" && element) {
      trapModalTabKey(element, event);
      event.stopPropagation();
      return;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      close();
      return;
    }
    event.stopPropagation();
  }

  onMount(() => {
    const deactivateMobileBack = androidSystemBackAvailable
      ? mobileBackStack.activate({ handle: close })
      : () => undefined;
    const deactivateKeyboard = activateModalKeyboardLayer(handleKeydown);
    let deactivateModalFocus = (): void => undefined;
    void tick().then(() => {
      if (element) deactivateModalFocus = activateModalFocus(element, closeButton);
    });
    return () => {
      deactivateMobileBack();
      deactivateKeyboard();
      deactivateModalFocus();
    };
  });

  function title(conflict: CalendarFieldConflict): string {
    if (conflict.field === "timing") return t("calendar.conflict.timingTitle");
    return conflict.field === "title"
      ? t("calendar.conflict.titleTitle")
      : t("calendar.conflict.descriptionTitle");
  }

  function source(version: CalendarConflictVersion): string {
    return t(
      "calendar.conflict.versionFrom",
      syncDeviceName(version.device, t("calendar.conflict.thisDevice"), t("calendar.conflict.otherDevice")),
      formatSyncTime(t, localization.locale, version.editedAtMs, Date.now()),
    );
  }

  /** Lines that describe a timing version: start, end, zone, recurrence, and the counts that travel with it. */
  function timingLines(version: CalendarConflictVersion): string[] {
    if (version.value.kind !== "timing") return [];
    const summary = summarizeTiming(version.value, localization.locale, t);
    const lines = [
      t("calendar.conflict.starts", summary.start),
      t("calendar.conflict.ends", summary.end),
    ];
    if (summary.allDay) lines.push(t("calendar.conflict.allDay"));
    else if (summary.timezone) lines.push(t("calendar.conflict.timezone", summary.timezone));
    lines.push(summary.recurrence ?? t("calendar.conflict.doesNotRepeat"));
    const counts = [
      [summary.excludedDates, "calendar.conflict.excludedDates"],
      [summary.addedDates, "calendar.conflict.addedDates"],
      [summary.overrides, "calendar.conflict.overrides"],
    ] as const;
    for (const [count, key] of counts) {
      if (count > 0) lines.push(t(key, count));
    }
    return lines;
  }

  async function resolve(conflict: CalendarFieldConflict, choice: CalendarConflictChoice): Promise<void> {
    if (busy) return;
    busy = true;
    const key = conflict.field;
    messages = { ...messages, [key]: { text: "", kind: "error" } };
    try {
      await resolveCalendarConflict({ eventId, field: conflict.field, choice });
      resolvedAny = true;
    } catch (cause: unknown) {
      if (cause instanceof CalendarConflictError && cause.code === "resolved") {
        resolvedAny = true;
        messages = { ...messages, [key]: { text: t("calendar.conflict.resolved"), kind: "resolved" } };
      } else {
        console.warn("Calendar conflict could not be resolved", cause);
        messages = { ...messages, [key]: { text: t("calendar.conflict.failed"), kind: "error" } };
      }
    } finally {
      busy = false;
      calendarConflicts.invalidate();
    }
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="fixed z-90 flex items-center justify-center"
  style="left: var(--visual-viewport-offset-left); top: var(--visual-viewport-offset-top); width: var(--visual-viewport-width); height: var(--visual-viewport-height); padding: calc(var(--safe-area-top) + 1rem) calc(var(--safe-area-right) + 1rem) calc(var(--safe-area-bottom) + 1rem) calc(var(--safe-area-left) + 1rem);"
  onpointerdown={backdropPress.start}
  onclick={(e) => { e.stopPropagation(); if (backdropPress.isClick(e)) close(); }}
>
  <div class="surface-backdrop absolute inset-0"></div>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    bind:this={element}
    class="surface-dialog relative z-10 flex max-h-full w-full max-w-lg flex-col gap-3 overflow-y-auto px-5 py-5 outline-none sm:px-6"
    style="--foreground: var(--card-foreground);"
    role="dialog"
    aria-modal="true"
    aria-label={t("calendar.conflict.dialogTitle")}
    tabindex="-1"
    onclick={(e) => e.stopPropagation()}
    onkeydown={handleKeydown}
  >
    <h2 class="text-[1rem] font-semibold text-foreground">{t("calendar.conflict.dialogTitle")}</h2>
    {#each conflicts as conflict (conflict.field)}
      {@const message = messages[conflict.field]}
      <SyncConflictCard
        title={title(conflict)}
        hint={t("calendar.conflict.hint")}
        message={message?.text ?? ""}
        messageKind={message?.kind ?? "error"}
      >
        {#each conflict.versions as version (version.version)}
          <div class="flex min-w-0 flex-col gap-1 rounded-md bg-black/5 px-2 py-1.5 dark:bg-white/5">
            {#if version.value.kind === "timing"}
              <ul class="flex flex-col gap-0.5">
                {#each timingLines(version) as line (line)}
                  <li class="wrap-anywhere">{line}</li>
                {/each}
              </ul>
            {:else}
              <div class="max-h-32 overflow-y-auto whitespace-pre-wrap wrap-anywhere">
                {#if version.value.text.trim()}
                  {previewConflictText(version.value.text)}
                {:else}
                  <span class="text-muted-foreground">{t("calendar.conflict.empty")}</span>
                {/if}
              </div>
            {/if}
            <div class="flex flex-wrap items-baseline gap-x-2 text-muted-foreground">
              <span>{source(version)}</span>
              {#if version.displayed}<span>{t("calendar.conflict.shown")}</span>{/if}
            </div>
            <div class="flex flex-wrap items-center gap-1">
              <button
                class="rounded-md px-1.5 py-0.5 font-medium underline hover:bg-black/10 disabled:opacity-40 dark:hover:bg-white/10"
                type="button"
                disabled={busy}
                onclick={() => void resolve(
                  conflict,
                  version.displayed ? { kind: "displayed" } : { kind: "version", version: version.version },
                )}
              >{version.displayed ? t("calendar.conflict.keepThis") : t("calendar.conflict.useOther")}</button>
            </div>
          </div>
        {/each}
      </SyncConflictCard>
    {/each}
    <div class="flex justify-start">
      <button
        type="button"
        bind:this={closeButton}
        onclick={close}
        class="min-h-12 rounded-md border border-border bg-card px-3.5 py-2 text-[0.866667rem] font-medium text-foreground transition-colors hover:bg-accent"
      >{t("calendar.conflict.close")}</button>
    </div>
  </div>
</div>
