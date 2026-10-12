<script lang="ts">
  import { tick } from "svelte";
  import Check from "@lucide/svelte/icons/check";
  import Crop from "@lucide/svelte/icons/crop";
  import ImagePlus from "@lucide/svelte/icons/image-plus";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import type { EventColor } from "$lib/calendar/types";
  import { EVENT_COLOR_OPTIONS, getEventColor } from "$lib/calendar/utils";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { BUILD_PLATFORM_PROFILE } from "$lib/platform";
  import { getTheme } from "$lib/stores/theme.svelte";
  import Switch from "$lib/components/ui/Switch.svelte";
  import { FLOATING_WIDTH } from "$lib/components/ui/floating-width";
  import { anchoredPanelStyle } from "$lib/utils/anchored-panel";
  import { portal } from "$lib/utils/portal";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";

  let {
    anchor,
    hasImage,
    useImage,
    color,
    busy = false,
    onUpload,
    onAdjust,
    onRemove,
    onUseImageChange,
    onColorChange,
    onClose,
  }: {
    anchor: HTMLElement;
    hasImage: boolean;
    useImage: boolean;
    color: EventColor;
    busy?: boolean;
    onUpload: () => void;
    onAdjust: () => void;
    onRemove: () => void;
    onUseImageChange: (useImage: boolean) => void;
    onColorChange: (color: EventColor) => void;
    onClose: () => void;
  } = $props();

  const { t } = getLocalization();
  const theme = getTheme();
  const mobileShell = BUILD_PLATFORM_PROFILE.shell === "mobile";
  /** Rows of the palette grid; the columns divide the palette evenly so no row is partial. */
  const SWATCH_ROWS = 4;
  const swatchColumns = Math.ceil(EVENT_COLOR_OPTIONS.length / SWATCH_ROWS);
  /** Touch keeps the wider panel so swatches stay large enough to tap. */
  const panelWidth = mobileShell ? FLOATING_WIDTH.md : FLOATING_WIDTH.sm;

  /** Set while turning the switch on without an image starts an upload; it shows on until the picker settles. */
  let uploadRequested = $state(false);

  function toggleUseImage(next: boolean): void {
    if (hasImage) {
      onUseImageChange(next);
      return;
    }
    if (!next || busy) return;
    uploadRequested = true;
    onUpload();
  }

  /** Anchor the panel to the avatar and close it on outside pointer or Escape. */
  function floatPanel(node: HTMLDivElement) {
    const portaled = portal(node, anchor.closest<HTMLElement>("[data-floating-root]") ?? document.body);
    const place = () => {
      if (!anchor.isConnected) return;
      node.style.cssText = anchoredPanelStyle({
        triggerRect: anchor.getBoundingClientRect(),
        viewportWidth: window.innerWidth,
        viewportHeight: window.innerHeight,
        preferredWidth: panelWidth,
        contentHeight: node.scrollHeight,
        horizontalAlign: "end",
      });
    };
    const outside = (event: Event) => {
      if (event.target instanceof Node && !node.contains(event.target) && !anchor.contains(event.target)) onClose();
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      onClose();
      anchor.focus();
    };
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(place);
    observer?.observe(node);
    observer?.observe(anchor);
    window.addEventListener("resize", place);
    window.addEventListener("scroll", place, true);
    window.addEventListener("pointerdown", outside, true);
    window.addEventListener("keydown", escape, true);
    void tick().then(() => {
      place();
      node.focus({ preventScroll: true });
    });
    return {
      destroy() {
        observer?.disconnect();
        window.removeEventListener("resize", place);
        window.removeEventListener("scroll", place, true);
        window.removeEventListener("pointerdown", outside, true);
        window.removeEventListener("keydown", escape, true);
        portaled.destroy();
      },
    };
  }
</script>

<div
  use:floatPanel
  role="dialog"
  tabindex="-1"
  aria-label={t("settings.profileIdentity.editPicture")}
  aria-busy={busy}
  data-app-floating-surface
  class="surface-floating z-80 flex flex-col overflow-hidden focus:outline-none"
>
  <div use:scrollEdgeFadeAction class="surface-floating-body min-h-0 overflow-y-auto">
    <p class="menu-label">{t("settings.profileIdentity.imageHeading")}</p>
    <div class="flex min-h-(--panel-row-height) w-full min-w-0 items-center gap-2 px-2">
      <span class="min-w-0 flex-1 truncate">{t("settings.profileIdentity.useImage")}</span>
      <Switch
        size="compact"
        checked={hasImage ? useImage : uploadRequested && busy}
        ariaLabel={t("settings.profileIdentity.useImage")}
        onChange={toggleUseImage}
      />
    </div>
    <button type="button" class="menu-item" disabled={busy} onclick={() => { uploadRequested = false; onUpload(); }}>
      <ImagePlus strokeWidth={1.75} aria-hidden="true" />
      <span class="min-w-0 flex-1 truncate">
        {t(hasImage ? "settings.profileIdentity.replacePicture" : "settings.profileIdentity.uploadPicture")}
      </span>
    </button>
    {#if hasImage}
      <button type="button" class="menu-item" disabled={busy} onclick={onAdjust}>
        <Crop strokeWidth={1.75} aria-hidden="true" />
        <span class="min-w-0 flex-1 truncate">{t("settings.profileIdentity.adjustPicture")}</span>
      </button>
      <button type="button" class="menu-item menu-item-destructive" disabled={busy} onclick={onRemove}>
        <Trash2 strokeWidth={1.75} aria-hidden="true" />
        <span class="min-w-0 flex-1 truncate">{t("settings.profileIdentity.removePicture")}</span>
      </button>
    {/if}

    <div class="menu-separator" aria-hidden="true"></div>

    <p class="menu-label">{t("settings.profileIdentity.colorHeading")}</p>
    <div
      class="grid gap-1.5 px-2 pt-1 pb-2"
      style={`grid-template-columns: repeat(${swatchColumns}, minmax(0, 1fr));`}
    >
      {#each EVENT_COLOR_OPTIONS as slot (slot)}
        {@const entry = getEventColor(slot, theme.current)}
        <button
          type="button"
          class="flex aspect-square items-center justify-center rounded-full focus:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          style={`background-color: ${entry.bg}; color: ${entry.text};`}
          aria-label={t("settings.profileIdentity.selectColor", slot + 1)}
          aria-pressed={color === slot}
          data-app-tooltip-disabled="true"
          onclick={() => onColorChange(slot)}
        >
          {#if color === slot}<Check size={12} strokeWidth={2.5} aria-hidden="true" />{/if}
        </button>
      {/each}
    </div>
  </div>
</div>
