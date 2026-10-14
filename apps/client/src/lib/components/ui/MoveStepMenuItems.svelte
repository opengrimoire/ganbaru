<script lang="ts">
  import ArrowDown from "@lucide/svelte/icons/arrow-down";
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import { getLocalization } from "$lib/i18n/translator.svelte";

  /**
   * Move up and Move down items for a row menu in a custom order, the path for touch and keyboard where dragging is
   * unavailable. An item at the end it would move past stays listed but disabled, so the menu keeps its shape.
   */
  let {
    canMoveUp,
    canMoveDown,
    onMove,
    onClose,
  }: {
    canMoveUp: boolean;
    canMoveDown: boolean;
    /** Moves the row one place up (`-1`) or down (`1`). */
    onMove: (step: -1 | 1) => void;
    /** Closes the menu before the row moves. */
    onClose: () => void;
  } = $props();

  const { t } = getLocalization();

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
  <ArrowUp />
  <span>{t("common.moveUp")}</span>
</button>
<button
  type="button"
  role="menuitem"
  class="menu-item"
  aria-disabled={canMoveDown ? undefined : "true"}
  onclick={() => move(1, canMoveDown)}
>
  <ArrowDown />
  <span>{t("common.moveDown")}</span>
</button>
