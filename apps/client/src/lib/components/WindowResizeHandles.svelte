<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";

  type ResizeDirection =
    | "East"
    | "North"
    | "NorthEast"
    | "NorthWest"
    | "South"
    | "SouthEast"
    | "SouthWest"
    | "West";

  let { disabled = false }: { disabled?: boolean } = $props();

  const appWindow = getCurrentWindow();
  const EDGE_HANDLE_PX = 10;
  const CORNER_HANDLE_PX = 20;

  function startResize(direction: ResizeDirection, e: PointerEvent) {
    if (disabled || !e.isPrimary || e.button !== 0) return;
    e.preventDefault();
    e.stopPropagation();
    void appWindow.startResizeDragging(direction).catch((error) => {
      console.error("Failed to start window resize:", error);
    });
  }

  function resizeHandle(node: HTMLElement, direction: ResizeDirection) {
    const handlePointerDown = (e: PointerEvent) => startResize(direction, e);
    node.addEventListener("pointerdown", handlePointerDown);

    return {
      destroy() {
        node.removeEventListener("pointerdown", handlePointerDown);
      },
    };
  }
</script>

{#if !disabled}
  <div aria-hidden="true" class="pointer-events-none fixed inset-0 z-2147483647">
    <div
      class="pointer-events-auto absolute cursor-n-resize"
      style="left: {CORNER_HANDLE_PX}px; right: {CORNER_HANDLE_PX}px; top: 0; height: {EDGE_HANDLE_PX}px;"
      use:resizeHandle={"North"}
    ></div>
    <div
      class="pointer-events-auto absolute cursor-s-resize"
      style="left: {CORNER_HANDLE_PX}px; right: {CORNER_HANDLE_PX}px; bottom: 0; height: {EDGE_HANDLE_PX}px;"
      use:resizeHandle={"South"}
    ></div>
    <div
      class="pointer-events-auto absolute cursor-w-resize"
      style="left: 0; top: {CORNER_HANDLE_PX}px; bottom: {CORNER_HANDLE_PX}px; width: {EDGE_HANDLE_PX}px;"
      use:resizeHandle={"West"}
    ></div>
    <div
      class="pointer-events-auto absolute cursor-e-resize"
      style="right: 0; top: {CORNER_HANDLE_PX}px; bottom: {CORNER_HANDLE_PX}px; width: {EDGE_HANDLE_PX}px;"
      use:resizeHandle={"East"}
    ></div>
    <div
      class="pointer-events-auto absolute left-0 top-0 cursor-nw-resize"
      style="width: {CORNER_HANDLE_PX}px; height: {CORNER_HANDLE_PX}px;"
      use:resizeHandle={"NorthWest"}
    ></div>
    <div
      class="pointer-events-auto absolute right-0 top-0 cursor-ne-resize"
      style="width: {CORNER_HANDLE_PX}px; height: {CORNER_HANDLE_PX}px;"
      use:resizeHandle={"NorthEast"}
    ></div>
    <div
      class="pointer-events-auto absolute bottom-0 left-0 cursor-sw-resize"
      style="width: {CORNER_HANDLE_PX}px; height: {CORNER_HANDLE_PX}px;"
      use:resizeHandle={"SouthWest"}
    ></div>
    <div
      class="pointer-events-auto absolute bottom-0 right-0 cursor-se-resize"
      style="width: {CORNER_HANDLE_PX}px; height: {CORNER_HANDLE_PX}px;"
      use:resizeHandle={"SouthEast"}
    ></div>
  </div>
{/if}
