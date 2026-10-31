import { shell } from "./shell";
import { views } from "./views";
import { configuration } from "./configuration";
import { taskWorkflow } from "./task-workflow";
import { syncMessages } from "./sync";

export const projects = {
  ...shell,
  ...views,
  ...configuration,
  ...taskWorkflow,
  ...syncMessages,
} as const;
