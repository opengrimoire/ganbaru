<script lang="ts">
  import type { ComponentProps } from "svelte";

  type Props = ComponentProps<
    typeof import("$lib/components/music/MusicSoundtrackAssignmentEditor.svelte").default
  >;

  const props: Props = $props();
  $effect(() => {
    Reflect.ownKeys(props);
  });
</script>
