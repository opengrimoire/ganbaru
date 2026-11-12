import type { MessageCatalog } from "$lib/i18n/messages/en";
import type { Translate } from "$lib/i18n/translator.svelte";

type NoticeTitleKey = `chat.timeline.noticeTitles.${keyof MessageCatalog["chat"]["timeline"]["noticeTitles"] & string}`;

/**
 * Catalog titles for provider notice codes whose title is fixed. Codes whose provider title
 * interpolates runtime values, or that cover several distinct titles, are left out so the
 * provider title stays visible for them.
 */
const NOTICE_TITLE_KEYS: Readonly<Record<string, NoticeTitleKey>> = {
  claude_api_retry: "chat.timeline.noticeTitles.claudeApiRetry",
  claude_permission_denied: "chat.timeline.noticeTitles.claudePermissionDenied",
  claude_malformed_message: "chat.timeline.noticeTitles.claudeMalformedMessage",
  opencode_question_reject_failed: "chat.timeline.noticeTitles.opencodeQuestionRejectFailed",
  opencode_session_rules_trimmed: "chat.timeline.noticeTitles.opencodeSessionRulesTrimmed",
  opencode_question_outside_turn: "chat.timeline.noticeTitles.opencodeQuestionOutsideTurn",
  codex_elicitation_declined: "chat.timeline.noticeTitles.codexElicitationDeclined",
  codex_organizational_authority_escalation_denied: "chat.timeline.noticeTitles.codexAuthorityEscalationDenied",
  codex_capacity_retry: "chat.timeline.noticeTitles.codexCapacityRetry",
  codex_resume_not_found_fresh_start: "chat.timeline.noticeTitles.codexResumeNotFound",
  streamed_text_limit: "chat.timeline.noticeTitles.streamedTextLimit",
  provider_interrupt_unconfirmed: "chat.timeline.noticeTitles.providerInterruptUnconfirmed",
  late_provider_event: "chat.timeline.noticeTitles.lateProviderEvent",
  provider_memory_limit: "chat.timeline.noticeTitles.providerMemoryLimit",
};

/**
 * Returns the localized title of a provider warning or notice.
 *
 * Known codes use the Chat catalog; any other code keeps the provider title, and an empty
 * provider title falls back to the generic notice label.
 */
export function resolveNoticeTitle(code: string, providerTitle: string, t: Translate): string {
  const key = Object.hasOwn(NOTICE_TITLE_KEYS, code) ? NOTICE_TITLE_KEYS[code] : undefined;
  if (key) return t(key);
  const title = providerTitle.trim();
  return title.length > 0 ? title : t("chat.timeline.activityStatus.unknown");
}

/** Returns the provider code a stored notice row carries in its safe metadata, if any. */
export function noticeCode(metadata: unknown): string | null {
  if (typeof metadata !== "object" || metadata === null || Array.isArray(metadata)) return null;
  const code: unknown = "code" in metadata ? metadata.code : null;
  return typeof code === "string" && code.length > 0 ? code : null;
}
