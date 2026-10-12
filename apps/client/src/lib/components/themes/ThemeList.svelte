<script lang="ts">
  import { onDestroy } from "svelte";
  import Upload from "@lucide/svelte/icons/upload";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import X from "@lucide/svelte/icons/x";
  import Sun from "@lucide/svelte/icons/sun";
  import Moon from "@lucide/svelte/icons/moon";
  import { themeDisplayName } from "$lib/i18n/theme-labels";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getTheme } from "$lib/stores/theme.svelte";
  import { getThemeEditor } from "$lib/stores/theme-editor.svelte";
  import type { ThemeId } from "$lib/themes";
  import ThemeRow from "./ThemeRow.svelte";
  import Select from "$lib/components/ui/Select.svelte";
  import ShortcutDescription from "$lib/components/ui/ShortcutDescription.svelte";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import ActionToast from "$lib/components/ui/ActionToast.svelte";
  import { BUILD_PLATFORM_PROFILE } from "$lib/platform";
  import {
    pickThemeJsonFile,
    saveThemeJsonFile,
  } from "$lib/themes/json-file";

  const themeStore = getTheme();
  const themeEditor = getThemeEditor();
  const { t } = getLocalization();
  const desktopShell = BUILD_PLATFORM_PROFILE.shell === "desktop";

  let pendingDelete = $state<ThemeId | undefined>(undefined);
  let importOpen = $state(false);
  let importDraft = $state("");
  let importErrors = $state<string[]>([]);
  let exportingThemeId = $state<ThemeId | undefined>(undefined);
  let toast = $state<{
    message: string;
    variant: "default" | "success" | "error";
    pending: boolean;
  } | undefined>(undefined);
  let toastTimer: ReturnType<typeof setTimeout> | undefined;
  const quickToggleShortcuts = ["Mod + Shift + L"] as const;
  const themePickerShortcuts = ["Mod + Shift + T"] as const;

  const orderedThemes = $derived.by(() => {
    const all = Object.values(themeStore.registry);
    return [
      ...all.filter((t) => themeStore.isBuiltin(t.id)),
      ...all.filter((t) => !themeStore.isBuiltin(t.id)),
    ];
  });
  const themeOptions = $derived(
    orderedThemes.map((theme) => ({
      value: theme.id,
      label: themeDisplayName(theme, t),
    })),
  );

  function clearToastTimer(): void {
    if (toastTimer) clearTimeout(toastTimer);
    toastTimer = undefined;
  }

  function dismissToast(): void {
    clearToastTimer();
    toast = undefined;
  }

  function showToast(
    message: string,
    variant: "default" | "success" | "error" = "default",
    pending = false,
  ): void {
    clearToastTimer();
    toast = { message, variant, pending };
    if (pending) return;
    toastTimer = setTimeout(() => {
      toast = undefined;
      toastTimer = undefined;
    }, variant === "error" ? 8_000 : 3_000);
  }

  onDestroy(clearToastTimer);

  function handleApply(id: ThemeId) {
    themeStore.setTheme(id);
  }

  function handleQuickToggleLight(id: string) {
    themeStore.setQuickToggleTheme("light", id);
  }

  function handleQuickToggleDark(id: string) {
    themeStore.setQuickToggleTheme("dark", id);
  }

  // Duplicated themes open with the edited theme as the active one so the
  // floating panel reflects changes live. Editing an existing user theme also
  // activates it for the same reason and captures a JSON snapshot so cancel can
  // roll back the edits. Built-ins carry no snapshot: they cannot be mutated
  // while the editor is open, so there is nothing to restore.
  async function handleDuplicate(id: ThemeId) {
    const previousActiveId = themeStore.id;
    const newId = await themeStore.duplicateTheme(id);
    if (!newId) return;
    themeStore.setTheme(newId);
    themeEditor.open(newId, { createdFresh: true, previousActiveId });
  }

  function handleOpen(id: ThemeId) {
    const previousActiveId = themeStore.id;
    const isBuiltin = themeStore.isBuiltin(id);
    const snapshot = isBuiltin ? undefined : themeStore.exportTheme(id);
    if (themeStore.id !== id) themeStore.setTheme(id);
    themeEditor.open(id, { snapshot, previousActiveId });
  }

  function handleImportToggle() {
    importOpen = !importOpen;
    importDraft = "";
    importErrors = [];
  }

  async function handlePasteFromClipboard() {
    try {
      const text = await navigator.clipboard.readText();
      importDraft = text;
    } catch (err) {
      console.error("clipboard read failed", err);
      importErrors = [t("settings.theme.clipboardFailed")];
    }
  }

  async function handleImportFromFile() {
    try {
      const text = await pickThemeJsonFile();
      if (text === null) return;
      const result = await themeStore.importTheme(text);
      if (!result.ok) {
        importErrors = result.errors;
        importDraft = text;
        return;
      }
      importErrors = [];
      importDraft = "";
      importOpen = false;
      showToast(t("settings.theme.imported"), "success");
    } catch (err) {
      console.error("import from file failed", err);
      importErrors = [
        err instanceof Error ? err.message : t("settings.theme.fileReadFailed"),
      ];
    }
  }

  async function handleImport() {
    if (importDraft.trim().length === 0) {
      importErrors = [t("settings.theme.pasteFirst")];
      return;
    }
    const result = await themeStore.importTheme(importDraft);
    if (!result.ok) {
      importErrors = result.errors;
      return;
    }
    importErrors = [];
    importDraft = "";
    importOpen = false;
    showToast(t("settings.theme.imported"), "success");
  }

  async function handleExport(id: ThemeId) {
    if (exportingThemeId) return;
    const contents = themeStore.exportTheme(id);
    if (!contents) {
      showToast(t("settings.theme.exportFailed"), "error");
      return;
    }
    exportingThemeId = id;
    if (!desktopShell) {
      showToast(t("settings.theme.exporting"), "default", true);
    }
    try {
      const outcome = await saveThemeJsonFile(`${id}.json`, contents);
      if (!outcome.saved) {
        if (toast?.pending) dismissToast();
        return;
      }
      showToast(
        outcome.destination === "downloads" && outcome.fileName
          ? t("settings.theme.exportedToDownloads", outcome.fileName)
          : t("settings.theme.exported"),
        "success",
      );
    } catch (err) {
      console.error("theme export failed", err);
      showToast(t("settings.theme.exportFailed"), "error");
    } finally {
      exportingThemeId = undefined;
    }
  }

  function handleDelete(id: ThemeId) {
    pendingDelete = id;
  }

  function confirmDelete() {
    if (!pendingDelete) return;
    void themeStore.deleteTheme(pendingDelete);
    pendingDelete = undefined;
  }

  function cancelDelete() {
    pendingDelete = undefined;
  }
