<script lang="ts">
  import type { EventColor } from "$lib/calendar/types";
  import { EVENT_COLOR_OPTIONS } from "$lib/calendar/utils";
  import type { IconPickerColor } from "$lib/projects/icons/picker";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { cn } from "$lib/utils";
  import Switch from "$lib/components/ui/Switch.svelte";

  let {
    open = $bindable(false), color, label, askEveryTime, colorSelectionBorder,
    automaticColor, colorLabel, colorSwatch, onSelect, onOpen,
    onAskEveryTimeChange, disabled = false,
  }: {
    open?: boolean;
    color: IconPickerColor;
    label: string;
    askEveryTime: boolean;
    colorSelectionBorder: string;
    automaticColor: string;
    colorLabel: (color: IconPickerColor) => string;
    colorSwatch: (color: EventColor) => string;
    onSelect: (color: IconPickerColor) => void;
    onOpen: () => void;
    onAskEveryTimeChange: (enabled: boolean) => void;
    disabled?: boolean;
  } = $props();
  const { t } = getLocalization();
</script>

<div class="relative shrink-0" data-icon-picker-inline-panel>
  <button
    type="button"
    {disabled}
    class={cn(
      "flex size-8 items-center justify-center rounded-floating-item border border-border hover:bg-accent",
      open && "bg-accent text-foreground",
    )}
    aria-label={label}
    data-app-tooltip-disabled="true"
    onclick={(event) => {
      event.stopPropagation();
      const nextOpen = !open;
      open = nextOpen;
      if (nextOpen) onOpen();
    }}
  >
    <span
      class="h-4 w-4 rounded-full border border-border"
      style={`background: ${color === "default" ? automaticColor : colorSwatch(color)};`}
    ></span>
  </button>
  {#if open}
    <div
      class="surface-floating absolute right-0 top-9 z-10 w-40 px-2.5 py-2"
      style={`background-color: var(--icon-picker-bg); color: var(--icon-picker-text); --project-icon-color-selection-border: ${colorSelectionBorder};`}
    >
      <button
        type="button"
        {disabled}
        class={cn(
          "grid h-8 w-full items-center justify-center gap-2 rounded-floating-item text-left text-foreground hover:bg-accent",
          color === "default" && "bg-accent/70",
        )}
        style="grid-template-columns: repeat(4, 1.375rem);"
        aria-label={colorLabel("default")}
        data-app-tooltip-disabled="true"
        onclick={(event) => {
          event.stopPropagation();
          onSelect("default");
          open = false;
        }}
      >
        <span
          class="size-5.5 rounded-full"
          style={`background-color: ${automaticColor};`}
        ></span>
        <span class="col-span-3 min-w-0 truncate">{colorLabel("default")}</span>
      </button>
      <div class="mt-1 grid justify-center gap-2" style="grid-template-columns: repeat(4, 1.375rem);">
        {#each EVENT_COLOR_OPTIONS as slot}
          <button
            type="button"
            {disabled}
            class={cn(
              "project-icon-color-swatch relative size-5.5 rounded-full",
              color === slot && "swatch-selected",
            )}
            style={`background-color: ${colorSwatch(slot)};`}
            aria-label={colorLabel(slot)}
            data-app-tooltip-disabled="true"
            onclick={(event) => {
              event.stopPropagation();
              onSelect(slot);
              open = false;
            }}
          ></button>
        {/each}
      </div>
      <div class="mx-1.5 mt-2 h-px bg-border/70" aria-hidden="true"></div>
      <div class="mt-1 flex min-h-8 items-center justify-between gap-2 px-1.5 text-foreground">
        <span class="min-w-0">{t("projects.iconPicker.askEveryTime")}</span>
        <Switch
          size="compact"
          checked={askEveryTime}
          onChange={onAskEveryTimeChange}
          ariaLabel={t("projects.iconPicker.askEveryTime")}
          {disabled}
        />
      </div>
    </div>
  {/if}
</div>

<style>
  .project-icon-color-swatch {
    overflow: hidden;
  }

  .project-icon-color-swatch.swatch-selected::after {
    content: "";
    position: absolute;
    inset: 0;
    border: 2px solid var(--project-icon-color-selection-border);
    border-radius: inherit;
    pointer-events: none;
  }
</style>
