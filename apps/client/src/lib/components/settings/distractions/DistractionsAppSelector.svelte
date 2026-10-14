<script lang="ts" module>
  export interface DistractionsAppSelection {
    name: string;
    matchNames: readonly string[];
  }

  export type DistractionsAppSelectorMode = "multi" | "single";
</script>

<script lang="ts">
  import { onMount, tick } from "svelte";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Search from "@lucide/svelte/icons/search";
  import X from "@lucide/svelte/icons/x";
  import {
    listDistractionsDesktopApps,
    type DistractionsDesktopAppCandidate,
  } from "$lib/api/distractions";
  import { isProtectedDistractionsDesktopAppName } from "$lib/distractions";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import { OPENING_MAY_FOCUS_TEXT } from "$lib/mobile/keyboard";

  let {
    title,
    mode = "multi",
    existingNames,
    protectAppSelf = false,
    onAdd,
    onRemove,
    onSelect,
    onCancel,
  }: {
    title: string;
    mode?: DistractionsAppSelectorMode;
    existingNames: readonly string[];
    protectAppSelf?: boolean;
    onAdd?: (app: DistractionsAppSelection) => boolean;
    onRemove?: (name: string) => void;
    onSelect?: (app: DistractionsAppSelection) => void;
    onCancel: () => void;
  } = $props();

  const { t } = getLocalization();

  /** Desktop app discovery is usually quick, so the pending list stays blank this long before showing rows. */
  const APP_LIST_LOADING_DELAY_MS = 1000;

  let searchInputEl: HTMLInputElement | undefined = $state();
  let query = $state("");
  let apps = $state<DistractionsDesktopAppCandidate[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let pendingAddApp = $state<DistractionsAppSelection | null>(null);
  let pendingRemoveName = $state<string | null>(null);
  const confirmationOpen = $derived(Boolean(pendingAddApp || pendingRemoveName));

  const existingKeys = $derived(new Set(existingNames.map((name) => name.toLowerCase())));
  const existingNameByKey = $derived(
    new Map(existingNames.map((name) => [name.toLowerCase(), name])),
  );
  const filteredApps = $derived.by(() => {
    const normalizedQuery = query.trim().toLowerCase();
    const selectableApps = protectAppSelf
      ? apps.filter((app) => !isProtectedAppCandidate(app))
      : apps;
    const matches = normalizedQuery
      ? selectableApps.filter((app) => app.name.toLowerCase().includes(normalizedQuery))
      : selectableApps;
    return matches.slice(0, 80);
  });

  function isProtectedAppCandidate(app: DistractionsDesktopAppCandidate): boolean {
    return (
      isProtectedDistractionsDesktopAppName(app.name)
      || app.processNames.some((name) => isProtectedDistractionsDesktopAppName(name))
    );
  }

  async function loadApps(): Promise<void> {
    loading = true;
    error = null;
    try {
      apps = await listDistractionsDesktopApps();
    } catch (err) {
      console.warn("Failed to list desktop apps:", err);
      error = t("settings.distractions.appSelector.loadError");
    } finally {
      loading = false;
    }
  }

  function selectionFor(name: string, matchNames: readonly string[] = [name]): DistractionsAppSelection | null {
    const trimmed = name.trim();
    if (!trimmed || existingKeys.has(trimmed.toLowerCase())) return null;
    if (protectAppSelf && isProtectedDistractionsDesktopAppName(trimmed)) return null;
    return { name: trimmed, matchNames };
  }

  function requestAdd(name: string, matchNames: readonly string[] = [name]): void {
    const selection = selectionFor(name, matchNames);
    if (!selection) return;
    if (mode === "single") {
      onSelect?.(selection);
      return;
    }
    pendingAddApp = selection;
  }

  function confirmAdd(): void {
    if (!pendingAddApp) return;
    onAdd?.(pendingAddApp);
    pendingAddApp = null;
  }

  function cancelAdd(): void {
    pendingAddApp = null;
  }

  function requestRemove(name: string): void {
    const existingName = existingNameByKey.get(name.toLowerCase());
    if (!existingName) return;
    if (mode === "single") return;
    pendingRemoveName = existingName;
  }

  function confirmRemove(): void {
    if (!pendingRemoveName) return;
    onRemove?.(pendingRemoveName);
    pendingRemoveName = null;
  }

  function cancelRemove(): void {
    pendingRemoveName = null;
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (pendingAddApp || pendingRemoveName) return;
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      onCancel();
      return;
    }
    event.stopPropagation();
  }

  onMount(() => {
    void loadApps();
    void tick().then(() => {
      if (OPENING_MAY_FOCUS_TEXT) searchInputEl?.focus();
    });
    window.addEventListener("keydown", handleKeydown, true);
    return () => window.removeEventListener("keydown", handleKeydown, true);
  });
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="fixed inset-0 z-90 flex items-center justify-center px-3 py-4"
  onclick={(event) => {
    event.stopPropagation();
    onCancel();
  }}
