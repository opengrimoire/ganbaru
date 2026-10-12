import { ensureDbUrl } from "$lib/api/db";
import { getProjects } from "$lib/stores/projects.svelte";
import { getNotes } from "$lib/stores/notes.svelte";
import { getChat } from "$lib/stores/chat.svelte";
import { mark as perfMark } from "$lib/stores/perf-log.svelte";

let preparation: Promise<void> | null = null;

/**
 * Prepare the core workspace during onboarding, sharing concurrent startup requests.
 *
 * Each step records a once-only `boot.workspace-*` mark so the diagnostics panel
 * shows which part of preparation delays the post-onboarding loading screen.
 */
export function prepareDesktopWorkspace(): Promise<void> {
  if (preparation) return preparation;
  preparation = (async () => {
    perfMark("boot.workspace-start");
    await ensureDbUrl();
    perfMark("boot.workspace-database-ready");
    const projects = getProjects();
    await projects.ensureLoaded();
    perfMark("boot.workspace-projects-ready");
    await Promise.all([
      getNotes().ensureLoaded().then(() => perfMark("boot.workspace-notes-ready")),
      getChat().prewarmForProject(projects.selectedProjectId)
        .then(() => perfMark("boot.workspace-chat-ready")),
    ]);
    perfMark("boot.workspace-ready");
  })().finally(() => { preparation = null; });
  return preparation;
}
