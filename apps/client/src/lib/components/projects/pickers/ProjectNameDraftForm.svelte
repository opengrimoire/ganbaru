<script lang="ts">
  import X from "@lucide/svelte/icons/x";
  import type IconPicker from "$lib/components/icon-picker/IconPicker.svelte";
  import ProjectIcon from "$lib/components/projects/ProjectIcon.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { cn } from "$lib/utils";

  /**
   * A name field with its icon button, used to create a project or a group. Enter submits; there is no save button.
   * Icons keep the automatic color here; icon color is a project settings choice.
   * The icon picker loads on first use and stays inside the hosting floating surface, so choosing an icon keeps the
   * surrounding picker open.
   */
  let {
    name = $bindable(),
    icon = $bindable(),
    placeholder,
    mobileLayout = false,
    iconStrokeWidth,
    onSubmit,
    onCancel,
  }: {
    name: string;
    /** A project icon value, such as `lucide:folder` or `emoji:...`. */
    icon: string;
    placeholder: string;
    mobileLayout?: boolean;
    iconStrokeWidth?: number;
    onSubmit: () => void;
    /** Shows a cancel button when given. */
    onCancel?: () => void;
  } = $props();

  const { t } = getLocalization();
  let Picker = $state<typeof IconPicker | null>(null);
  let loading = $state(false);
  let loadError = $state<string | null>(null);

  /** Load the picker on first use; it then opens itself through `initiallyOpen`. */
  async function loadPicker(): Promise<void> {
    if (Picker || loading) return;
    loading = true;
    loadError = null;
    try {
      Picker = (await import("$lib/components/icon-picker/IconPicker.svelte")).default;
    } catch (caught: unknown) {
      loadError = caught instanceof Error ? caught.message : String(caught);
    } finally {
      loading = false;
    }
  }

  function focusOnMount(node: HTMLInputElement): void {
    node.focus({ preventScroll: true });
  }
</script>

{#snippet iconButton(open: boolean, toggle: () => void, panelId: string)}
  <button
    type="button"
    class={cn(
      "flex shrink-0 items-center justify-center border border-border text-popover-foreground hover:bg-accent",
      mobileLayout ? "size-12 rounded-xl" : "size-(--panel-row-height) rounded-floating-item",
    )}
    aria-label={t("projects.settings.icon")}
    aria-haspopup="dialog"
    aria-expanded={open}
    aria-controls={open ? panelId : undefined}
    aria-busy={loading || undefined}
    title={loadError ?? undefined}
    onclick={toggle}
  >
    <ProjectIcon name={icon} size={mobileLayout ? 18 : 14} strokeWidth={iconStrokeWidth} />
  </button>
{/snippet}

<form class={cn("flex gap-1", mobileLayout && "gap-2")} onsubmit={(event) => { event.preventDefault(); onSubmit(); }}>
  {#if Picker}
    <Picker value={icon} onChange={(next) => { icon = next; }} ariaLabel={t("projects.settings.icon")} showRemove={false} showIconColor={false} nested panelAlign="start" initiallyOpen>
      {#snippet trigger({ open, toggle, panelId })}{@render iconButton(open, toggle, panelId)}{/snippet}
    </Picker>
  {:else}
    {@render iconButton(false, () => void loadPicker(), "")}
  {/if}
  <input
    bind:value={name}
    {@attach focusOnMount}
    {placeholder}
    enterkeyhint="done"
    class={cn(
      "field min-w-0 flex-1 text-popover-foreground",
      mobileLayout && "min-h-12 rounded-xl px-3 text-base",
    )}
  />
  {#if onCancel}
    <button
      type="button"
      class="flex min-h-12 min-w-12 items-center justify-center rounded-xl active:bg-accent"
      aria-label={t("common.cancel")}
      onclick={onCancel}
    >
      <X size={18} strokeWidth={iconStrokeWidth} aria-hidden="true" />
    </button>
  {/if}
</form>
