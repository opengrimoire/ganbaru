import type { ChatRememberableApprovalRead } from "$lib/chat/contracts";
import type { Translate } from "$lib/i18n/translator.svelte";

/**
 * What a remembered approval matches, as a person reads it: the host of a web origin, or the exact command
 * or tool name. An origin that does not parse is shown as stored.
 */
export function rememberedApprovalSubject(rule: ChatRememberableApprovalRead): string {
  if (rule.kind !== "url_origin") return rule.key;
  try {
    return new URL(rule.key).host || rule.key;
  } catch {
    return rule.key;
  }
}

/** The one-sentence description of what "Allow and don't ask again" records for a request. */
export function rememberedApprovalDescription(t: Translate, rule: ChatRememberableApprovalRead): string {
  switch (rule.kind) {
    case "url_origin": return t("chat.composer.rememberOrigin", rememberedApprovalSubject(rule));
    case "command": return t("chat.composer.rememberCommand", rule.key);
    case "tool": return t("chat.composer.rememberTool");
  }
}
