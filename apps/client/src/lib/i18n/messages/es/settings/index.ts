import type { settings as enSettings } from "../../en/settings";
import type { MessageShape } from "../../types";
import { distractions } from "./distractions";
import { focusAndShortcuts } from "./focus-and-shortcuts";
import { general } from "./general";
import { agentsSettings } from "./agents";
import { chatSettings } from "./chat";
import { theme } from "./theme";

export const settings = {
  ...general,
  theme,
  ...focusAndShortcuts,
  chat: chatSettings,
  agents: agentsSettings,
  distractions,
} as const satisfies MessageShape<typeof enSettings>;
