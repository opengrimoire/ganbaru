<script lang="ts">
  let { name }: {
    /** Name of the open channel or page, as the user sees it. */
    name: string;
  } = $props();
</script>

<!-- The phone top bar names only the project, so the action row names the open channel or page; it takes the space left by the controls before it and truncates first. -->
<span class="min-w-0 flex-1 truncate px-2 text-identity font-medium text-foreground" data-mobile-workspace-name>{name}</span>
