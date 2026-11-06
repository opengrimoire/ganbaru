<script lang="ts">
  import ActionToast from "$lib/components/ui/ActionToast.svelte";
  import type { LanguagePreference } from "$lib/i18n/locales";
  import {
    browserLocaleCandidates,
    plannedLanguageEntries,
    preVaultLanguageChoices,
    writePreVaultLanguagePreference,
    type PlannedLanguageEntry,
  } from "$lib/i18n/pre-vault-language";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { cn } from "$lib/utils";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import WelcomeStepHeader from "./WelcomeStepHeader.svelte";

  /**
   * The first welcome screen: the app's language. Each language is named in English with its own name underneath, so
   * the alphabetical order stays the same whatever the interface language, and every name reads left to right here.
   * Choosing one applies it at once, so the rest of the welcome reads in it. The choice is remembered until vault
   * setup saves it. Planned languages follow, dimmed; touching one shows a toast, in that language, saying it is
   * coming, just above the navigation.
   */

  const localization = getLocalization();
  const { t } = localization;
  const headingId = $props.id();
  const NOTICE_TIMEOUT_MS = 3_000;

  const systemCandidates = browserLocaleCandidates(typeof navigator === "undefined" ? undefined : navigator);
  const preference = $derived(localization.languagePreference);
  const choices = $derived(preVaultLanguageChoices(systemCandidates, t("language.systemOption")));
  const planned = plannedLanguageEntries();
  let notice = $state<PlannedLanguageEntry | null>(null);
  let noticeTimer: ReturnType<typeof setTimeout> | undefined;

  function clearNoticeTimer(): void {
    if (noticeTimer) clearTimeout(noticeTimer);
    noticeTimer = undefined;
  }

  function showNotice(entry: PlannedLanguageEntry): void {
    clearNoticeTimer();
    notice = entry;
    noticeTimer = setTimeout(() => {
      notice = null;
      noticeTimer = undefined;
    }, NOTICE_TIMEOUT_MS);
  }

  function dismissNotice(): void {
    clearNoticeTimer();
    notice = null;
  }

  $effect(() => clearNoticeTimer);

  function safeStorage(): Storage | undefined {
    try {
      return window.localStorage;
    } catch {
      return undefined;
    }
  }

  async function choose(value: LanguagePreference): Promise<void> {
    const applied = await localization.setLanguagePreference(value, { persist: false });
    if (applied) writePreVaultLanguagePreference(safeStorage(), value);
  }
</script>

<section class="relative flex h-full flex-col">
  <div use:scrollEdgeFadeAction={{ edges: "bottom" }} class="min-h-0 flex-1 overflow-x-hidden overflow-y-auto">
    <div class="mx-auto flex w-full max-w-xl flex-col gap-8 px-4 pb-6 pt-12 @min-[560px]:px-8 @min-[560px]:pt-18 @min-[860px]:gap-10 @min-[860px]:pt-26">
      <WelcomeStepHeader
        headingId="{headingId}-heading"
        headline={t("vaultSetup.language.headline")}
        body={t("vaultSetup.language.body")}
      />

      <div class="grid gap-1.5">
        <div role="radiogroup" aria-labelledby="{headingId}-heading" class="grid gap-1.5">
          {#each choices as choice (choice.value)}
            {@const selected = choice.value === preference}
            <label
              class={cn(
                "flex min-h-14 cursor-pointer items-center gap-4 rounded-lg px-4 py-2.5 transition-colors has-focus-visible:bg-accent",
                selected ? "bg-accent" : "hover:bg-accent/60",
              )}
            >
              <input
                type="radio"
                name="{headingId}-language"
                value={choice.value}
                checked={selected}
                onchange={() => void choose(choice.value)}
                class="sr-only"
              />
              <span class="min-w-0 flex-1">
                <span lang={choice.titleLang ?? undefined} class="block truncate text-base font-medium text-foreground">
                  {choice.title}
                </span>
                <span dir="ltr" class="block truncate text-left text-sm text-muted-foreground">
                  <span lang={choice.detailLang}>{choice.detail}</span>
                  {#if choice.preferredPlanned}
                    <span aria-hidden="true"> · </span>
                    <span lang={choice.preferredPlanned.lang} dir={choice.preferredPlanned.direction}>
                      {choice.preferredPlanned.text}
                    </span>
                  {/if}
                </span>
              </span>
              <span
                aria-hidden="true"
                class={cn(
                  "flex size-5 shrink-0 items-center justify-center rounded-full border-2 transition-colors",
                  selected ? "border-primary" : "border-foreground/25",
                )}
              >
                {#if selected}<span class="size-2.5 rounded-full bg-primary"></span>{/if}
              </span>
            </label>
          {/each}
        </div>
        <ul class="grid gap-1.5">
          {#each planned as entry (entry.id)}
            <li>
              <button
                type="button"
                aria-disabled="true"
                class="flex min-h-14 w-full flex-col justify-center rounded-lg px-4 py-2.5 text-left opacity-55 transition-colors focus-visible:bg-accent"
                onclick={() => showNotice(entry)}
              >
                <span lang="en" class="block w-full truncate text-base font-medium text-foreground">{entry.title}</span>
                <span lang={entry.id} dir="ltr" class="block w-full truncate text-left text-sm text-muted-foreground">
                  {entry.detail}
                </span>
              </button>
            </li>
          {/each}
        </ul>
      </div>
    </div>
  </div>

  <!-- The step ends at the navigation's separator, so the toast sits just above it. -->
  {#if notice}
    <div class="pointer-events-none absolute inset-x-0 bottom-0 z-70 flex justify-center px-4 pb-3">
      <div class="pointer-events-auto">
        <ActionToast
          anchored
          wrap
          message={notice.notice}
          messageLang={notice.id}
          messageDir={notice.direction}
          onDismiss={dismissNotice}
        />
      </div>
    </div>
  {/if}
</section>
