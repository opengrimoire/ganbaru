<script lang="ts">
  import ArrowDown from "@lucide/svelte/icons/arrow-down";
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import ArrowRight from "@lucide/svelte/icons/arrow-right";
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import { getLocalization } from "$lib/i18n/translator.svelte";

  /**
   * Move up and Move down items, or Move left and Move right for a row of items, for a menu in a custom order: the path
   * for touch and keyboard where dragging is unavailable. An item at the end it would move past stays listed but
   * disabled, so the menu keeps its shape.
   */
  let {
    canMoveUp,
    canMoveDown,
    onMove,
    onClose,
    orientation = "vertical",
  }: {
    /** Whether the item can move one place back: up, or left in a row. */
    canMoveUp: boolean;
    /** Whether the item can move one place forward: down, or right in a row. */
    canMoveDown: boolean;
    /** Moves the item one place back (`-1`) or forward (`1`). */
    onMove: (step: -1 | 1) => void;
    /** Closes the menu before the item moves. */
    onClose: () => void;
    /** `horizontal` labels the moves left and right, for items laid out in a row. */
    orientation?: "vertical" | "horizontal";
  } = $props();

  const { t } = getLocalization();
  const horizontal = $derived(orientation === "horizontal");

  function move(step: -1 | 1, allowed: boolean): void {
    if (!allowed) return;
    onClose();
    onMove(step);
  }
</script>

<button
  type="button"
  role="menuitem"
  class="menu-item"
  aria-disabled={canMoveUp ? undefined : "true"}
  onclick={() => move(-1, canMoveUp)}
>
  {#if horizontal}<ArrowLeft />{:else}<ArrowUp />{/if}
  <span>{horizontal ? t("common.moveLeft") : t("common.moveUp")}</span>
</button>
<button
  type="button"
  role="menuitem"
  class="menu-item"
  aria-disabled={canMoveDown ? undefined : "true"}
  onclick={() => move(1, canMoveDown)}
>
  {#if horizontal}<ArrowRight />{:else}<ArrowDown />{/if}
  <span>{horizontal ? t("common.moveRight") : t("common.moveDown")}</span>
</button>
