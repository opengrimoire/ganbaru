import { invoke } from "@tauri-apps/api/core";
import { ensureDbUrl } from "$lib/api/db";
import type { ProjectWorkingFolderId, ProjectWorkingFolderRead } from "$lib/chat/contracts";
import { chatErrorCode } from "$lib/chat/error-presentation";
import { parseProjectWorkingFolderRead } from "$lib/chat/validation";
import { translate } from "$lib/i18n/translator.svelte";

/** Localized text for the native trust dialog; the path and digests are resolved natively. */
function trustPrompt() {
  return {
    title: translate("projects.settings.workingFolders.trustTitle"),
    message: translate("projects.settings.workingFolders.trustMessage"),
    trustLabel: translate("projects.settings.workingFolders.trustAccept"),
    untrustedLabel: translate("projects.settings.workingFolders.trustDecline"),
    cancelLabel: translate("projects.settings.workingFolders.trustCancel"),
  };
}

/**
 * Records this device's trust decision for a folder, asking natively only when the folder
 * defines repository programs, hooks, or provider settings that no current decision covers.
 * Returns `null` when the user cancels the dialog.
 */
export async function decideProjectWorkingFolderTrust(
  workingFolderId: ProjectWorkingFolderId,
): Promise<ProjectWorkingFolderRead | null> {
  const value = await invoke<unknown>("projects_decide_working_folder_trust", {
    dbUrl: await ensureDbUrl(),
    workingFolderId,
    prompt: trustPrompt(),
  });
  return value === null ? null : parseProjectWorkingFolderRead(value);
}

/** Turns off repository programs and hooks for a folder on this device until it is trusted again. */
export async function revokeProjectWorkingFolderTrust(
  workingFolderId: ProjectWorkingFolderId,
): Promise<ProjectWorkingFolderRead> {
  return parseProjectWorkingFolderRead(await invoke<unknown>("projects_revoke_working_folder_trust", {
    dbUrl: await ensureDbUrl(),
    workingFolderId,
  }));
}

/**
 * Asks for a trust decision right after a native binding so later Git actions run without a prompt.
 * Binding has already succeeded, so a failed or cancelled decision keeps the bound folder and
 * leaves trust undecided; the first user Git action asks again.
 */
export async function decideTrustAfterBinding(
  bound: ProjectWorkingFolderRead,
): Promise<ProjectWorkingFolderRead> {
  try {
    return await decideProjectWorkingFolderTrust(bound.workingFolder.id) ?? bound;
  } catch (reason) {
    console.warn("Working folder trust decision failed after binding", reason);
    return bound;
  }
}

/**
 * Runs a user Git action. When Rust reports that the folder needs a trust decision, asks once
 * natively and retries; a cancelled dialog surfaces the original failure.
 */
export async function withWorkingFolderTrust<T>(
  workingFolderId: ProjectWorkingFolderId,
  action: () => Promise<T>,
  decide: (id: ProjectWorkingFolderId) => Promise<unknown> = decideProjectWorkingFolderTrust,
): Promise<T> {
  try {
    return await action();
  } catch (reason) {
    if (chatErrorCode(reason) !== "trust_decision_required") throw reason;
    if (await decide(workingFolderId) === null) throw reason;
    return action();
  }
}

/**
 * Starts a provider turn or fork. When Rust reports that the folder named in the error needs a
 * decision about its project provider configuration, asks once natively and retries; a
 * cancelled dialog surfaces the original failure.
 */
export async function withProviderStartTrust<T>(
  action: () => Promise<T>,
  decide: (id: ProjectWorkingFolderId) => Promise<unknown> = decideProjectWorkingFolderTrust,
): Promise<T> {
  try {
    return await action();
  } catch (reason) {
    const workingFolderId = trustDecisionFolder(reason);
    if (workingFolderId === null) throw reason;
    if (await decide(workingFolderId) === null) throw reason;
    return action();
  }
}

/** The folder a `trust_decision_required` failure names, or `null` for any other failure. */
function trustDecisionFolder(reason: unknown): ProjectWorkingFolderId | null {
  if (chatErrorCode(reason) !== "trust_decision_required") return null;
  if (typeof reason !== "object" || reason === null || !("details" in reason)) return null;
  const details = reason.details;
  if (typeof details !== "object" || details === null || !("workingFolderId" in details)) return null;
  const workingFolderId = details.workingFolderId;
  return typeof workingFolderId === "string" && workingFolderId.trim() ? workingFolderId : null;
}
