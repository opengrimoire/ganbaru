<script lang="ts">
  import Info from "@lucide/svelte/icons/info";
  import Moon from "@lucide/svelte/icons/moon";
  import Sun from "@lucide/svelte/icons/sun";
  import {
    getThemeConflict,
    ThemeConflictError,
    type ThemeConflict,
    type ThemeConflictChoice,
    type ThemeConflictVersion,
  } from "$lib/api/themes";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { formatSyncTime, syncDeviceName } from "$lib/sync/status";

  let {
    themeId,
    displayName,
    iconLabel,
    onResolve,
  }: {
    themeId: string;
    displayName: string;
    iconLabel: "light" | "dark";
    /** Resolves the conflict on every linked device; rejects with a {@link ThemeConflictError} when it is gone. */
    onResolve: (choice: ThemeConflictChoice) => Promise<void>;
  } = $props();

  /** Source tokens shown as swatches beside the palette, in display order. */
  const SWATCH_SOURCES = ["canvas", "ink", "primary"] as const;
  const localization = getLocalization();
  const { t } = localization;
  const BaseIcon = $derived(iconLabel === "dark" ? Moon : Sun);
  let conflict = $state<ThemeConflict | null>(null);
  let loading = $state(true);
  let busy = $state(false);
  let message = $state("");
  let messageKind = $state<"resolved" | "loadFailed" | "resolveFailed">("resolveFailed");
  let loadGeneration = 0;

  function swatches(version: ThemeConflictVersion): string[] {
    return SWATCH_SOURCES.flatMap((key) => {
      const token = version.tokens.find((entry) => entry.kind === "source" && entry.key === key);
      return token ? [token.value] : [];
    });
  }

  function source(version: ThemeConflictVersion): string {
    return t(
      "theme.conflict.versionFrom",
      syncDeviceName(version.device, t("theme.conflict.thisDevice"), t("theme.conflict.otherDevice")),
      formatSyncTime(t, localization.locale, version.editedAtMs, Date.now()),
    );
  }

  async function load(id: string): Promise<void> {
    const expected = ++loadGeneration;
    loading = true;
    try {
      const next = await getThemeConflict(id);
      if (expected !== loadGeneration) return;
      conflict = next;
      message = "";
    } catch (cause: unknown) {
      if (expected !== loadGeneration) return;
      if (cause instanceof ThemeConflictError && cause.code === "resolved") {
        conflict = { id, versions: [] };
        return;
      }
      console.warn("theme conflict could not be loaded", cause);
      message = t("theme.conflict.loadFailed");
      messageKind = "loadFailed";
    } finally {
      if (expected === loadGeneration) loading = false;
    }
  }

  async function resolve(choice: ThemeConflictChoice): Promise<void> {
    if (busy) return;
    busy = true;
    message = "";
    try {
      await onResolve(choice);
    } catch (cause: unknown) {
      if (cause instanceof ThemeConflictError && cause.code === "resolved") {
        message = t("theme.conflict.resolved");
        messageKind = "resolved";
      } else {
        console.warn("theme conflict could not be resolved", cause);
        message = t("theme.conflict.failed");
        messageKind = "resolveFailed";
      }
    } finally {
      busy = false;
    }
  }

  $effect(() => {
    void load(themeId);
  });
</script>

<section
  class="flex max-h-80 flex-col gap-2 overflow-y-auto rounded-lg bg-warning/15 px-3 py-2 text-[0.8rem]"
  aria-label={t("theme.conflict.title")}
>
  <div class="font-medium">{t("theme.conflict.title")}</div>
  <div class="text-muted-foreground">{t("theme.conflict.hint")}</div>
  {#if loading && !conflict}
    <Loading layout="rows" rows={2} label={t("theme.conflict.loading")} />
  {/if}
  {#each conflict?.versions ?? [] as version (version.version)}
    <div class="flex flex-col gap-1.5 rounded-md bg-black/5 px-2 py-1.5 dark:bg-white/5">
      <div class="flex min-w-0 items-center gap-2">
        <BaseIcon size={13} strokeWidth={1.75} class="shrink-0 text-muted-foreground" />
        <span class="truncate font-medium">{displayName}</span>
        <span class="flex shrink-0 gap-0.5" aria-hidden="true">
          {#each swatches(version) as color, index (index)}
            <span class="size-3.5 rounded-sm border border-border" style:background-color={color}></span>
          {/each}
        </span>
      </div>
      <div class="flex h-2.5 w-full overflow-hidden rounded-sm" role="img" aria-label={t("theme.conflict.palette")}>
        {#each version.palette as color, slot (slot)}
          <span class="min-w-0 flex-1" style:background-color={color}></span>
        {/each}
      </div>
      <div class="flex flex-wrap items-baseline gap-x-2 text-muted-foreground">
        <span>{source(version)}</span>
        {#if version.displayed}<span>{t("theme.conflict.shown")}</span>{/if}
      </div>
      <div class="flex flex-wrap items-center gap-1">
        {#if version.displayed}
          <button class="rounded-md px-1.5 py-0.5 font-medium underline hover:bg-black/10 disabled:opacity-40 dark:hover:bg-white/10" type="button" disabled={busy} onclick={() => void resolve({ kind: "displayed" })}>{t("theme.conflict.keepThis")}</button>
        {:else}
          <button class="rounded-md px-1.5 py-0.5 font-medium underline hover:bg-black/10 disabled:opacity-40 dark:hover:bg-white/10" type="button" disabled={busy} onclick={() => void resolve({ kind: "version", version: version.version })}>{t("theme.conflict.useOther")}</button>
        {/if}
      </div>
    </div>
  {/each}
  {#if message}
    <Notice
      {message}
      icon={messageKind === "resolved" ? Info : undefined}
      role={messageKind === "resolved" ? "status" : "alert"}
      copy={false}
      onRetry={messageKind === "loadFailed" ? () => load(themeId) : undefined}
    />
  {/if}
</section>
