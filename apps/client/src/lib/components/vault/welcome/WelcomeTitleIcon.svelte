<script lang="ts">
  import type FileText from "@lucide/svelte/icons/file-text";
  import { cn } from "$lib/utils";

  /**
   * A desktop welcome scene's title bar button, drawn as the title bar draws its utilities: lit while its panel is
   * open or the pointer is on it.
   */
  let {
    icon,
    open = false,
    hovered = false,
    cursor,
  }: {
    icon: typeof FileText;
    open?: boolean;
    /** Whether the pointer is on the button. */
    hovered?: boolean;
    /** The name the scene's pointer finds the button by. */
    cursor?: string;
  } = $props();

  /** The title bar's icon size and stroke, as the title bar draws them. */
  const ICON_SIZE = 14;
  const ICON_STROKE_WIDTH = 1.5;
  const Icon = $derived(icon);
</script>

<span
  data-cursor={cursor}
  class={cn(
    "grid size-7 place-items-center rounded-lg",
    open ? "bg-sidebar-accent text-foreground" : "text-foreground/68 dark:text-white/76",
    hovered && "bg-sidebar-accent",
  )}
>
  <Icon size={ICON_SIZE} strokeWidth={ICON_STROKE_WIDTH} />
</span>