</script>

<div class="flex flex-col gap-4">
  <header class="flex items-start justify-between gap-3 px-1 max-[520px]:flex-col">
    <div class="min-w-0 flex-1">
      <h2 class="text-[0.866667rem] font-semibold text-foreground">{t("settings.theme.themesHeading")}</h2>
    </div>
  </header>

  {#if desktopShell}
    <section
      class="flex items-center justify-between gap-4 px-1 py-1 max-[640px]:flex-col max-[640px]:items-stretch max-[640px]:gap-2"
    >
      <div class="min-w-0 flex-1">
        <h3 class="text-[0.866667rem] font-normal text-foreground">{t("settings.theme.quickToggle")}</h3>
        <ShortcutDescription shortcuts={quickToggleShortcuts} />
      </div>
      <div
        class="flex flex-wrap items-center justify-end gap-x-3 gap-y-2 max-[640px]:justify-start"
      >
        <div class="flex items-center gap-1.5">
          <Sun
            size={13}
            strokeWidth={1.75}
            class="shrink-0 text-muted-foreground"
            aria-hidden="true"
          />
          <Select
            ariaLabel={t("settings.theme.lightQuickToggle")}
            value={themeStore.quickToggleLightId}
            options={themeOptions}
            onChange={handleQuickToggleLight}
            class="w-36"
          />
        </div>
        <div class="flex items-center gap-1.5">
          <Moon
            size={13}
            strokeWidth={1.75}
            class="shrink-0 text-muted-foreground"
            aria-hidden="true"
          />
          <Select
            ariaLabel={t("settings.theme.darkQuickToggle")}
            value={themeStore.quickToggleDarkId}
            options={themeOptions}
            onChange={handleQuickToggleDark}
            class="w-36"
          />
        </div>
      </div>
    </section>
  {/if}

  <section class="flex flex-col gap-3">
    <div class="px-1">
      <h3 class="text-[0.866667rem] font-normal text-foreground">{t("settings.theme.allThemes")}</h3>
      {#if desktopShell}
        <ShortcutDescription shortcuts={themePickerShortcuts} />
      {/if}
    </div>

    <div class="flex flex-col">
      {#each orderedThemes as theme (theme.id)}
        <ThemeRow
          {theme}
          isActive={theme.id === themeStore.id}
          isBuiltin={themeStore.isBuiltin(theme.id)}
          onApply={() => handleApply(theme.id)}
          onOpen={() => handleOpen(theme.id)}
          onDuplicate={() => handleDuplicate(theme.id)}
          onExport={() => handleExport(theme.id)}
          onDelete={() => handleDelete(theme.id)}
          exporting={exportingThemeId === theme.id}
          exportDisabled={exportingThemeId !== undefined && exportingThemeId !== theme.id}
          mobileLayout={!desktopShell}
        />
      {/each}
      {#if importOpen}
        <div class="px-1 py-1">
          <div class="flex flex-col gap-2">
            <div class="flex items-center justify-between gap-2">
              <span class="text-[0.8rem] font-medium text-foreground">
                {t("settings.theme.pasteJson")}
              </span>
              <button
                type="button"
                onclick={handleImportToggle}
                aria-label={t("settings.theme.closeImport")}
                data-app-tooltip-disabled="true"
                class={desktopShell
                  ? "flex h-6 w-6 items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground"
                  : "flex size-12 items-center justify-center rounded-xl text-muted-foreground active:bg-accent active:text-foreground"}
              >
                <X size={13} strokeWidth={2} />
              </button>
            </div>
            <textarea
              bind:value={importDraft}
              placeholder={'{\n  "id": "midnight",\n  "displayName": "Midnight",\n  ...\n}'}
              rows={8}
              spellcheck={false}
              class="field w-full resize-y p-2 text-[0.733333rem] text-foreground"
            ></textarea>
            {#if importErrors.length > 0}
              <ul
                class="flex flex-col gap-0.5 rounded-md border border-destructive/40 bg-destructive/10 p-2 text-[0.733333rem] text-destructive"
              >
                {#each importErrors as err}
                  <li>{err}</li>
                {/each}
              </ul>
            {/if}
            <div class="flex items-center justify-between gap-2 max-[520px]:flex-col max-[520px]:items-stretch">
              <div class="flex items-center gap-1.5 max-[520px]:flex-wrap">
                <button
                  type="button"
                  onclick={handlePasteFromClipboard}
                  class={desktopShell
                    ? "rounded-md border border-border bg-card px-2.5 py-1 text-[0.733333rem] text-foreground transition-colors hover:bg-accent dark:bg-transparent"
                    : "min-h-12 rounded-xl border border-border bg-card px-3 text-sm text-foreground active:bg-accent dark:bg-transparent"}
                >
                  {t("settings.theme.pasteClipboard")}
                </button>
                <button
                  type="button"
                  onclick={handleImportFromFile}
                  class={desktopShell
                    ? "flex items-center gap-1.5 rounded-md border border-border bg-card px-2.5 py-1 text-[0.733333rem] text-foreground transition-colors hover:bg-accent dark:bg-transparent"
                    : "flex min-h-12 items-center gap-1.5 rounded-xl border border-border bg-card px-3 text-sm text-foreground active:bg-accent dark:bg-transparent"}
                >
                  <FolderOpen size={11} strokeWidth={2.25} />
                  <span>{t("settings.theme.openFile")}</span>
                </button>
              </div>
              <button
                type="button"
                onclick={handleImport}
                class={desktopShell
                  ? "rounded-md border border-border bg-primary px-3 py-1 text-[0.8rem] font-medium text-primary-foreground transition-colors hover:bg-primary/90 max-[520px]:self-end"
                  : "min-h-12 rounded-xl border border-primary bg-primary px-4 text-sm font-medium text-primary-foreground active:bg-primary/90 max-[520px]:self-end"}
              >
                {t("settings.theme.import")}
              </button>
            </div>
          </div>
        </div>
      {:else}
        <button
          type="button"
          onclick={handleImportToggle}
          class={desktopShell
            ? "flex w-full min-w-0 items-center gap-2 rounded-md px-1 py-1 text-[0.866667rem] text-foreground transition-colors hover:bg-accent/25 focus:outline-none focus-visible:ring-1 focus-visible:ring-ring"
            : "flex min-h-12 w-full min-w-0 items-center gap-2 rounded-xl px-3 text-sm text-foreground active:bg-accent/40 focus:outline-none focus-visible:ring-1 focus-visible:ring-ring"}
        >
          <Upload
            size={13}
            strokeWidth={1.75}
            class="shrink-0 text-muted-foreground"
          />
          <span>{t("settings.theme.importTheme")}</span>
        </button>
      {/if}
    </div>
  </section>

  {#if toast}
    <ActionToast
      message={toast.message}
      variant={toast.variant}
      controlsVisible={!toast.pending}
      dismissLabel={t("settings.theme.dismissTransferNotification")}
      onDismiss={dismissToast}
    />
  {/if}
</div>

{#if pendingDelete}
  {@const target = themeStore.registry[pendingDelete]}
  <ConfirmDialog
    title={t(
      "settings.theme.deleteTitle",
      target ? themeDisplayName(target, t) : t("settings.theme.thisTheme"),
    )}
    message={t("settings.theme.cannotBeUndone")}
    confirmLabel={t("settings.distractions.shared.deleteAction")}
    cancelLabel={t("common.cancel")}
    onConfirm={confirmDelete}
    onCancel={cancelDelete}
  />
{/if}