>
  <div class="surface-backdrop absolute inset-0"></div>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="surface-dialog relative z-10 flex h-[min(36rem,calc(100vh-2rem))] w-full max-w-lg flex-col outline-none"
    role="dialog"
    aria-modal="true"
    tabindex="-1"
    onclick={(event) => event.stopPropagation()}
    onkeydown={handleKeydown}
  >
    <div class="flex min-w-0 items-start justify-between gap-3 px-4 pt-3">
      <div class="min-w-0">
        <h2 class="text-[1rem] font-semibold text-foreground">{title}</h2>
        <p class="mt-0.5 text-[0.8rem] text-muted-foreground">
          {t("settings.distractions.appSelector.systemHidden")}
        </p>
      </div>
      <button
        type="button"
        onclick={onCancel}
        aria-label={t("settings.distractions.appSelector.close")}
        data-app-tooltip-disabled="true"
        class="flex h-7 w-7 shrink-0 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
      >
        <X size={15} strokeWidth={2} />
      </button>
    </div>

    <div class="flex min-h-0 min-w-0 flex-1 flex-col gap-3 overflow-hidden px-4 pb-4 pt-3">
      <div class="field flex min-w-0 items-center gap-2 px-2.5 py-1.5">
        <Search size={14} strokeWidth={2.25} class="shrink-0 text-muted-foreground" />
        <input
          bind:this={searchInputEl}
          bind:value={query}
          type="text"
          spellcheck="false"
          placeholder={t("settings.distractions.appSelector.search")}
          class="field-bare h-7 text-[0.866667rem] text-foreground"
        />
        <button
          type="button"
          onclick={loadApps}
          disabled={loading}
          aria-label={t("settings.distractions.appSelector.refresh")}
          class="flex h-7 w-7 shrink-0 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground disabled:cursor-not-allowed disabled:opacity-40"
        >
          {#if loading}
            <LoaderCircle size={14} strokeWidth={2.25} class="animate-spin" />
          {:else}
            <RefreshCw size={14} strokeWidth={2.25} />
          {/if}
        </button>
      </div>

      <div class="min-h-0 flex-1 overflow-hidden rounded-floating-item border border-border">
        <div use:scrollEdgeFadeAction class="h-full overflow-y-auto">
        {#if loading}
          <Loading layout="rows" rows={3} delay={APP_LIST_LOADING_DELAY_MS} label={t("settings.distractions.appSelector.loading")} />
        {:else if error}
          <div class="flex h-full min-h-36 items-center justify-center px-4 text-center text-[0.866667rem] text-muted-foreground">
            {error}
          </div>
        {:else if filteredApps.length === 0}
          <div class="flex h-full min-h-36 items-center justify-center px-4 text-center text-[0.866667rem] text-muted-foreground">
            {t("settings.distractions.appSelector.empty")}
          </div>
        {:else}
          <div class="flex flex-col">
            {#each filteredApps as app (app.name.toLowerCase())}
              {@const alreadyAdded = mode === "single"
                ? existingKeys.has(app.name.toLowerCase()) || app.processNames.some((name) => existingKeys.has(name.toLowerCase()))
                : existingKeys.has(app.name.toLowerCase())}
              <button
                type="button"
                onclick={() => alreadyAdded ? requestRemove(app.name) : requestAdd(app.name, app.processNames)}
                class={[
                  "flex min-w-0 items-center justify-between gap-3 border-b border-border/70 px-3 py-2 text-left last:border-b-0",
                  !confirmationOpen && (!alreadyAdded || mode === "multi") && "hover:bg-accent",
                  alreadyAdded && mode === "single" && "cursor-not-allowed opacity-60",
                ]}
                disabled={alreadyAdded && mode === "single"}
              >
                <span class="min-w-0 truncate text-[0.866667rem] text-foreground">{app.name}</span>
                <span class="shrink-0 text-[0.8rem] text-muted-foreground">
                  {#if alreadyAdded}
                    {mode === "single"
                      ? t("settings.distractions.appSelector.used")
                      : t("settings.distractions.appSelector.remove")}
                  {:else}
                    {mode === "single"
                      ? t("settings.distractions.appSelector.choose")
                      : t("settings.distractions.appSelector.add")}
                  {/if}
                </span>
              </button>
            {/each}
          </div>
        {/if}
        </div>
      </div>
    </div>
  </div>
</div>

{#if pendingAddApp}
  <ConfirmDialog
    title={t("settings.distractions.appSelector.blockTitle", pendingAddApp.name)}
    message={t("settings.distractions.appSelector.blockMessage")}
    confirmLabel={t("settings.distractions.appSelector.blockAction")}
    cancelLabel={t("settings.distractions.shared.cancelAction")}
    onConfirm={confirmAdd}
    onCancel={cancelAdd}
  />
{/if}

{#if pendingRemoveName}
  <ConfirmDialog
    title={t("settings.distractions.desktop.removeAppTitle", pendingRemoveName)}
    message={t("settings.distractions.desktop.removeMessage")}
    confirmLabel={t("settings.distractions.shared.removeAction")}
    cancelLabel={t("settings.distractions.shared.cancelAction")}
    onConfirm={confirmRemove}
    onCancel={cancelRemove}
  />
{/if}
