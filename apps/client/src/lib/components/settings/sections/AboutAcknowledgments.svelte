<script lang="ts">
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { getLocalization } from "$lib/i18n/translator.svelte";

  let { showHeading = true }: { showHeading?: boolean } = $props();

  interface SoundCredit {
    appUse: () => string;
    filename: string;
    sourceSound: string;
    sourceUrl: string;
    author: string;
    authorUrl: string;
    license: string;
  }

  interface IconCredit {
    appUse: () => string;
    source: string;
    sourceUrl: string;
    author: () => string;
    authorUrl: string;
    license: () => string;
  }

  const SOUND_FILE_BASE_URL = "https://github.com/opengrimoire/ganbaru-ai/blob/main/apps/client/static/sfx/";
  const FREESOUND_URL = "https://freesound.org/";
  const linkButtonClass =
    "inline-flex min-w-0 max-w-full items-center gap-1 text-left text-primary underline-offset-2 transition-colors hover:underline focus:outline-none focus-visible:ring-1 focus-visible:ring-ring";
  const { t } = getLocalization();

  const soundCredits: readonly SoundCredit[] = [
    {
      appUse: () => t("settings.about.soundUse.eventNotification"),
      filename: "event-notification.wav",
      sourceSound: "Short Success Sound Glockenspiel Treasure Video Game.mp3",
      sourceUrl: "https://freesound.org/people/FunWithSound/sounds/456965/",
      author: "FunWithSound",
      authorUrl: "https://freesound.org/people/FunWithSound/",
      license: "Creative Commons 0",
    },
    {
      appUse: () => t("settings.about.soundUse.idleAlert"),
      filename: "idle-alert.wav",
      sourceSound: "Soft Short App Melody",
      sourceUrl: "https://freesound.org/people/CogFireStudios/sounds/619837/",
      author: "CogFireStudios",
      authorUrl: "https://freesound.org/people/CogFireStudios/",
      license: "Creative Commons 0",
    },
    {
      appUse: () => t("settings.about.soundUse.focusFailureAfterLongIdle"),
      filename: "focus-session-failed-long-idle.wav",
      sourceSound: "Game Over 8 (One wrong step) .aif",
      sourceUrl: "https://freesound.org/people/SilverIllusionist/sounds/562103/",
      author: "SilverIllusionist",
      authorUrl: "https://freesound.org/people/SilverIllusionist/",
      license: "Attribution 4.0",
    },
    {
      appUse: () => t("settings.about.soundUse.oneMinuteBeforeBreak"),
      filename: "focus-ending-warning.wav",
      sourceSound: "sfx_rpg_ui_focus",
      sourceUrl: "https://freesound.org/people/MATUSTRM/sounds/848972/",
      author: "MATUSTRM",
      authorUrl: "https://freesound.org/people/MATUSTRM/",
      license: "Creative Commons 0",
    },
    {
      appUse: () => t("settings.about.soundUse.breakStart"),
      filename: "break-start.wav",
      sourceSound: "Success 03",
      sourceUrl: "https://freesound.org/people/rhodesmas/sounds/322930/",
      author: "rhodesmas",
      authorUrl: "https://freesound.org/people/rhodesmas/",
      license: "Attribution 4.0",
    },
    {
      appUse: () => t("settings.about.soundUse.breakFinish"),
      filename: "break-finished.wav",
      sourceSound: "Achievement Happy Beeps Jingle",
      sourceUrl: "https://freesound.org/people/CogFireStudios/sounds/619838/",
      author: "CogFireStudios",
      authorUrl: "https://freesound.org/people/CogFireStudios/",
      license: "Attribution 4.0",
    },
    {
      appUse: () => t("settings.about.soundUse.eventFinish"),
      filename: "event-finished.wav",
      sourceSound: "Reflective Guitar Chords #1",
      sourceUrl: "https://freesound.org/people/SilverIllusionist/sounds/843310/",
      author: "SilverIllusionist",
      authorUrl: "https://freesound.org/people/SilverIllusionist/",
      license: "Creative Commons 0",
    },
    {
      appUse: () => t("settings.about.soundUse.dayCompleted"),
      filename: "pomodoro-day-complete.wav",
      sourceSound: "Victory Fanfare (Light Wills Ever) no drums",
      sourceUrl: "https://freesound.org/people/SilverIllusionist/sounds/669323/",
      author: "SilverIllusionist",
      authorUrl: "https://freesound.org/people/SilverIllusionist/",
      license: "Creative Commons 0",
    },
    {
      appUse: () => t("settings.about.soundUse.workweekCompleted"),
      filename: "pomodoro-workweek-complete.wav",
      sourceSound: "Victory Fanfare (RPG or High Fantasy)",
      sourceUrl: "https://freesound.org/people/SilverIllusionist/sounds/659751/",
      author: "SilverIllusionist",
      authorUrl: "https://freesound.org/people/SilverIllusionist/",
      license: "Attribution 4.0",
    },
    {
      appUse: () => t("settings.about.soundUse.aiResponseFinished"),
      filename: "ai-response-finished.wav",
      sourceSound: "Three-Note Doorbell or Notification",
      sourceUrl: "https://freesound.org/people/eqylizer/sounds/624599/",
      author: "eqylizer",
      authorUrl: "https://freesound.org/people/eqylizer/",
      license: "Creative Commons 0",
    },
  ];

  const iconCredits: readonly IconCredit[] = [
    {
      appUse: () => t("settings.about.iconUse.interface"),
      source: "Lucide",
      sourceUrl: "https://github.com/lucide-icons/lucide",
      author: () => t("settings.about.lucideAuthor"),
      authorUrl: "https://github.com/lucide-icons/lucide/graphs/contributors",
      license: () => t("settings.about.lucideLicense"),
    },
    {
      appUse: () => t("settings.about.iconUse.chatFiles"),
      source: "vscode-icons",
      sourceUrl: "https://github.com/vscode-icons/vscode-icons",
      author: () => t("settings.about.vscodeIconsAuthor"),
      authorUrl: "https://github.com/vscode-icons/vscode-icons/graphs/contributors",
      license: () => t("settings.about.vscodeIconsLicense"),
    },
  ];

  async function openExternalUrl(url: string): Promise<void> {
    try {
      await openUrl(url);
    } catch (error: unknown) {
      console.warn("Failed to open external URL:", error);
    }
  }

  function soundFileUrl(filename: string): string {
    return `${SOUND_FILE_BASE_URL}${encodeURIComponent(filename)}`;
  }
