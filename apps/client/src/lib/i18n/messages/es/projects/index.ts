import type { projects as enProjects } from "../../en/projects";
import type { MessageShape } from "../../types";
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
} as const satisfies MessageShape<typeof enProjects>;
