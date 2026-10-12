<script lang="ts">
  import type { NotesWorkingMarkdownTreeProps } from "./tree-contracts";

  const props: NotesWorkingMarkdownTreeProps = $props();
  $effect(() => {
    Reflect.ownKeys(props);
  });
</script>