</script>

<section class="flex flex-col gap-4">
  {#if showHeading}
    <h2 class="px-1 text-[0.866667rem] font-semibold text-foreground">{t("settings.about.acknowledgmentsHeading")}</h2>
  {/if}
  <div class="flex flex-col gap-3">
    <div class="px-1 py-1">
      <div class="text-[0.866667rem] text-foreground">{t("settings.about.soundEffects")}</div>
      <div class="mt-0.5 text-[0.8rem] leading-5 text-muted-foreground">
        {t("settings.about.soundEffectsDescriptionPrefix")}
        <button type="button" class={linkButtonClass} onclick={() => { void openExternalUrl(FREESOUND_URL); }}>
          Freesound <ExternalLink size={11} strokeWidth={2.25} aria-hidden="true" />
        </button>
        {t("settings.about.soundEffectsDescriptionSuffix")}
      </div>
    </div>

    <div class="flex min-w-0 flex-col gap-6 px-1">
      {#each soundCredits as credit}
        <article class="flex min-w-0 flex-col gap-1.5 text-[0.8rem]">
          <h3 class="wrap-break-word min-w-0 text-[0.733333rem] leading-5 text-foreground">{credit.appUse()}</h3>
          <div class="grid min-w-0 gap-1.5">
            <div class="grid min-w-0 grid-cols-[4rem_minmax(0,1fr)] items-start gap-2 max-[420px]:grid-cols-1 max-[420px]:gap-0.5">
              <span class="text-[0.733333rem] leading-5 text-muted-foreground">{t("settings.about.file")}</span>
              <button type="button" class={linkButtonClass} onclick={() => { void openExternalUrl(soundFileUrl(credit.filename)); }}><span class="wrap-break-word">{credit.filename}</span><ExternalLink size={11} strokeWidth={2.25} class="shrink-0" aria-hidden="true" /></button>
            </div>
            <div class="grid min-w-0 grid-cols-[4rem_minmax(0,1fr)] items-start gap-2 max-[420px]:grid-cols-1 max-[420px]:gap-0.5">
              <span class="text-[0.733333rem] leading-5 text-muted-foreground">{t("settings.about.source")}</span>
              <button type="button" class={linkButtonClass} onclick={() => { void openExternalUrl(credit.sourceUrl); }}><span class="wrap-break-word">{credit.sourceSound}</span><ExternalLink size={11} strokeWidth={2.25} class="shrink-0" aria-hidden="true" /></button>
            </div>
            <div class="grid min-w-0 grid-cols-[4rem_minmax(0,1fr)] items-start gap-2 max-[420px]:grid-cols-1 max-[420px]:gap-0.5">
              <span class="text-[0.733333rem] leading-5 text-muted-foreground">{t("settings.about.author")}</span>
              <button type="button" class={linkButtonClass} onclick={() => { void openExternalUrl(credit.authorUrl); }}><span class="wrap-break-word">{credit.author}</span><ExternalLink size={11} strokeWidth={2.25} class="shrink-0" aria-hidden="true" /></button>
            </div>
            <div class="grid min-w-0 grid-cols-[4rem_minmax(0,1fr)] items-start gap-2 max-[420px]:grid-cols-1 max-[420px]:gap-0.5">
              <span class="text-[0.733333rem] leading-5 text-muted-foreground">{t("settings.about.licenseHeading")}</span>
              <span class="text-[0.8rem] leading-5 text-primary">{credit.license}</span>
            </div>
          </div>
        </article>
      {/each}
    </div>
  </div>

  <div class="mt-4 flex flex-col gap-3">
    <div class="px-1 py-1">
      <div class="text-[0.866667rem] text-foreground">{t("settings.about.icons")}</div>
      <div class="mt-0.5 text-[0.8rem] leading-5 text-muted-foreground">{t("settings.about.iconsDescription")}</div>
    </div>

    <div class="flex min-w-0 flex-col gap-6 px-1">
      {#each iconCredits as credit}
        <article class="flex min-w-0 flex-col gap-1.5 text-[0.8rem]">
          <h3 class="wrap-break-word min-w-0 text-[0.733333rem] leading-5 text-foreground">{credit.appUse()}</h3>
          <div class="grid min-w-0 gap-1.5">
            <div class="grid min-w-0 grid-cols-[4rem_minmax(0,1fr)] items-start gap-2 max-[420px]:grid-cols-1 max-[420px]:gap-0.5">
              <span class="text-[0.733333rem] leading-5 text-muted-foreground">{t("settings.about.source")}</span>
              <button type="button" class={linkButtonClass} onclick={() => { void openExternalUrl(credit.sourceUrl); }}><span class="wrap-break-word">{credit.source}</span><ExternalLink size={11} strokeWidth={2.25} class="shrink-0" aria-hidden="true" /></button>
            </div>
            <div class="grid min-w-0 grid-cols-[4rem_minmax(0,1fr)] items-start gap-2 max-[420px]:grid-cols-1 max-[420px]:gap-0.5">
              <span class="text-[0.733333rem] leading-5 text-muted-foreground">{t("settings.about.author")}</span>
              <button type="button" class={linkButtonClass} onclick={() => { void openExternalUrl(credit.authorUrl); }}><span class="wrap-break-word">{credit.author()}</span><ExternalLink size={11} strokeWidth={2.25} class="shrink-0" aria-hidden="true" /></button>
            </div>
            <div class="grid min-w-0 grid-cols-[4rem_minmax(0,1fr)] items-start gap-2 max-[420px]:grid-cols-1 max-[420px]:gap-0.5">
              <span class="text-[0.733333rem] leading-5 text-muted-foreground">{t("settings.about.licenseHeading")}</span>
              <span class="text-[0.8rem] leading-5 text-primary">{credit.license()}</span>
            </div>
          </div>
        </article>
      {/each}
    </div>
  </div>
</section>
