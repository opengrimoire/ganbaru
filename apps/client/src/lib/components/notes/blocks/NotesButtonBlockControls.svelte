<script lang="ts">
  import Select from "$lib/components/ui/Select.svelte";
  import CopyPlus from "@lucide/svelte/icons/copy-plus";
  import MousePointerClick from "@lucide/svelte/icons/mouse-pointer-click";
  import Plus from "@lucide/svelte/icons/plus";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    NOTES_BUTTON_NATIVE_ICON_CHOICES,
    NOTES_BUTTON_UNSUPPORTED_ACTIONS,
    notesButtonPrimaryInsertPosition,
    type NotesButtonBlockStatus,
    type NotesButtonNativeIconChoice,
    type NotesButtonUnsupportedAction,
  } from "$lib/notes/block-types/button";
  import {
    NOTES_PAGE_ICON_COLOR_CHOICES,
    createNotesNativePageIcon,
    notesPageNativeIconColor,
  } from "$lib/notes/pages/icon";
  import type {
    NotesButtonBlockPayload,
    NotesButtonInsertPosition,
    NotesIcon,
    NotesIconColor,
  } from "$lib/notes/types";
  import NotesPageIcon from "$lib/components/notes/pages/NotesPageIcon.svelte";

  const BUTTON_INSERT_POSITIONS: readonly NotesButtonInsertPosition[] = [
    "below_button",
    "above_button",
    "top_of_page",
    "bottom_of_page",
  ];

  let {
    blockId,
    title,
    button,
    status,
    onUseButton,
    onAddButtonChild,
    onButtonIconChange,
    onButtonInsertPositionChange,
  }: {
    blockId: string;
    title: string;
    button: NotesButtonBlockPayload;
    status: NotesButtonBlockStatus;
    onUseButton: (blockId: string) => void;
    onAddButtonChild: (blockId: string) => void;
    onButtonIconChange: (blockId: string, icon: NotesIcon | null) => void;
    onButtonInsertPositionChange: (
      blockId: string,
      position: NotesButtonInsertPosition,
    ) => void;
  } = $props();

  const { t } = getLocalization();
  const statusId = $derived(`notes-button-status-${blockId}`);
  const unsupportedId = $derived(`notes-button-unsupported-${blockId}`);
  const iconChoice = $derived(buttonIconChoice(button.icon));
  const iconColor = $derived(button.icon?.type === "icon" ? button.icon.icon.color ?? "gray" : "gray");
  const insertPosition = $derived(notesButtonPrimaryInsertPosition(button));
  const statusText = $derived(buttonStatusText());

  function buttonStatusText(): string {
    switch (status.useUnavailableReason) {
      case "empty":
        return t("notes.buttonEmpty");
      case "child_page":
        return t("notes.buttonChildPageUnsupported");
      case null:
        return t("notes.buttonChildCount", status.childCount);
    }
  }

  function buttonIconChoice(icon: NotesIcon | null): NotesButtonNativeIconChoice | "custom" | "none" {
    if (!icon) return "none";
    if (icon.type !== "icon") return "custom";
    const name = icon.icon.name;
    return NOTES_BUTTON_NATIVE_ICON_CHOICES.includes(name as NotesButtonNativeIconChoice)
      ? name as NotesButtonNativeIconChoice
      : "custom";
  }

  function iconLabel(choice: NotesButtonNativeIconChoice): string {
    switch (choice) {
      case "mouse-pointer-click":
        return t("notes.buttonIconClick");
      case "plus":
        return t("notes.buttonIconPlus");
      case "copy":
        return t("notes.buttonIconCopy");
      case "list-plus":
        return t("notes.buttonIconListPlus");
      case "check":
        return t("notes.buttonIconCheck");
      case "star":
        return t("notes.buttonIconStar");
      case "calendar-days":
        return t("notes.buttonIconCalendar");
      case "lightbulb":
        return t("notes.buttonIconLightbulb");
    }
  }

  function positionLabel(position: NotesButtonInsertPosition): string {
    switch (position) {
      case "below_button":
        return t("notes.buttonPositionBelow");
      case "above_button":
        return t("notes.buttonPositionAbove");
      case "top_of_page":
        return t("notes.buttonPositionTop");
      case "bottom_of_page":
        return t("notes.buttonPositionBottom");
    }
  }

  function unsupportedActionLabel(action: NotesButtonUnsupportedAction): string {
    switch (action) {
      case "database_edit":
        return t("notes.buttonUnsupportedDatabaseEdit");
      case "webhook":
        return t("notes.buttonUnsupportedWebhook");
      case "destructive_automation":
        return t("notes.buttonUnsupportedDestructiveAutomation");
    }
  }

  function selectIcon(choice: string): void {
    if (choice === "none") {
      onButtonIconChange(blockId, null);
      return;
    }
    if (choice === "custom") return;
    onButtonIconChange(blockId, createNotesNativePageIcon(choice, iconColor));
  }

  function selectIconColor(color: string): void {
    if (button.icon?.type !== "icon") return;
    onButtonIconChange(blockId, createNotesNativePageIcon(button.icon.icon.name, color as NotesIconColor));
  }
