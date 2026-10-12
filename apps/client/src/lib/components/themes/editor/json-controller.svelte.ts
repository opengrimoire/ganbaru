import { onDestroy, untrack } from "svelte";
import { getLocalization } from "$lib/i18n/translator.svelte";
import { getTheme } from "$lib/stores/theme.svelte";
import {
  THEME_JSON_FILE_SAVE_AVAILABLE,
  saveThemeJsonFile,
} from "$lib/themes/json-file";

type ThemeStore = ReturnType<typeof getTheme>;
type Translator = ReturnType<typeof getLocalization>["t"];

export interface ThemeJsonControllerContext {
  store: ThemeStore;
  themeId: () => string;
  translate: Translator;
  reportError: (message: string, error: unknown) => void;
}

export interface ThemeJsonNotice {
  message: string;
  variant: "default" | "success" | "error";
}

/** Own the editable JSON draft and asynchronous import/export feedback. */
export class ThemeJsonController {
  readonly fileSaveAvailable = THEME_JSON_FILE_SAVE_AVAILABLE;
  draft = $state("");
  dirty = $state(false);
  errors = $state<string[]>([]);
  saving = $state(false);
  notice = $state<ThemeJsonNotice | undefined>(undefined);
  #noticeTimer: ReturnType<typeof setTimeout> | undefined;

  constructor(private readonly context: ThemeJsonControllerContext) {
    this.draft = untrack(() => context.store.exportTheme(context.themeId()) ?? "");
    $effect(() => {
      const next = context.store.exportTheme(context.themeId()) ?? "";
      if (!this.dirty) this.draft = next;
    });
    onDestroy(() => {
      if (this.#noticeTimer) clearTimeout(this.#noticeTimer);
    });
  }

  #flash(
    message: string,
    variant: ThemeJsonNotice["variant"] = "default",
  ): void {
    this.notice = { message, variant };
    if (this.#noticeTimer) clearTimeout(this.#noticeTimer);
    this.#noticeTimer = setTimeout(() => {
      this.notice = undefined;
      this.#noticeTimer = undefined;
    }, variant === "error" ? 8_000 : 3_000);
  }

  dismissNotice = (): void => {
    if (this.#noticeTimer) clearTimeout(this.#noticeTimer);
    this.#noticeTimer = undefined;
    this.notice = undefined;
  };

  copy = async (): Promise<void> => {
    try {
      await navigator.clipboard.writeText(this.draft);
      this.#flash(this.context.translate("settings.theme.editor.jsonCopied"));
    } catch (error) {
      this.context.reportError("clipboard write failed", error);
      this.#flash(
        this.context.translate("settings.theme.editor.jsonCopyFailed"),
        "error",
      );
    }
  };

  save = async (): Promise<void> => {
    if (!this.fileSaveAvailable || this.saving) return;
    this.saving = true;
    this.dismissNotice();
    try {
      const outcome = await saveThemeJsonFile(
        `${this.context.themeId()}.json`,
        this.draft,
      );
      if (!outcome.saved) return;
      this.#flash(
        outcome.destination === "downloads" && outcome.fileName
          ? this.context.translate(
              "settings.theme.editor.jsonSavedToDownloads",
              outcome.fileName,
            )
          : this.context.translate("settings.theme.editor.jsonSaved"),
        "success",
      );
    } catch (error) {
      this.context.reportError("save dialog failed", error);
      this.#flash(
        this.context.translate("settings.theme.editor.jsonSaveFailed"),
        "error",
      );
    } finally {
      this.saving = false;
    }
  };

  apply = async (): Promise<void> => {
    const result = this.context.store.replaceThemeDraft(
      this.context.themeId(),
      this.draft,
    );
    if (!result.ok) {
      this.errors = result.errors;
      return;
    }
    this.errors = [];
    this.dirty = false;
    this.#flash(
      this.context.translate("settings.theme.editor.jsonUpdated"),
      "success",
    );
  };

  reset = (): void => {
    this.draft = this.context.store.exportTheme(this.context.themeId()) ?? "";
    this.dirty = false;
    this.errors = [];
  };

  input = (event: Event): void => {
    this.draft = (event.currentTarget as HTMLTextAreaElement).value;
    this.dirty = true;
    this.errors = [];
  };
}
