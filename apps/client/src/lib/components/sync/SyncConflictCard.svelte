<script lang="ts">
  import type { Snippet } from "svelte";
  import Info from "@lucide/svelte/icons/info";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { cn } from "$lib/utils";

  let {
    title,
    hint,
    message = "",
    messageKind = "error",
    class: className,
    children,
  }: {
    title: string;
    hint?: string;
    /** Outcome shown under the choices, such as a failed or already resolved conflict. */
    message?: string;
    messageKind?: "resolved" | "error";
    class?: string;
    /** The versions or choices, each in its own row. */
    children: Snippet;
  } = $props();
</script>

<section
  class={cn("flex min-w-0 flex-col gap-2 rounded-lg bg-warning/15 px-3 py-2 text-[0.8rem]", className)}
  aria-label={title}
>
  <div class="font-medium">{title}</div>
  {#if hint}<div class="text-muted-foreground">{hint}</div>{/if}
  {@render children()}
  {#if message}
    <Notice
      {message}
      icon={messageKind === "resolved" ? Info : undefined}
      role={messageKind === "resolved" ? "status" : "alert"}
      copy={false}
    />
  {/if}
</section>
