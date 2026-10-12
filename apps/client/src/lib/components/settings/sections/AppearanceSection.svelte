<script lang="ts">
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { APP_ZOOM_LEVELS, getZoom } from "$lib/stores/zoom.svelte";
  import {
    DEFAULT_FONT_SCALE,
    DEFAULT_FONT_FAMILY_ID,
    FONT_SCALE_LEVELS,
    isLanguagePreference,
  } from "$lib/stores/preference-options";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import Select from "$lib/components/ui/Select.svelte";
  import ThemeList from "$lib/components/themes/ThemeList.svelte";
  import { BUILD_PLATFORM_PROFILE } from "$lib/platform";

  const preferences = getPreferences();
  const zoom = getZoom();
  const { t } = getLocalization();
  const mobileShell = BUILD_PLATFORM_PROFILE.shell === "mobile";

  type SelectOption = { value: string; label: string; style?: string };

  function percentString(percent: number): string {
    return String(Math.round(percent));
  }

  function handleAppZoomChange(value: string) {
    const percent = Number(value);
    if (!Number.isFinite(percent)) return;
    zoom.setLevel(percent / 100);
  }

  function handleFontScaleChange(value: string) {
    const percent = Number(value);
    if (!Number.isFinite(percent)) return;
    preferences.setFontScale(percent / 100);
  }

  const appZoomOptions: readonly SelectOption[] = APP_ZOOM_LEVELS.map((level) => {
    const percent = Math.round(level * 100);
    return { value: percentString(percent), label: `${percent}%` };
  });

  const languageOptions = $derived<SelectOption[]>(
    preferences.languagePreferences.map((language) => {
      if (language === "system") {
        return { value: language, label: t("language.systemOption") };
      }
      if (language === "en") {
        return { value: language, label: t("language.englishOption") };
      }
      return { value: language, label: t("language.spanishOption") };
    }),
  );

  const baseFontScaleOptions: readonly SelectOption[] = FONT_SCALE_LEVELS.map((level) => {
    const percent = Math.round(level * 100);
    return { value: percentString(percent), label: `${percent}%` };
  });

  const fontFamilyOptions = $derived(
    preferences.fontFamilies.map((f) => ({
      value: f.id,
      label:
        f.id === DEFAULT_FONT_FAMILY_ID
          ? `${f.displayName} (${t("settings.appearance.recommended")})`
          : f.displayName,
      style: `font-family: ${f.cssStack};`,
    })),
  );

  const fontScaleValue = $derived(percentString(preferences.fontScale * 100));
  const fontScaleOptions = $derived.by(() => {
    if (baseFontScaleOptions.some((option) => option.value === fontScaleValue)) {
      return baseFontScaleOptions;
    }
    return [
      ...baseFontScaleOptions,
      { value: fontScaleValue, label: `${fontScaleValue}%` },
    ].sort((a, b) => Number(a.value) - Number(b.value));
  });
</script>

<div class="flex flex-col gap-6">
  <ThemeList />

  <div class="h-px shrink-0 scale-y-50 bg-border" aria-hidden="true"></div>

  <section class="flex flex-col gap-4">
    <h2 class="px-1 text-[0.866667rem] font-semibold text-foreground">{t("settings.appearance.zoomHeading")}</h2>
    <div class="flex flex-col gap-3">
      <Select
        label={t("settings.appearance.appZoom")}
        descriptionShortcuts={mobileShell ? [] : ["Mod + +", "Mod + -", "Mod + 0"]}
        value={percentString(zoom.percent)}
        options={appZoomOptions}
        onChange={handleAppZoomChange}
        canReset={!zoom.isDefault}
        onReset={() => zoom.reset()}
      />
    </div>
  </section>

  <div class="h-px shrink-0 scale-y-50 bg-border" aria-hidden="true"></div>

  <section class="flex flex-col gap-4">
    <h2 class="px-1 text-[0.866667rem] font-semibold text-foreground">{t("settings.appearance.textHeading")}</h2>
    <div class="flex flex-col gap-3">
      <Select
        label={t("settings.appearance.fontFamily")}
        description={t("settings.appearance.fontFamilyDescription")}
        value={preferences.fontFamilyId}
        options={fontFamilyOptions}
        onChange={(id) => preferences.setFontFamily(id)}
        canReset={preferences.fontFamilyId !== DEFAULT_FONT_FAMILY_ID}
        onReset={() => preferences.resetFontFamily()}
      />
      <Select
        label={t("settings.appearance.textSize")}
        description={t("settings.appearance.textSizeDescription")}
        value={fontScaleValue}
        options={fontScaleOptions}
        onChange={handleFontScaleChange}
        canReset={preferences.fontScale !== DEFAULT_FONT_SCALE}
        onReset={() => preferences.resetFontScale()}
      />
    </div>
  </section>

  <div class="h-px shrink-0 scale-y-50 bg-border" aria-hidden="true"></div>

  <section class="flex flex-col gap-4">
    <h2 class="px-1 text-[0.866667rem] font-semibold text-foreground">{t("settings.appearance.languageHeading")}</h2>
    <div class="flex flex-col gap-3">
      <Select
        label={t("language.preferenceLabel")}
        description={t("language.preferenceDescription")}
        value={preferences.languagePreference}
        options={languageOptions}
        onChange={(value) => {
          if (isLanguagePreference(value)) void preferences.setLanguagePreference(value);
        }}
        canReset={preferences.languagePreference !== "system"}
        onReset={() => void preferences.setLanguagePreference("system")}
      />
    </div>
  </section>
</div>
