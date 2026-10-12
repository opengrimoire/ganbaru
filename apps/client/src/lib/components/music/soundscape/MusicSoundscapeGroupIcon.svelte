<script lang="ts">
  import ProjectIcon from "$lib/components/projects/ProjectIcon.svelte";

  let { icon, size = 16 }: { icon: string; size?: number } = $props();
</script>

<ProjectIcon name={icon} {size} strokeWidth={1.5} />
