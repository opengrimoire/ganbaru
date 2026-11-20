<script lang="ts">
  import type { ApprovalSubject, ApprovalSubjectKind } from "$lib/chat/contracts";
  import { getLocalization } from "$lib/i18n/translator.svelte";

  /** The exact command, path, or other target an approval acts on, shown before any decision. */
  let { subject }: { subject: ApprovalSubject } = $props();

  const { t } = getLocalization();

  function subjectLabel(kind: ApprovalSubjectKind): string {
    switch (kind) {
      case "command": return t("chat.composer.approvalSubjectCommand");
      case "path": return t("chat.composer.approvalSubjectPath");
      case "url": return t("chat.composer.approvalSubjectUrl");
      case "pattern": return t("chat.composer.approvalSubjectPattern");
      case "query": return t("chat.composer.approvalSubjectQuery");
      case "tool": return t("chat.composer.approvalSubjectTool");
    }
  }
</script>

<div class="approval-subject" data-testid="approval-subject">
  <small>{subjectLabel(subject.kind)}</small>
  <pre>{subject.text}</pre>
  {#if subject.location}<small>{t("chat.composer.approvalSubjectLocation", subject.location)}</small>{/if}
  {#if subject.truncated}<small>{t("chat.composer.approvalSubjectTruncated")}</small>{/if}
</div>

<style>
  .approval-subject { display: grid; gap: 0.2rem; }
  small { color: var(--muted-foreground); font-size: calc(0.666667rem * var(--type-scale)); }
  pre { max-height: 10rem; overflow: auto; overflow-wrap: anywhere; white-space: pre-wrap; border-radius: 0.35rem; background: var(--muted); padding: 0.5rem; font-size: calc(0.666667rem * var(--type-scale)); }
</style>
