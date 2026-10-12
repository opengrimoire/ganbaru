<script lang="ts">
  import type { DistractionsMode } from "$lib/distractions";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { cn } from "$lib/utils";
  import DistractionsModeSelector from "./DistractionsModeSelector.svelte";
  import SwitchField from "$lib/components/ui/SwitchField.svelte";

  type ScheduleToggle = "enabled" | "focus" | "shortBreaks" | "longBreaks" | "pause";

  let {
    title,
    enabled,
    blockDuringFocus,
    blockDuringShortBreaks,
    blockDuringLongBreaks,
    pauseDuringFocusPause,
    mode = "blacklist",
    enabledLabel,
    enabledDescription,
    focusDescription,
    shortBreakDescription,
    longBreakDescription,
    pauseDescription,
    showMode = true,
    modeHeading = "Website mode",
    modeDescription = "Choose how listed websites are handled",
    blacklistDescription = "Blocks listed websites",
    whitelistDescription = "Only allows listed websites",
    onScheduleChange,
    onModeChange,
  }: {
    title: string;
    enabled: boolean;
    blockDuringFocus: boolean;
    blockDuringShortBreaks: boolean;
    blockDuringLongBreaks: boolean;
    pauseDuringFocusPause: boolean;
    mode?: DistractionsMode;
    enabledLabel: string;
    enabledDescription: string;
    focusDescription: string;
    shortBreakDescription: string;
    longBreakDescription: string;
    pauseDescription: string;
    showMode?: boolean;
    modeHeading?: string;
    modeDescription?: string;
    blacklistDescription?: string;
    whitelistDescription?: string;
    onScheduleChange: (toggle: ScheduleToggle, checked: boolean) => void;
    onModeChange?: (mode: DistractionsMode) => void;
  } = $props();

  const { t } = getLocalization();
</script>

<section class="flex flex-col gap-4">
  <h2 class="px-1 text-[0.866667rem] font-semibold text-foreground">{title}</h2>
  <div class="flex flex-col gap-3">
    <SwitchField
      label={enabledLabel}
      description={enabledDescription}
      checked={enabled}
      onChange={(checked) => onScheduleChange("enabled", checked)}
    />

    <fieldset
      disabled={!enabled}
      aria-disabled={!enabled}
      class={cn(
        "m-0 flex min-w-0 flex-col gap-4 border-0 p-0 transition-opacity",
        !enabled && "opacity-50",
      )}
    >
      <div class="flex flex-col gap-3" aria-label={t("settings.distractions.shared.blockingSchedule")}>
        <SwitchField
          label={t("settings.distractions.shared.blockDuringFocus")}
          description={focusDescription}
          checked={blockDuringFocus}
          onChange={(checked) => onScheduleChange("focus", checked)}
        />
        <SwitchField
          label={t("settings.distractions.shared.blockDuringShortBreaks")}
          description={shortBreakDescription}
          checked={blockDuringShortBreaks}
          onChange={(checked) => onScheduleChange("shortBreaks", checked)}
        />
        <SwitchField
          label={t("settings.distractions.shared.blockDuringLongBreaks")}
          description={longBreakDescription}
          checked={blockDuringLongBreaks}
          onChange={(checked) => onScheduleChange("longBreaks", checked)}
        />
        <SwitchField
          label={t("settings.distractions.shared.pauseIfFocusPaused")}
          description={pauseDescription}
          checked={pauseDuringFocusPause}
          onChange={(checked) => onScheduleChange("pause", checked)}
        />
      </div>

      {#if showMode && onModeChange}
        <div class="flex flex-col gap-2 px-1">
          <div class="min-w-0">
            <h3 class="text-[0.866667rem] text-foreground">{modeHeading}</h3>
            <div class="mt-0.5 text-[0.8rem] text-muted-foreground">
              {modeDescription}
            </div>
          </div>
          <DistractionsModeSelector
            {mode}
            {blacklistDescription}
            {whitelistDescription}
            onChange={onModeChange}
          />
        </div>
      {/if}
    </fieldset>
  </div>
</section>
