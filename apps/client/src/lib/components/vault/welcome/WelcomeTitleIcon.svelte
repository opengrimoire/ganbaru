<script lang="ts">
  import type FileText from "@lucide/svelte/icons/file-text";
  import { cn } from "$lib/utils";

  /** A desktop welcome scene's title bar button, drawn as the title bar draws its utilities, and lit while its panel is open. */
  let { icon, open = false }: { icon: typeof FileText; open?: boolean } = $props();

  /** The title bar's icon size and stroke, as the title bar draws them. */
  const ICON_SIZE = 14;
  const ICON_STROKE_WIDTH = 1.5;
  const Icon = $derived(icon);
</script>

<span class={cn("grid size-7 place-items-center rounded-lg", open ? "bg-sidebar-accent text-foreground" : "text-foreground/68 dark:text-white/76")}>
  <Icon size={ICON_SIZE} strokeWidth={ICON_STROKE_WIDTH} />
</span>
