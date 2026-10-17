<script lang="ts">
  import type { Snippet } from "svelte";
  import { containMusicDialogFocus } from "$lib/music/builder/dialog-focus";
  import { portal } from "$lib/utils/portal";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";

  type DialogSize = "small" | "medium" | "large";

  let {
    title,
    titleId,
    description,
    size = "medium",
    role = "dialog",
    dismissDisabled = false,
    enterAction,
    enterDisabled = false,
    onDismiss,
    leading,
    children,
    footer,
  }: {
    title: string;
    titleId: string;
    description?: string;
    size?: DialogSize;
    role?: "dialog" | "alertdialog";
    dismissDisabled?: boolean;
    enterAction?: () => void;
    enterDisabled?: boolean;
    onDismiss: () => void;
    leading?: Snippet;
    children?: Snippet;
    footer?: Snippet;
  } = $props();

  const widthClass = $derived(
    size === "small" ? "max-w-md" : size === "large" ? "max-w-2xl" : "max-w-xl",
  );

  function dismiss(): void {
    if (!dismissDisabled) onDismiss();
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  use:portal
  class="fixed z-90 flex items-center justify-center"
  style="left: var(--visual-viewport-offset-left); top: var(--visual-viewport-offset-top); width: var(--visual-viewport-width); height: var(--visual-viewport-height); padding: calc(var(--safe-area-top) + 1rem) calc(var(--safe-area-right) + 1rem) calc(var(--safe-area-bottom) + 1rem) calc(var(--safe-area-left) + 1rem);"
  onclick={(event) => { event.stopPropagation(); dismiss(); }}
>
  <div class="surface-backdrop absolute inset-0"></div>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    use:containMusicDialogFocus={{
      onEscape: dismiss,
      escapeDisabled: dismissDisabled,
      onEnter: enterAction,
      enterDisabled,
    }}
    class={`relative z-10 flex max-h-full w-full ${widthClass} flex-col overflow-hidden surface-dialog outline-none`}
    style="--foreground: var(--card-foreground);"
    {role}
    aria-modal="true"
    aria-labelledby={titleId}
    tabindex="-1"
    onclick={(event) => event.stopPropagation()}
  >
    <header class="flex shrink-0 items-start gap-3 px-5 pb-3 pt-5 sm:px-8 sm:pt-6">
      {#if leading}<div class="shrink-0">{@render leading()}</div>{/if}
      <div class="min-w-0">
        <h2 id={titleId} class="text-base font-semibold text-foreground">{title}</h2>
        {#if description}<p class="mt-1 text-sm leading-relaxed text-muted-foreground">{description}</p>{/if}
      </div>
    </header>
    {#if children}
      <div use:scrollEdgeFadeAction class="min-h-0 flex-1 overflow-y-auto px-5 pb-5 sm:px-8" data-music-scrollable="true">
        {@render children()}
      </div>
    {/if}
    {#if footer}
      <footer class="flex shrink-0 flex-wrap items-center justify-start gap-2 border-t border-border px-5 py-4 sm:px-8">
        {@render footer()}
      </footer>
    {/if}
  </div>
</div>
