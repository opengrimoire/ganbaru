<script lang="ts">
  /** Fixed role label for members whose role cannot be changed in place, such as the owner. */
  let { label }: { label: string } = $props();
</script>

<span class="member-role-chip">{label}</span>

<style>
  .member-role-chip { display: inline-flex; height: 1.25rem; flex: 0 0 auto; align-items: center; border: 1px solid var(--border); border-radius: 9999px; padding-inline: 0.5rem; color: var(--muted-foreground); font-size: var(--panel-detail-font-size); font-weight: 500; white-space: nowrap; }
</style>
