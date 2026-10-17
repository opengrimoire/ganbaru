import { getTheme } from "$lib/stores/theme.svelte";
import type {
  CalendarColorDefaultMode,
  ThemeSources,
  UserTheme,
} from "$lib/themes";
import { canResetTokenToSeed } from "$lib/themes/operations";

type ThemeStore = ReturnType<typeof getTheme>;
type TokenScope = "app" | "calendar";

export interface ThemeEditorActionContext {
  store: ThemeStore;
  themeId: () => string;
  readOnly: () => boolean;
  userTheme: () => UserTheme | undefined;
}

/** Typed intent adapter over the single authoritative theme store. */
export function createThemeEditorActions(context: ThemeEditorActionContext) {
  function writable(): boolean {
    return !context.readOnly();
  }

  function setToken(scope: TokenScope, key: string, hex: string): void {
    if (!writable()) return;
    void context.store.setTokenValue(context.themeId(), scope, key, hex);
  }

  function isolate(scope: TokenScope, key: string): void {
    if (!writable()) return;
    void context.store.isolateToken(context.themeId(), scope, key);
  }

  function relink(scope: TokenScope, key: string): void {
    if (!writable()) return;
    void context.store.relinkToken(context.themeId(), scope, key);
  }

  function reset(scope: "source" | TokenScope, key: string): void {
    if (!writable()) return;
    void context.store.resetTokenToSeed(context.themeId(), scope, key);
  }

  function canReset(scope: "source" | TokenScope, key: string): boolean {
    const theme = context.userTheme();
    return theme ? canResetTokenToSeed(theme, scope, key) : false;
  }

  return {
    rename(next: string): void {
      if (!writable()) return;
      void context.store.renameTheme(context.themeId(), next);
    },
    setPaletteSlot(index: number, hex: string): void {
      if (!writable()) return;
      void context.store.setPaletteSlot(context.themeId(), index, hex);
    },
    setAppToken(key: string, hex: string): void { setToken("app", key, hex); },
    setCalendarToken(key: string, hex: string): void { setToken("calendar", key, hex); },
    setSource(key: keyof ThemeSources, hex: string): void {
      if (!writable()) return;
      void context.store.updateSourceValue(context.themeId(), key, hex);
    },
    applyCalendarDefault(mode: CalendarColorDefaultMode, custom?: string): void {
      if (!writable() || !context.userTheme()) return;
      void context.store.applyCalendarDefault(context.themeId(), mode, custom);
    },
    resetCalendarDefault(): void {
      if (!writable() || !context.userTheme()) return;
      void context.store.resetCalendarDefaultToSeed(context.themeId());
    },
    canResetCalendarDefault(): boolean {
      return writable() && context.store.canResetCalendarDefault(context.themeId());
    },
    isolateAppToken(key: string): void { isolate("app", key); },
    isolateCalendarToken(key: string): void { isolate("calendar", key); },
    relinkAppToken(key: string): void { relink("app", key); },
    relinkCalendarToken(key: string): void { relink("calendar", key); },
    canResetSource(key: keyof ThemeSources): boolean { return canReset("source", key); },
    canResetAppToken(key: string): boolean { return canReset("app", key); },
    canResetCalendarToken(key: string): boolean { return canReset("calendar", key); },
    resetSource(key: keyof ThemeSources): void { reset("source", key); },
    resetAppToken(key: string): void { reset("app", key); },
    resetCalendarToken(key: string): void { reset("calendar", key); },
    rebake(): void {
      if (!context.userTheme()) return;
      void context.store.rebakeTheme(context.themeId());
    },
    dismissRebake(): void {
      if (!context.userTheme()) return;
      void context.store.dismissUpgrade(context.themeId());
    },
  };
}
