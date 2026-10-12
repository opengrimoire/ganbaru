<script lang="ts">
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import SwitchField from "$lib/components/ui/SwitchField.svelte";

  const preferences = getPreferences();
  const { t } = getLocalization();
</script>

<div class="flex flex-col gap-6">
  <section class="flex flex-col gap-4">
    <h2 class="px-1 text-[0.866667rem] font-semibold text-foreground">{t("settings.projectsGeneral.heading")}</h2>

    <div class="flex flex-col gap-3">
      <SwitchField
        label={t("settings.projectsGeneral.showPropertyIcons")}
        description={t("settings.projectsGeneral.showPropertyIconsDescription")}
        checked={preferences.projectsShowPropertyIcons}
        onChange={preferences.setProjectsShowPropertyIcons}
      />
    </div>
  </section>
</div>