</script>

<div
  class="mb-1 flex min-w-0 flex-wrap items-center justify-between gap-2 rounded-md border border-border bg-background/70 px-2 py-1.5 text-[0.8rem]"
>
  <div class="min-w-0">
    <span class="block min-w-0 truncate font-medium text-foreground">
      {t("notes.buttonSourceContent")}
    </span>
    <span id={statusId} class="block min-w-0 text-muted-foreground">
      {statusText}
    </span>
  </div>
  <div class="flex min-w-0 flex-wrap items-center gap-1.5">
    <div
      class="inline-flex min-h-7 max-w-full items-center gap-1.5 rounded border border-border bg-background px-2 text-muted-foreground"
    >
      <NotesPageIcon icon={button.icon} size={14} strokeWidth={1.8} />
      <span class="sr-only">{t("notes.buttonIcon")}</span>
      <Select
        inline
        appearance="quiet"
        contentAlign="start"
        class="w-full min-w-0"
        ariaLabel={t("notes.buttonIcon")}
        value={String(iconChoice ?? "")}
        options={[{ value: "none", label: t("notes.noButtonIcon") },
          ...(iconChoice === "custom" ? [{ value: "custom", label: t("notes.buttonImportedIcon") }] : []),
          ...(NOTES_BUTTON_NATIVE_ICON_CHOICES).map((choice) => ({ value: String(choice), label: String(iconLabel(choice)) }))]}
        onChange={(nextValue) => selectIcon(nextValue)}
      />
    </div>
    {#if button.icon?.type === "icon"}
      <div class="flex min-h-7 items-center gap-1 rounded border border-border bg-background px-1.5">
        {#each NOTES_PAGE_ICON_COLOR_CHOICES as color}
          {@const selected = iconColor === color}
          <button
            type="button"
            class="size-4 rounded-full border"
            class:border-foreground={selected}
            class:border-border={!selected}
            style={`background: ${notesPageNativeIconColor(color) ?? "#646470"}`}
            aria-label={t("notes.buttonIconColor", color)}
            data-app-tooltip={t("notes.buttonIconColor", color)}
            onclick={() => selectIconColor(color)}
          ></button>
        {/each}
      </div>
    {/if}
    <div
      class="inline-flex min-h-7 max-w-full items-center gap-1.5 rounded border border-border bg-background px-2 text-muted-foreground"
    >
      <CopyPlus class="size-3.5" aria-hidden="true" />
      <span class="sr-only">{t("notes.buttonActionPosition")}</span>
      <Select
        inline
        appearance="quiet"
        contentAlign="start"
        class="w-full min-w-0"
        ariaLabel={t("notes.buttonActionPosition")}
        value={String(insertPosition ?? "")}
        options={[...(BUTTON_INSERT_POSITIONS).map((position) => ({ value: String(position), label: String(positionLabel(position)) }))]}
        onChange={(nextValue) => onButtonInsertPositionChange(blockId, nextValue as NotesButtonInsertPosition)}
      />
    </div>
    <button
      type="button"
      class="inline-flex min-h-7 items-center gap-1.5 rounded border border-border bg-background px-2 font-medium text-foreground hover:bg-accent"
      onclick={() => onAddButtonChild(blockId)}
    >
      <Plus class="size-3.5" aria-hidden="true" />
      <span>{t("notes.addButtonSourceContent")}</span>
    </button>
    <button
      type="button"
      class="inline-flex min-h-7 items-center gap-1.5 rounded border border-border bg-background px-2 font-medium text-foreground hover:bg-accent disabled:cursor-not-allowed disabled:opacity-45"
      aria-label={t("notes.useButton", title)}
      aria-describedby={`${statusId} ${unsupportedId}`}
      disabled={!status.canUse}
      onclick={() => onUseButton(blockId)}
    >
      <MousePointerClick class="size-3.5" aria-hidden="true" />
      <span>{t("notes.useButtonButton")}</span>
    </button>
  </div>
  <div id={unsupportedId} class="flex basis-full flex-wrap items-center gap-1 text-[0.733333rem] text-muted-foreground">
    <span>{t("notes.buttonUnsupportedActions")}</span>
    {#each NOTES_BUTTON_UNSUPPORTED_ACTIONS as action}
      <button
        type="button"
        class="rounded border border-border px-1.5 py-0.5 text-muted-foreground opacity-60"
        disabled
        aria-disabled="true"
      >
        {unsupportedActionLabel(action)}
      </button>
    {/each}
  </div>
</div>
