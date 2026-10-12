<script lang="ts">
  import { onMount } from "svelte";
  import { getSoundscapeStore } from "$lib/stores/soundscape.svelte";
  import { getMusicPlayer } from "$lib/stores/music-player.svelte";
  import { onActiveVaultIdentityChange } from "$lib/vault/active-vault";
  import { shouldRecoverSoundscapeOutput } from "$lib/music/soundscape/lifecycle";

  const soundscape = getSoundscapeStore();
  const player = getMusicPlayer();
  $effect(() => {
    const version = player.soundscapeVersion;
    if (version !== null && version !== undefined) void soundscape.refreshAcceptedOutput();
  });
  onMount(() => {
    let hiddenAt: number | null = null;
    const onVisibility = () => {
      if (document.visibilityState === "hidden") { hiddenAt = Date.now(); return; }
      const hiddenForMs = hiddenAt === null ? 0 : Math.max(0, Date.now() - hiddenAt);
      hiddenAt = null;
      if (shouldRecoverSoundscapeOutput({ desiredPlaying: soundscape.persisted?.desiredPlaying ?? false, status: soundscape.snapshot.status, hiddenForMs })) void soundscape.recover();
    };
    const unsubscribeVault = onActiveVaultIdentityChange((_previous, next) => { void soundscape.switchVault(next); });
    document.addEventListener("visibilitychange", onVisibility);
    void soundscape.initialize();
    return () => { unsubscribeVault(); document.removeEventListener("visibilitychange", onVisibility); };
  });
</script>
