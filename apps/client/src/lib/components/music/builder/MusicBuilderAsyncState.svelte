<script lang="ts">
  import Disc3 from "@lucide/svelte/icons/disc-3";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";

  let {
    kind,
    title = "",
    description = "",
    onRetry,
    actionLabel = null,
    onAction = () => undefined,
  }: {
    kind: "loading" | "error" | "empty";
    title?: string;
    description?: string;
    onRetry?: () => unknown;
    actionLabel?: string | null;
    onAction?: () => void;
  } = $props();
  const { t } = getLocalization();
</script>

{#if kind === "loading"}
  <div class="p-3" aria-label={t("music.builder.loading")} aria-busy="true">
    <div class="space-y-2">
      {#each Array(7) as _, index}
        <div class="flex h-15 items-center gap-3 rounded-xl border border-border/35 bg-card/30 px-3" aria-hidden="true">
          <div class="builder-skeleton h-9 w-9 shrink-0 rounded-lg"></div>
          <div class="min-w-0 flex-1 space-y-2">
            <div class="builder-skeleton h-2.5 rounded-full" style={`width: ${54 + (index % 3) * 13}%`}></div>
            <div class="builder-skeleton h-2 rounded-full" style={`width: ${31 + (index % 2) * 17}%`}></div>
          </div>
        </div>
      {/each}
    </div>
  </div>
{:else if kind === "error"}
  <Notice layout="fill" class="text-sm" message={description ? `${title} ${description}` : title} retryLabel={t("music.builder.retryLoad")} {onRetry} />
{:else}
  <div class="grid h-full min-h-40 place-items-center p-5 text-center">
    <div class="max-w-sm">
      <div class="mx-auto mb-3 grid h-11 w-11 place-items-center rounded-2xl bg-secondary/75 text-muted-foreground">
        <Disc3 size={21} strokeWidth={1.45} />
      </div>
      <h2 class="text-sm font-semibold text-foreground">{title}</h2>
      {#if description}<p class="mt-1.5 text-xs leading-relaxed text-muted-foreground">{description}</p>{/if}
      {#if actionLabel}
        <button type="button" onclick={onAction} class="mt-4 inline-flex h-8 items-center rounded-lg bg-primary px-3 text-xs font-semibold text-primary-foreground hover:bg-primary/90">
          {actionLabel}
        </button>
      {/if}
    </div>
  </div>
{/if}

<style>
  .builder-skeleton {
    background: linear-gradient(90deg, color-mix(in srgb, var(--muted) 72%, transparent), color-mix(in srgb, var(--accent) 70%, transparent), color-mix(in srgb, var(--muted) 72%, transparent));
    background-size: 220% 100%;
    animation: builder-shimmer 1.7s ease-in-out infinite;
  }
  @media (prefers-reduced-motion: reduce) { .builder-skeleton { animation: none; } }
  @keyframes builder-shimmer { from { background-position: 100% 0; } to { background-position: -100% 0; } }
</style>
