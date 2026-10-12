<script lang="ts">
  import BookOpen from "@lucide/svelte/icons/book-open";
  import CalendarDays from "@lucide/svelte/icons/calendar-days";
  import ListTodo from "@lucide/svelte/icons/list-todo";
  import MessageSquare from "@lucide/svelte/icons/message-square";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { View } from "$lib/navigation";
  import type { MobileNavigationPresentation } from "$lib/mobile/layout";
  import { cn } from "$lib/utils";

  let {
    current,
    presentation,
    onNavigate,
  }: {
    current: View;
    presentation: MobileNavigationPresentation;
    onNavigate: (view: View) => void;
  } = $props();

  const { t } = getLocalization();
  const destinations = [
    { view: "calendar", icon: CalendarDays },
    { view: "projects", icon: ListTodo },
    { view: "notes", icon: BookOpen },
    { view: "chat", icon: MessageSquare },
  ] as const;
</script>

<nav
  aria-label={t("mobile.primaryNavigation")}
  class={cn(
    "mobile-primary-navigation",
    presentation === "rail"
      ? "flex w-(--mobile-nav-rail-w) shrink-0 flex-col border-r border-sidebar-border bg-sidebar px-2 py-3 text-sidebar-foreground"
      : "grid h-(--mobile-bottom-nav-h) shrink-0 grid-cols-4 border-t border-sidebar-border px-2 text-foreground",
  )}
  style={presentation === "bottom" ? "background-color: var(--cal-bg);" : undefined}
>
  {#each destinations as destination}
    {@const Icon = destination.icon}
    {@const selected = current === destination.view}
    <button
      type="button"
      aria-current={selected ? "page" : undefined}
      onclick={() => onNavigate(destination.view)}
      class={cn(
        "group relative flex min-w-0 flex-col items-center justify-center gap-1 text-xs font-medium transition-colors",
        presentation === "rail" && "mb-2 min-h-12 rounded-xl",
        presentation === "rail"
          ? selected
            ? "bg-sidebar-accent text-sidebar-accent-foreground"
            : "text-sidebar-foreground/70 active:bg-sidebar-accent/70"
          : selected
            ? "text-foreground"
            : "text-muted-foreground",
      )}
    >
      {#if presentation === "bottom"}
        <span
          class={cn("grid h-8 w-14 place-items-center rounded-full transition-colors", selected ? "bg-accent" : "group-active:bg-accent/60")}
          data-mobile-navigation-indicator={selected || undefined}
        >
          <Icon size={20} strokeWidth={selected ? 2 : 1.7} aria-hidden="true" />
        </span>
      {:else}
        <Icon size={21} strokeWidth={selected ? 2 : 1.7} aria-hidden="true" />
      {/if}
      <span class="max-w-full truncate">{t(`titleBar.tab.${destination.view}`)}</span>
    </button>
  {/each}
</nav>
