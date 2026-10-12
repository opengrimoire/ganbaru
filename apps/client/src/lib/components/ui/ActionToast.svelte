<script lang="ts">
  import X from "@lucide/svelte/icons/x";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { cn } from "$lib/utils";

  const { t } = getLocalization();

  let {
    message,
    actionLabel,
    dismissLabel,
    variant = "default",
    stacked = false,
    controlsVisible = true,
    reserveActionLabel,
    onAction,
    onDismiss,
  }: {
    message: string;
    actionLabel?: string;
    dismissLabel?: string;
    variant?: "default" | "success" | "error";
    stacked?: boolean;
    controlsVisible?: boolean;
    reserveActionLabel?: string;
    onAction?: () => void | Promise<void>;
    onDismiss: () => void;
  } = $props();

  const toastVariantClass = $derived(
    variant === "success"
      ? "border-action-confirm/45 bg-action-confirm text-action-confirm-foreground"
      : variant === "error"
        ? "border-destructive/45 bg-destructive text-destructive-foreground"
      : "border-border bg-popover text-popover-foreground",
  );

  const actionButtonClass = $derived(
    variant === "success"
      ? "rounded-sm px-2 py-1 text-[0.8rem] font-medium text-action-confirm-foreground underline decoration-action-confirm-foreground/50 underline-offset-2 transition-colors hover:bg-white/15 hover:no-underline focus:outline-none focus:ring-1 focus:ring-action-confirm-foreground/70"
      : variant === "error"
        ? "rounded-sm px-2 py-1 text-[0.8rem] font-medium text-destructive-foreground underline decoration-destructive-foreground/50 underline-offset-2 transition-colors hover:bg-white/15 hover:no-underline focus:outline-none focus:ring-1 focus:ring-destructive-foreground/70"
      : "rounded-sm px-2 py-1 text-[0.8rem] font-medium text-popover-foreground underline decoration-popover-foreground/40 underline-offset-2 transition-colors hover:bg-accent hover:text-accent-foreground hover:no-underline focus:outline-none focus:ring-1 focus:ring-ring",
  );

  const dismissButtonClass = $derived(
    variant === "success"
      ? "flex h-7 w-7 items-center justify-center rounded-sm text-action-confirm-foreground/75 transition-colors hover:bg-white/15 hover:text-action-confirm-foreground focus:outline-none focus:ring-1 focus:ring-action-confirm-foreground/70"
      : variant === "error"
        ? "flex h-7 w-7 items-center justify-center rounded-sm text-destructive-foreground/75 transition-colors hover:bg-white/15 hover:text-destructive-foreground focus:outline-none focus:ring-1 focus:ring-destructive-foreground/70"
      : "flex h-7 w-7 items-center justify-center rounded-sm text-muted-foreground transition-colors hover:bg-accent hover:text-foreground focus:outline-none focus:ring-1 focus:ring-ring",
  );

  const reservedActionLabel = $derived(reserveActionLabel ?? actionLabel);
  const viewportBottom = $derived(
    stacked
      ? "calc(var(--keyboard-inset) + var(--safe-area-bottom) + 4rem)"
      : "calc(var(--keyboard-inset) + var(--safe-area-bottom) + 1rem)",
  );
</script>

<div
  role="status"
  aria-live="polite"
  style:bottom={viewportBottom}
  class={cn(
    "fixed left-1/2 z-70 flex w-[min(22rem,calc(100vw-2rem))] -translate-x-1/2 items-center gap-3 rounded-floating border px-3 py-2 text-[0.866667rem] shadow-floating",
    toastVariantClass,
  )}
>
  <span class="min-w-0 flex-1 truncate">{message}</span>
  {#if controlsVisible && actionLabel && onAction}
    <button
      type="button"
      class={actionButtonClass}
      onclick={() => {
        void onAction();
      }}
    >
      {actionLabel}
    </button>
  {:else if reservedActionLabel}
    <span class={cn(actionButtonClass, "invisible pointer-events-none select-none")} aria-hidden="true">
      {reservedActionLabel}
    </span>
  {/if}
  {#if controlsVisible}
    <button
      type="button"
      class={dismissButtonClass}
      onclick={onDismiss}
    >
      <span class="sr-only">{dismissLabel ?? t("common.dismissNotification")}</span>
      <X size={14} strokeWidth={2} aria-hidden="true" />
    </button>
  {:else}
    <span class={cn(dismissButtonClass, "invisible pointer-events-none select-none")} aria-hidden="true">
      <X size={14} strokeWidth={2} aria-hidden="true" />
    </span>
  {/if}
</div>
