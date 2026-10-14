<script lang="ts">
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import {
    DEFAULT_SIDEBAR_SORT_ORDER,
    isSidebarSortOrder,
    SIDEBAR_SORT_ORDER_LABEL_KEYS,
    SIDEBAR_SORT_ORDERS,
  } from "$lib/utils/sidebar-order";
  import Select from "$lib/components/ui/Select.svelte";
  import SwitchField from "$lib/components/ui/SwitchField.svelte";
  import ProjectsInactiveList from "./ProjectsInactiveList.svelte";

  const preferences = getPreferences();
  const { t } = getLocalization();

  const sidebarOrderOptions = $derived(
    SIDEBAR_SORT_ORDERS.map((order) => ({ value: order, label: t(SIDEBAR_SORT_ORDER_LABEL_KEYS[order]) })),
  );

  function handleSidebarOrderChange(value: string): void {
    if (isSidebarSortOrder(value)) preferences.setProjectsSidebarOrder(value);
  }
</script>

<div class="flex flex-col gap-6">
  <section class="flex flex-col gap-4">
    <h2 class="px-1 text-[0.866667rem] font-semibold text-foreground">{t("settings.projectsGeneral.heading")}</h2>

    <div class="flex flex-col gap-3">
      <Select
        label={t("settings.projectsGeneral.sidebarOrder")}
        description={t("settings.projectsGeneral.sidebarOrderDescription")}
        value={preferences.projectsSidebarOrder}
        options={sidebarOrderOptions}
        onChange={handleSidebarOrderChange}
        canReset={preferences.projectsSidebarOrder !== DEFAULT_SIDEBAR_SORT_ORDER}
        onReset={() => preferences.resetProjectsSidebarOrder()}
      />
      <SwitchField
        label={t("settings.projectsGeneral.showPropertyIcons")}
        description={t("settings.projectsGeneral.showPropertyIconsDescription")}
        checked={preferences.projectsShowPropertyIcons}
        onChange={preferences.setProjectsShowPropertyIcons}
      />
    </div>
  </section>

  <ProjectsInactiveList />
</div>
