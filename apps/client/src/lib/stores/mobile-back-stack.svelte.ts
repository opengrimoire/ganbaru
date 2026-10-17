import type { MobileBackLayer, MobileBackStack } from "./mobile-back-stack-contracts";
import { untrack } from "svelte";

export type { MobileBackLayer, MobileBackStack } from "./mobile-back-stack-contracts";

interface RegisteredMobileBackLayer extends MobileBackLayer {
  id: symbol;
}

let layers = $state<RegisteredMobileBackLayer[]>([]);

/**
 * Coordinate mobile overlays and nested surfaces as a last-opened-first-closed stack.
 *
 * Callers register from an effect only while their layer is active. Registering
 * at activation time makes array order match visual stacking order.
 */
export function getMobileBackStack(): MobileBackStack {
  return {
    get hasActiveLayer(): boolean {
      return layers.length > 0;
    },

    activate(layer: MobileBackLayer): () => void {
      const id = Symbol("mobile-back-layer");
      layers = [...untrack(() => layers), { ...layer, id }];
      let registered = true;
      return () => {
        if (!registered) return;
        registered = false;
        layers = untrack(() => layers).filter((candidate) => candidate.id !== id);
      };
    },

    consume(): boolean {
      const layer = layers[layers.length - 1];
      if (!layer) return false;
      layer.handle();
      return true;
    },
  };
}
