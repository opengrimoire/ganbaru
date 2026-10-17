<script lang="ts">
  import { onMount } from "svelte";
  import type { StartupMemorySnapshot } from "$lib/diagnostics/memory-report";
  import { getSettingsLauncher } from "$lib/stores/settings-launcher.svelte";
  import { getThemeEditor } from "$lib/stores/theme-editor.svelte";
  import SettingsModal from "$lib/components/settings/SettingsModal.svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import MusicPanel from "$lib/components/music/MusicPanel.svelte";
  import QuickNotesPanel from "$lib/components/quick-notes/QuickNotesPanel.svelte";
  import { preloadQuickNotesInitialSnapshot } from "$lib/quick-notes/initial-snapshot";
  import { startMusicFirstUsePreload } from "$lib/music/first-use-preload";
  import { preloadDataSection } from "$lib/components/settings/section-catalog";

  type PerformancePopoverComponent = typeof import("$lib/components/diagnostics/PerformancePopover.svelte").default;
  type FloatingThemeEditorComponent = typeof import("$lib/components/themes/editor/FloatingThemeEditor.svelte").default;
  type ThemeQuickSwitcherComponent = typeof import("$lib/components/themes/ThemeQuickSwitcher.svelte").default;

  let {
    showPerformance = $bindable(),
    performancePinned = $bindable(),
    showThemeQuickSwitcher = $bindable(),
    showQuickNotes = $bindable(),
    showMusic = $bindable(),
    shellStartupMs,
    startupMemorySnapshot,
    ensureBenchmarkOverlay,
  }: {
    showPerformance: boolean;
    performancePinned: boolean;
    showThemeQuickSwitcher: boolean;
    showQuickNotes: boolean;
    showMusic: boolean;
    shellStartupMs: number | null;
    startupMemorySnapshot: StartupMemorySnapshot;
    ensureBenchmarkOverlay: () => Promise<void>;
  } = $props();

  const settingsLauncher = getSettingsLauncher();
  const themeEditor = getThemeEditor();

  let PerformancePopover = $state<PerformancePopoverComponent | null>(null);
  let FloatingThemeEditor = $state<FloatingThemeEditorComponent | null>(null);
  let ThemeQuickSwitcher = $state<ThemeQuickSwitcherComponent | null>(null);
  let performanceLoad: Promise<void> | null = null;
  let editorLoad: Promise<void> | null = null;
  let switcherLoad: Promise<void> | null = null;
  let musicMounted = $state(false);

  $effect(() => {
    if (showMusic) musicMounted = true;
  });

  function loadPerformance(): Promise<void> {
    if (PerformancePopover) return Promise.resolve();
    performanceLoad ??= import("$lib/components/diagnostics/PerformancePopover.svelte")
      .then((module) => { PerformancePopover = module.default; })
      .finally(() => { performanceLoad = null; });
    return performanceLoad;
  }

  function loadEditor(): Promise<void> {
    if (FloatingThemeEditor) return Promise.resolve();
    editorLoad ??= import("$lib/components/themes/editor/FloatingThemeEditor.svelte")
      .then((module) => { FloatingThemeEditor = module.default; })
      .finally(() => { editorLoad = null; });
    return editorLoad;
  }

  function loadSwitcher(): Promise<void> {
    if (ThemeQuickSwitcher) return Promise.resolve();
    switcherLoad ??= import("$lib/components/themes/ThemeQuickSwitcher.svelte")
      .then((module) => { ThemeQuickSwitcher = module.default; })
      .finally(() => { switcherLoad = null; });
    return switcherLoad;
  }

  $effect(() => {
    if (showPerformance) void loadPerformance();
    if (themeEditor.editingId) void loadEditor();
    if (showThemeQuickSwitcher) void loadSwitcher();
  });

  onMount(() => {
    const stopMusicPreload = startMusicFirstUsePreload();
    void preloadDataSection().catch((error: unknown) => {
      console.warn("Data settings preload failed", error);
    });
    void preloadQuickNotesInitialSnapshot().catch((error: unknown) => {
      console.warn("Quick notes preload failed", error);
    });
    return stopMusicPreload;
  });
</script>

{#if showPerformance}
  {#if !performancePinned}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="fixed inset-0 z-40"
      onclick={() => { showPerformance = false; }}
      onkeydown={(event) => { if (event.key === "Escape") showPerformance = false; }}
    ></div>
  {/if}
  {#if PerformancePopover}
    {@const Popover = PerformancePopover}
    <Popover
      {shellStartupMs}
      {startupMemorySnapshot}
      pinned={performancePinned}
      onPinnedChange={(nextPinned: boolean) => { performancePinned = nextPinned; }}
      {ensureBenchmarkOverlay}
    />
  {:else}
    <div
      class="surface-floating fixed z-50 overflow-hidden"
      style="top: calc(var(--titlebar-h) + 4px); right: 8px; width: min(18rem, calc(100vw - 16px)); max-height: calc(100dvh - var(--titlebar-h) - 12px);"
    >
      <Loading layout="fill" />
    </div>
  {/if}
{/if}

{#if showQuickNotes}
  <QuickNotesPanel onClose={() => { showQuickNotes = false; }} />
{/if}

{#if musicMounted}
  <MusicPanel visible={showMusic} onClose={() => { showMusic = false; }} />
{/if}

{#if showThemeQuickSwitcher && ThemeQuickSwitcher}
  {@const Switcher = ThemeQuickSwitcher}
  <Switcher onClose={() => { showThemeQuickSwitcher = false; }} />
{/if}

{#if settingsLauncher.isOpen}
  <SettingsModal
    onClose={() => settingsLauncher.close()}
    initialSection={settingsLauncher.targetSection}
    initialDistractionsTab={settingsLauncher.targetDistractionsTab}
    initialContactsTab={settingsLauncher.targetContactsTab}
    initialChatSubsection={settingsLauncher.targetChatSubsection}
    initialChatTeammateId={settingsLauncher.targetChatTeammateId}
    initialChatChannelId={settingsLauncher.targetChatChannelId}
    initialChatCreateTeammate={settingsLauncher.targetChatCreateTeammate}
  />
{/if}

{#if themeEditor.editingId && FloatingThemeEditor}
  {@const Editor = FloatingThemeEditor}
  <Editor
    onBackToList={() => {
      settingsLauncher.open("appearance");
    }}
  />
{/if}
