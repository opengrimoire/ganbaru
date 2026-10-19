<script lang="ts">
  import { onMount, tick } from "svelte";
  import AlertTriangle from "@lucide/svelte/icons/triangle-alert";
  import Check from "@lucide/svelte/icons/check";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import Minus from "@lucide/svelte/icons/minus";
  import Search from "@lucide/svelte/icons/search";
  import VolumeX from "@lucide/svelte/icons/volume-x";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { MusicSoundscapeDefinition } from "$lib/music/soundscape/contracts";
  import { orderedGeneratedSounds } from "$lib/music/soundscape/presentation";
  import { soundChoicesEqual, type MusicSoundChoice } from "$lib/music/sound-choice";
  import { OPENING_MAY_FOCUS_TEXT } from "$lib/mobile/keyboard";
  import { getSoundscapeStore } from "$lib/stores/soundscape.svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import { FLOATING_WIDTH } from "$lib/components/ui/floating-width";
  import { cn } from "$lib/utils";
  import { portal } from "$lib/utils/portal";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import { pickSelectPopoverGeometry, type SelectPopoverGeometry } from "$lib/utils/select-popover-position";
  import MusicSoundscapeGroupIcon from "./MusicSoundscapeGroupIcon.svelte";
  import MusicSoundscapeSoundIcon from "./MusicSoundscapeSoundIcon.svelte";

  let {
    value,
    onChange,
    label,
    disabled = false,
    class: className = "",
    triggerClass = "",
  }: {
    value: MusicSoundChoice;
    onChange: (choice: MusicSoundChoice) => void;
    label: string;
    disabled?: boolean;
    class?: string;
    /** Classes merged into the trigger so a host surface can match its own controls. */
    triggerClass?: string;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const soundscape = getSoundscapeStore();
  const id = $props.id();
  const POPOVER_WIDTH_PX = FLOATING_WIDTH.lg;
  const POPOVER_MAX_HEIGHT_PX = 520;
  let open = $state(false);
  let search = $state("");
  let trigger = $state<HTMLButtonElement | null>(null);
  let popover = $state<HTMLDivElement | null>(null);
  let searchInput = $state<HTMLInputElement | null>(null);
  let geometry = $state<SelectPopoverGeometry | null>(null);

  onMount(() => { void soundscape.ensureCatalog(); });

  const loading = $derived(soundscape.loading && soundscape.definitions.length === 0);
  const selectedSound = $derived(value.kind === "sound" ? soundscape.definitions.find((entry) => entry.id === value.soundscapeId) ?? null : null);
  const selectedPreset = $derived(value.kind === "preset" ? soundscape.presets.find((entry) => entry.id === value.presetId) ?? null : null);
  const missing = $derived(!loading && (
    (value.kind === "sound" && selectedSound?.availability !== "available")
    || (value.kind === "preset" && (!selectedPreset || !soundscape.presetIsPlayable(selectedPreset)))
  ));
  const query = $derived(search.trim().toLocaleLowerCase());
  const presets = $derived(soundscape.presets.filter((preset) => matches(preset.name)));
  const generated = $derived(orderedGeneratedSounds(soundscape.definitions).filter((entry) => matches(soundName(entry))));
  const local = $derived(soundscape.definitions.filter((entry) => entry.sourceKind !== "generated-noise" && matches(entry.name)));
  const localSections = $derived.by(() => {
    const grouped = soundscape.groups
      .map((group) => ({ key: group.id, group, sounds: local.filter((entry) => entry.groupId === group.id) }))
      .filter((section) => section.sounds.length > 0);
    const ungrouped = local.filter((entry) => !entry.groupId || !soundscape.groups.some((group) => group.id === entry.groupId));
    return ungrouped.length > 0 ? [...grouped, { key: "ungrouped", group: null, sounds: ungrouped }] : grouped;
  });
  const hasMatches = $derived(presets.length > 0 || generated.length > 0 || local.length > 0);

  /** Display name of a sound; generated sounds use their localized rain names. */
  function soundName(definition: MusicSoundscapeDefinition): string {
    return definition.generatedKind ? t(`music.soundscape.rainName.${definition.generatedKind}`) : definition.name;
  }

  function soundChoice(soundscapeId: string): MusicSoundChoice {
    return { kind: "sound", soundscapeId };
  }

  function presetChoice(presetId: string): MusicSoundChoice {
    return { kind: "preset", presetId };
  }

  function matches(name: string): boolean {
    return !query || name.toLocaleLowerCase().includes(query);
  }

  async function toggle(): Promise<void> {
    if (disabled || loading) return;
    if (open) {
      close();
      return;
    }
    open = true;
    geometry = null;
    await tick();
    if (!open) return;
    position();
    if (OPENING_MAY_FOCUS_TEXT) searchInput?.focus({ preventScroll: true });
  }

  function close(restoreFocus = false): void {
    open = false;
    search = "";
    if (restoreFocus && trigger?.isConnected) queueMicrotask(() => trigger?.focus({ preventScroll: true }));
  }

  function choose(choice: MusicSoundChoice): void {
    if (disabled || loading) return;
    if (!soundChoicesEqual(choice, value)) onChange(choice);
    close(true);
  }

  /** Match the playlist chooser's width and keep the menu within the viewport. */
  function position(): void {
    if (!trigger) return;
    geometry = pickSelectPopoverGeometry({
      triggerRect: trigger.getBoundingClientRect(),
      boundaryRect: {
        top: 8,
        left: 8,
        right: window.innerWidth - 8,
        bottom: window.innerHeight - 8,
        width: window.innerWidth - 16,
        height: window.innerHeight - 16,
      },
      contentHeight: Math.min(popover?.scrollHeight ?? POPOVER_MAX_HEIGHT_PX, POPOVER_MAX_HEIGHT_PX),
      contentWidth: POPOVER_WIDTH_PX,
      horizontalAlign: "end",
    });
  }

  function popoverStyle(): string {
    if (!geometry) return "visibility:hidden;top:0;left:0";
    return `top:${geometry.top}px;left:${geometry.left}px;width:${geometry.width ?? POPOVER_WIDTH_PX}px;max-width:${geometry.maxWidth}px;max-height:${Math.min(geometry.maxHeight, POPOVER_MAX_HEIGHT_PX)}px`;
  }

  function handlePopoverFocusOut(event: FocusEvent): void {
    const next = event.relatedTarget;
    if (!(next instanceof Node) || popover?.contains(next) || trigger?.contains(next)) return;
    close();
  }

  /** Navigate options without letting Escape close the parent surface. */
  function handleKeydown(event: KeyboardEvent): void {
    if (!(event.target instanceof Node) || (!popover?.contains(event.target) && !trigger?.contains(event.target))) return;
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      close(true);
      return;
    }
    const options = [...(popover?.querySelectorAll<HTMLButtonElement>('[role="option"]') ?? [])];
    if (event.target === searchInput && event.key === "Enter") {
      event.preventDefault();
      event.stopPropagation();
      options[0]?.click();
      return;
    }
    if (event.target === searchInput && (event.key === "Home" || event.key === "End")) return;
    if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key) || options.length === 0) return;
    event.preventDefault();
    event.stopPropagation();
    const index = options.findIndex((option) => option === document.activeElement);
    const next = event.key === "Home" ? 0 : event.key === "End" ? options.length - 1
      : event.key === "ArrowDown" ? (index + 1) % options.length
      : (index <= 0 ? options.length : index) - 1;
    options[next]?.focus({ preventScroll: true });
  }

  $effect(() => {
    if (!open) return;
    const pointer = (event: PointerEvent) => {
      if (!(event.target instanceof Node)) return;
      if (!trigger?.contains(event.target) && !popover?.contains(event.target)) close();
    };
    const scroll = (event: Event) => {
      if (event.target instanceof Node && !popover?.contains(event.target)) position();
    };
    window.addEventListener("pointerdown", pointer, true);
    window.addEventListener("keydown", handleKeydown, true);
    window.addEventListener("resize", position);
    window.addEventListener("scroll", scroll, true);
    return () => {
      window.removeEventListener("pointerdown", pointer, true);
      window.removeEventListener("keydown", handleKeydown, true);
      window.removeEventListener("resize", position);
      window.removeEventListener("scroll", scroll, true);
    };
  });
