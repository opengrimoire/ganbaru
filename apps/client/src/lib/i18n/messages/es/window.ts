import type { window as enWindow } from "../en/window";
import type { MessageShape } from "../types";

export const window = {
  close: "Cerrar",
  closeWindowWithShortcut: (shortcut: string) => `Cerrar ventana (${shortcut})`,
} as const satisfies MessageShape<typeof enWindow>;
