<script lang="ts">
  import ActionToast from "$lib/components/ui/ActionToast.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getNavigation } from "$lib/stores/navigation.svelte";
  import { notesHeldEditNotice } from "$lib/stores/notes/held-edit-notice.svelte";
  import { writeTextToClipboard } from "$lib/utils/clipboard";

  /**
   * Tells the user, wherever they are in the app, that a held Notes edit refused a close, tab move, or hide, and
   * opens Notes, whose pane still shows the page that waits for a choice. When that page was removed, it keeps the
   * held text until the user copies it or dismisses the notice.
   */
  const { t } = getLocalization();
  const nav = getNavigation();
  const notice = $derived(notesHeldEditNotice.current);
  /** The removed text whose last copy failed, so the failure shows only while that exact text is still the notice. */
  let copyFailedText = $state.raw<string | null>(null);
  const copyFailed = $derived(notice?.kind === "removed" && notice.text === copyFailedText);

  function openNotes(): void {
    notesHeldEditNotice.dismiss();
    nav.navigate("notes");
  }

  /**
   * Whether `text` is still the removed text the notice shows.
   *
   * @param text The text a copy started with.
   */
  function isShownRemovedText(text: string): boolean {
    const shown = notesHeldEditNotice.current;
    return shown?.kind === "removed" && shown.text === text;
  }

  /**
   * Copies the removed text and dismisses the notice, unless the notice changed while the copy ran, for example
   * because more removed text was appended to it, so text that was not copied is never dismissed.
   *
   * @param text The removed text to copy.
   */
  async function copyText(text: string): Promise<void> {
    try {
      await writeTextToClipboard(text);
      if (!isShownRemovedText(text)) return;
      copyFailedText = null;
      notesHeldEditNotice.dismiss();
    } catch (cause: unknown) {
      console.warn("Held Notes text could not be copied", cause);
      if (isShownRemovedText(text)) copyFailedText = text;
    }
  }

  function dismiss(): void {
    copyFailedText = null;
    notesHeldEditNotice.dismiss();
  }
</script>

{#if notice?.kind === "held"}
  <ActionToast
    message={t("notes.conflict.heldNotice")}
    actionLabel={t("notes.conflict.heldOpen")}
    onAction={openNotes}
    onDismiss={dismiss}
  />
{:else if notice?.kind === "removed"}
  {@const text = notice.text}
  <ActionToast
    message={copyFailed ? t("notes.conflict.removedCopyFailed") : t("notes.conflict.removedNotice")}
    variant={copyFailed ? "error" : "default"}
    actionLabel={t("notes.conflict.removedCopy")}
    onAction={() => copyText(text)}
    onDismiss={dismiss}
  />
{/if}
