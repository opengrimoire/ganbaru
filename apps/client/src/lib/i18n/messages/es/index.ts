import type { MessageCatalog } from "../en";
import type { MessageShape } from "../types";
import { benchmark } from "./benchmark";
import { calendar } from "./calendar";
import { chat } from "./chat";
import { collections } from "./collections";
import { common } from "./common";
import { diagnostics } from "./diagnostics";
import { focusDialog, pomodoroNotification, pomodoroOverlay } from "./focus";
import { format } from "./format";
import { music } from "./music";
import { mobile } from "./mobile";
import { notes } from "./notes";
import { contacts } from "./contacts";
import { projects } from "./projects";
import { quickNotes } from "./quick-notes";
import { settings } from "./settings";
import { sync } from "./sync";
import { theme } from "./theme";
import { titleBar } from "./title-bar";
import { updates } from "./updates";
import { dataFolderError, language, vaultHandoff, vaultOwnership, vaultOwnershipPrompt, vaultSetup } from "./vault";
import { window } from "./window";

export const es = {
  common,
  window,
  vaultSetup,
  dataFolderError,
  language,
  vaultOwnership,
  vaultOwnershipPrompt,
  vaultHandoff,
  focusDialog,
  pomodoroOverlay,
  pomodoroNotification,
  calendar,
  chat,
  collections,
  settings,
  updates,
  diagnostics,
  benchmark,
  notes,
  contacts,
  projects,
  quickNotes,
  sync,
  music,
  mobile,
  titleBar,
  format,
  theme,
} as const satisfies MessageShape<MessageCatalog>;
