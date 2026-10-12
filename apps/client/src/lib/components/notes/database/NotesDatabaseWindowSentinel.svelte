<script lang="ts">
  import Loading from "$lib/components/ui/Loading.svelte";

  let {
    hasMore,
    loading,
    onLoad,
  }: {
    hasMore: boolean;
    loading: boolean;
    onLoad: () => Promise<void> | void;
  } = $props();

  let sentinel: HTMLDivElement | null = $state(null);

  $effect(() => {
    const node = sentinel;
    if (!node || !hasMore || typeof IntersectionObserver === "undefined") return;
    const observer = new IntersectionObserver((entries) => {
      if (!loading && entries.some((entry) => entry.isIntersecting)) void onLoad();
    }, { root: node.closest("[data-notes-editor-scroll]") });
    observer.observe(node);
    return () => observer.disconnect();
  });
</script>

{#if hasMore}
  <div bind:this={sentinel} class="h-px" aria-hidden="true"></div>
  {#if loading}
    <Loading layout="rows" rows={2} />
  {/if}
{/if}
