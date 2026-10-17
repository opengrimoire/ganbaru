<script lang="ts">
  import type IconPicker from "$lib/components/icon-picker/IconPicker.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { parseProjectIcon, serializeProjectIcon } from "$lib/projects/icons/values";
  import { cn } from "$lib/utils";
  import CollectionPropertyIcon from "./CollectionPropertyIcon.svelte";
  import type { CollectionPropertyKind } from "./property-icons";

  /**
   * Opens a Lucide-only icon picker for a property. Remove returns to the type icon.
   * The picker loads on first use so collection tables stay out of the first-use bundle, and it stays inside the hosting floating surface, so choosing an icon does not dismiss the menu around it.
   */
  let { icon = null, kind, editable = true, class: className = "", onChange }: {
    /** A `lucide:` picker value; null shows the type icon. */
    icon?: string | null;
    kind: CollectionPropertyKind | null;
    editable?: boolean;
    class?: string;
    /** Receives a `lucide:` value, or null to show the type icon. */
    onChange: (icon: string | null) => void;
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
      loadError = t("collections.property.iconFailed", caught instanceof Error ? caught.message : String(caught));
    } finally {
      loading = false;
    }
  }

  /** Keep Lucide choices and map removal to the type icon; other icon sources are not property icons. */
  function choose(value: string): void {
    const parsed = parseProjectIcon(value);
    if (parsed.kind === "none") onChange(null);
    else if (parsed.kind === "lucide") onChange(serializeProjectIcon(parsed));
  }
</script>

{#snippet button(open: boolean, toggle: () => void, panelId: string)}
  <button type="button" class={cn("inline-flex shrink-0 items-center justify-center rounded-md border border-border text-muted-foreground enabled:hover:bg-accent enabled:hover:text-foreground disabled:cursor-not-allowed", className)}
    aria-label={t("collections.property.icon")} aria-haspopup="dialog" aria-expanded={open} aria-controls={open ? panelId : undefined}
    aria-busy={loading || undefined} title={loadError ?? undefined}
    disabled={!editable} onclick={toggle}>
    <CollectionPropertyIcon {icon} {kind} size={16} />
  </button>
{/snippet}

{#if editable && Picker}
  <Picker value={icon ?? "none"} onChange={choose} ariaLabel={t("collections.property.icon")} showEmoji={false} showUpload={false} showRemove={icon !== null} nested panelAlign="start" initiallyOpen>
    {#snippet trigger({ open, toggle, panelId })}{@render button(open, toggle, panelId)}{/snippet}
  </Picker>
{:else}
  {@render button(false, () => void loadPicker(), "")}
{/if}
