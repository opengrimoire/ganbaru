<script lang="ts">
  import { untrack } from "svelte";
  import { createTitleBarShortcutController, type TitleBarShortcutControllerContext } from "./shortcut-controller.svelte";
  let { context }: { context: TitleBarShortcutControllerContext } = $props();
  // The test host has the same lifetime as the shell shortcut registration.
  createTitleBarShortcutController(untrack(() => context));
</script>
