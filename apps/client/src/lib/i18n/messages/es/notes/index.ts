import type { notes as enNotes } from "../../en/notes";
import type { MessageShape } from "../../types";
import { navigationAndSearch } from "./navigation-and-search";
import { collaboration } from "./collaboration";
import { templatesHistory } from "./templates-history";
import { assets } from "./assets";
import { editor } from "./editor";
import { database } from "./database";
import { advancedBlocks } from "./advanced-blocks";
import { pageActions } from "./page-actions";
import { diagnostics } from "./diagnostics";
import { workingMarkdown } from "./working-markdown";
import { propertyDisplay } from "./property-display";
import { rowHierarchy } from "./row-hierarchy";
import { rowPage } from "./row-page";
import { syncMessages } from "./sync";

export const notes = {
  ...navigationAndSearch,
  ...collaboration,
  ...templatesHistory,
  ...assets,
  ...editor,
  ...database,
  ...advancedBlocks,
  ...pageActions,
  ...diagnostics,
  ...workingMarkdown,
  ...propertyDisplay,
  ...rowHierarchy,
  ...rowPage,
  ...syncMessages,
} as const satisfies MessageShape<typeof enNotes>;