</script>

{#snippet soundOption(definition: MusicSoundscapeDefinition)}
  {@const choice = soundChoice(definition.id)}
  {@const selected = soundChoicesEqual(choice, value)}
  <button type="button" role="option" aria-selected={selected} disabled={definition.availability !== "available" && !selected} onclick={() => choose(choice)} class="menu-item">
    <span class="grid size-4 shrink-0 place-items-center"><MusicSoundscapeSoundIcon {definition} size={15} /></span>
    <span class="min-w-0 flex-1 truncate font-medium">{soundName(definition)}</span>
    {#if selected}<Check size={14} />{/if}
    {#if definition.availability !== "available"}<span class="shrink-0 text-panel-detail text-muted-foreground">{t("music.soundscape.needsRepair")}</span>{/if}
  </button>
{/snippet}

<div class={cn("min-w-0", className)}>
  <button
    bind:this={trigger}
    type="button"
    disabled={disabled || loading}
    onclick={() => { void toggle(); }}
    onkeydown={(event) => {
      if (!open && (event.key === "ArrowDown" || event.key === "ArrowUp")) {
        event.preventDefault();
        void toggle();
      }
    }}
    aria-haspopup="dialog"
    aria-expanded={open}
    aria-controls={open ? id : undefined}
    aria-label={label}
    class={cn(
      "flex h-7 w-full min-w-0 items-center gap-2 rounded-md border border-border bg-card px-2.5 text-left text-[0.8rem] font-medium text-foreground transition-colors hover:bg-accent disabled:cursor-not-allowed disabled:hover:bg-card dark:bg-transparent dark:disabled:hover:bg-transparent",
      triggerClass,
      missing && "text-warning-foreground ring-1 ring-warning/55",
    )}
  >
    {#if missing}<AlertTriangle size={14} class="shrink-0 text-warning" />
    {:else if selectedPreset}<span class="grid size-3.5 shrink-0 place-items-center text-muted-foreground"><MusicSoundscapeGroupIcon icon={selectedPreset.icon} size={14} /></span>
    {:else if selectedSound}<span class="grid size-3.5 shrink-0 place-items-center text-muted-foreground"><MusicSoundscapeSoundIcon definition={selectedSound} size={14} /></span>
    {:else if value.kind === "silence"}<VolumeX size={14} strokeWidth={1.5} class="shrink-0 text-muted-foreground" />
    {:else}<Minus size={14} strokeWidth={1.5} class="shrink-0 text-muted-foreground" />{/if}
    <span class="min-w-0 flex-1 truncate">
      {#if loading}<Loading layout="inline" label={t("music.soundscape.title")} />
      {:else if missing}{t("music.soundscape.choice.missing")}
      {:else if selectedPreset}{selectedPreset.name}
      {:else if selectedSound}{soundName(selectedSound)}
      {:else if value.kind === "silence"}{t("music.soundscape.choice.silence")}
      {:else}{t("music.soundscape.choice.keep")}{/if}
    </span>
    <ChevronDown size={13} strokeWidth={2} class={cn("shrink-0 text-muted-foreground transition-transform", open && "rotate-180")} />
  </button>
</div>

{#if open}
  <div
    use:portal={trigger?.closest<HTMLElement>("[data-floating-root]") ?? "body"}
    bind:this={popover}
    role="dialog"
    {id}
    tabindex="-1"
    data-app-floating-surface
    aria-label={label}
    onfocusout={handlePopoverFocusOut}
    style={popoverStyle()}
    class="surface-floating fixed z-80 flex min-h-0 flex-col overflow-hidden"
  >
    <div class="shrink-0 px-1.5 pt-1.5">
      <label class="field flex items-center gap-1.5">
        <Search size={13} strokeWidth={1.5} class="shrink-0 text-muted-foreground" />
        <input bind:this={searchInput} bind:value={search} type="search" aria-label={t("music.soundscape.choice.search")} class="field-bare" placeholder={t("music.soundscape.choice.search")} />
      </label>
    </div>
    <div use:scrollEdgeFadeAction class="surface-floating-body min-h-0 flex-1 overflow-y-auto overscroll-contain" data-music-scrollable="true">
      <div role="listbox" aria-label={label}>
        {#if !query}
          <button type="button" role="option" aria-selected={value.kind === "keep"} onclick={() => choose({ kind: "keep" })} class="menu-item">
            <Minus size={15} strokeWidth={1.75} />
            <span class="min-w-0 flex-1 truncate font-medium">{t("music.soundscape.choice.keep")}</span>
            {#if value.kind === "keep"}<Check size={14} />{/if}
          </button>
          <button type="button" role="option" aria-selected={value.kind === "silence"} onclick={() => choose({ kind: "silence" })} class="menu-item">
            <VolumeX size={15} strokeWidth={1.75} />
            <span class="min-w-0 flex-1 truncate font-medium">{t("music.soundscape.choice.silence")}</span>
            {#if value.kind === "silence"}<Check size={14} />{/if}
          </button>
        {/if}
        {#if presets.length > 0}
          <p class="px-3 pt-2 pb-1 text-panel-detail text-muted-foreground">{t("music.soundscape.choice.presets")}</p>
          {#each presets as preset (preset.id)}
            {@const choice = presetChoice(preset.id)}
            {@const selected = soundChoicesEqual(choice, value)}
            {@const playable = soundscape.presetIsPlayable(preset)}
            <button type="button" role="option" aria-selected={selected} disabled={!playable && !selected} onclick={() => choose(choice)} class="menu-item">
              <span class="grid size-4 shrink-0 place-items-center"><MusicSoundscapeGroupIcon icon={preset.icon} size={15} /></span>
              <span class="min-w-0 flex-1 truncate font-medium">{preset.name}</span>
              {#if selected}<Check size={14} />{/if}
              <span class="shrink-0 text-panel-detail text-muted-foreground">{playable ? t("music.soundscape.choice.presetCount", preset.soundscapeIds.length) : t("music.soundscape.needsRepair")}</span>
            </button>
          {/each}
        {/if}
        {#if generated.length > 0}
          <p class="px-3 pt-2 pb-1 text-panel-detail text-muted-foreground">{t("music.soundscape.generated")}</p>
          {#each generated as definition (definition.id)}{@render soundOption(definition)}{/each}
        {/if}
        {#each localSections as section (section.key)}
          <p class="flex items-center gap-1.5 px-3 pt-2 pb-1 text-panel-detail text-muted-foreground">
            {#if section.group}<MusicSoundscapeGroupIcon icon={section.group.icon} size={12} />{section.group.name}
            {:else if localSections.length > 1}{t("music.soundscape.ungrouped")}
            {:else}{t("music.soundscape.localLoops")}{/if}
          </p>
          {#each section.sounds as definition (definition.id)}{@render soundOption(definition)}{/each}
        {/each}
        {#if query && !hasMatches}
          <p class="px-3 py-6 text-center text-muted-foreground">{t("music.soundscape.choice.noMatches")}</p>
        {/if}
      </div>
    </div>
  </div>
{/if}
