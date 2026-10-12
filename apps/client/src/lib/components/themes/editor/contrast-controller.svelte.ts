import { contrastRatio, pickReadableForeground } from "$lib/color/math";
import { getLocalization } from "$lib/i18n/translator.svelte";
import {
  resolveAppTokens,
  resolveCalendarTokens,
  type ThemeSources,
  type UserTheme,
} from "$lib/themes";
import {
  SOURCE_GROUPS,
  type GroupContrastRow,
  type SourceGroup,
} from "./model";

type Translator = ReturnType<typeof getLocalization>["t"];
export type PairContrast = { ratio: number; passes: boolean; target: number };
type LocatedPair = { row: GroupContrastRow; group: SourceGroup };

export interface ThemeContrastControllerContext {
  theme: () => UserTheme;
  readOnly: () => boolean;
  translate: Translator;
  setSource: (key: keyof ThemeSources, hex: string) => void;
  setAppToken: (key: string, hex: string) => void;
  setCalendarToken: (key: string, hex: string) => void;
  expandGroup: (groupId: SourceGroup["id"]) => void;
}

const AA_BODY_TARGET = 4.5;

const ALL_PAIRS: LocatedPair[] = SOURCE_GROUPS.flatMap((group) =>
  group.rows
    .filter((row): row is GroupContrastRow => row.kind === "pair" || row.kind === "source-pair")
    .map((row) => ({ row, group })),
);

/** Own contrast evaluation, navigation, and repair intents. */
export class ThemeContrastController {
  #nextPairCursor = 0;

  constructor(private readonly context: ThemeContrastControllerContext) {}

  #target(row: GroupContrastRow): number {
    return row.target ?? AA_BODY_TARGET;
  }

  #effectiveColor(key: string, scope: "app" | "cal"): string {
    const theme = this.context.theme();
    return scope === "app" ? resolveAppTokens(theme)[key] : resolveCalendarTokens(theme)[key];
  }

  pairContrast = (row: GroupContrastRow): PairContrast => {
    const ratio = contrastRatio(
      this.#effectiveColor(row.fg, row.scope),
      this.#effectiveColor(row.bg, row.scope),
    );
    const target = this.#target(row);
    return { ratio, passes: ratio >= target, target };
  };

  pairKey = (row: GroupContrastRow): string => `${row.scope}:${row.bg}:${row.fg}`;

  contrastTitle = (contrast: PairContrast): string => {
    const suffix = contrast.target >= AA_BODY_TARGET
      ? this.context.translate("settings.theme.editor.contrastTargetAaBody")
      : this.context.translate("settings.theme.editor.contrastTargetAaLargeUi");
    if (this.context.readOnly()) {
      return this.context.translate(
        "settings.theme.editor.contrastTitle",
        contrast.ratio.toFixed(2),
        String(contrast.target),
        suffix,
      );
    }
    return this.context.translate(
      "settings.theme.editor.contrastTitleEditable",
      contrast.ratio.toFixed(2),
      String(contrast.target),
      suffix,
    );
  };

  autoFix = (row: GroupContrastRow): void => {
    if (this.context.readOnly()) return;
    const theme = this.context.theme();
    const app = resolveAppTokens(theme);
    const next = pickReadableForeground(this.#effectiveColor(row.bg, row.scope), {
      ink: app["--foreground"],
      canvas: app["--background"],
      target: this.#target(row),
    });
    if (row.kind === "source-pair") {
      this.context.setSource(row.fgSource, next);
    } else if (row.scope === "app") {
      this.context.setAppToken(row.fg, next);
    } else {
      this.context.setCalendarToken(row.fg, next);
    }
  };

  get failingPairs(): LocatedPair[] {
    return ALL_PAIRS.filter(({ row }) => !this.pairContrast(row).passes);
  }

  jumpToNext = (): void => {
    const pairs = this.failingPairs;
    if (pairs.length === 0) return;
    const index = this.#nextPairCursor % pairs.length;
    const target = pairs[index];
    this.#nextPairCursor = index + 1;
    this.context.expandGroup(target.group.id);
    queueMicrotask(() => {
      document.querySelector<HTMLElement>(`[data-pair-key="${this.pairKey(target.row)}"]`)
        ?.scrollIntoView({ behavior: "smooth", block: "center" });
    });
  };

  fixAll = (): void => {
    if (this.context.readOnly()) return;
    for (const { row } of this.failingPairs) this.autoFix(row);
    this.#nextPairCursor = 0;
  };
}
