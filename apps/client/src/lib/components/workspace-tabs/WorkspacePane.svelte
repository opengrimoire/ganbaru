<script lang="ts">
  import type { Snippet } from "svelte";
  import type { WorkspacePaneScope } from "$lib/workspace-tabs/model";
  import { provideWorkspacePaneFocus } from "$lib/workspace-tabs/pane-focus";
  import { provideWorkspacePaneScope } from "$lib/workspace-tabs/pane-scope";

  let {
    focused,
    scope,
    hidden,
    painted,
    order,
    basis,
    onFocus,
    children,
  }: {
    /** Whether window-wide shortcuts act on this pane. */
    focused: boolean;
    /** The set of view stores the view shows through. */
    scope: WorkspacePaneScope;
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
  provideWorkspacePaneScope(() => scope);

  /** Focus the pane when keyboard navigation moves into it, but not when a view focuses itself after a click. */
  function handleFocusIn(event: FocusEvent): void {
    if (event.target instanceof Element && event.target.matches(":focus-visible")) onFocus();
  }
</script>

<div
  class="workspace-pane relative min-h-0 min-w-0 overflow-hidden rounded-(--content-radius)"
  class:hidden
  class:surface-content={painted}
  style:order
  style:flex={basis === null ? "1 1 0" : `0 0 ${basis}`}
  onpointerdowncapture={onFocus}
  onfocusin={handleFocusIn}
>
  {@render children()}
</div>
