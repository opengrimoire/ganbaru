<script lang="ts">
  import appIconUrl from "../../../../../src-tauri/icons/64x64.png";
  import { cn } from "$lib/utils";

  /** The app's launcher icon as Android settings show it, a square with soft corners that scale with its size. */
  let { class: className = "size-6" }: { class?: string } = $props();
</script>

<img src={appIconUrl} alt="" class={cn("shrink-0 rounded-[22%]", className)} draggable="false" />
