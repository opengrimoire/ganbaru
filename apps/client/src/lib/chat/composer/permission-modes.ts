import type { ProviderFamilyId, SafetyMode } from "$lib/chat/contracts";

/**
 * Reports whether a provider can faithfully implement a permission mode.
 *
 * @param familyId - Provider family selected in the Chat composer.
 * @param mode - Permission mode requested by the user.
 * @returns Whether Ganbaru has a native mapping for the provider and mode.
 */
export function providerSupportsPermissionMode(
  familyId: ProviderFamilyId | null,
  mode: SafetyMode,
): boolean {
  if (familyId === null) return false;
  if (mode === "ask_for_approval" || mode === "full_access") return true;
  if (mode === "approve_for_me") return familyId === "codex" || familyId === "claude";
  return true;
}

/** Returns the documented user configuration filename that owns provider permissions. */
export function providerPermissionFileName(familyId: ProviderFamilyId | null): string {
  switch (familyId) {
    case "codex":
    case "grok":
      return "config.toml";
    case "claude":
      return "settings.json";
    case "cursor":
      return "cli-config.json";
    case "opencode":
      return "opencode.json/JSONC";
    default:
      return "provider configuration";
  }
}
