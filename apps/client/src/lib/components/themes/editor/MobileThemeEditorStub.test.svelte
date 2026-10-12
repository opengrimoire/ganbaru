<script lang="ts">
  import type { Theme } from "$lib/themes";

  let { theme }: { theme: Theme } = $props();
</script>

<div data-theme-editor-stub>{theme.displayName}</div>
