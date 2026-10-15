<script lang="ts">
  import type { Snippet } from "svelte";
  import { provideWorkspacePaneFocus } from "$lib/workspace-tabs/pane-focus";

  let {
    focused,
    outlined,
    hidden,
    painted,
    order,
    basis,
    onFocus,
    children,
  }: {
    /** Whether window-wide shortcuts act on this pane. */
    focused: boolean;
    /** Whether to mark this pane as the focused one beside another. */
    outlined: boolean;
    hidden: boolean;
    /** Whether the view paints no content surface of its own, so the pane paints it. */
    painted: boolean;
    order: number;
    /** Fixed share of the split for the first pane, or null for the pane that takes the rest. */
    basis: string | null;
    onFocus: () => void;
    children: Snippet;
  } = $props();

  provideWorkspacePaneFocus(() => focused);

  /** Focus the pane when keyboard navigation moves into it, but not when a view focuses itself after a click. */
  function handleFocusIn(event: FocusEvent): void {
    if (event.target instanceof Element && event.target.matches(":focus-visible")) onFocus();
  }
</script>

<div
  class="workspace-pane relative min-h-0 min-w-0 overflow-hidden rounded-(--content-radius)"
  class:hidden
  class:surface-content={painted}
  class:workspace-pane-outlined={outlined}
  style:order
  style:flex={basis === null ? "1 1 0" : `0 0 ${basis}`}
  onpointerdowncapture={onFocus}
  onfocusin={handleFocusIn}
>
  {@render children()}
</div>

<style>
  .workspace-pane-outlined {
    outline: 1px solid color-mix(in srgb, var(--ring) 55%, transparent);
    outline-offset: -1px;
  }
</style>
