import DiffsWorker from "@pierre/diffs/worker/worker.js?worker";
import {
  CodeView,
  parsePatchFiles,
  registerCustomCSSVariableTheme,
  type CodeViewDiffItem,
  type CodeViewOptions,
  type DiffLineAnnotation,
  type FileDiffMetadata,
  type LineAnnotation,
  type SelectedLineRange,
} from "@pierre/diffs";
import {
  getOrCreateWorkerPoolSingleton,
  terminateWorkerPoolSingleton,
} from "@pierre/diffs/worker";
import type { ChatReviewCommentRead } from "$lib/chat/contracts";

export interface ReviewDiffRenderItem {
  key: string;
  fileId: string;
  fileName: string;
  previousFileName: string | null;
  patch: string;
  pageIndex: number;
  comments: ChatReviewCommentRead[];
  patchVersion: number;
  version: number;
}

export interface ReviewLineSelection {
  fileId: string;
  range: SelectedLineRange;
}

export interface ReviewAnnotationLabels {
  attach: string;
  resolve: string;
  reopen: string;
  outdated: string;
}

export interface ReviewDiffRuntimeOptions {
  diffStyle: "unified" | "split";
  wrap: boolean;
  labels: ReviewAnnotationLabels;
  onSelection: (selection: ReviewLineSelection | null) => void;
  onAttachComment: (comment: ChatReviewCommentRead) => void;
  onResolveComment: (comment: ChatReviewCommentRead, resolved: boolean) => void;
  onActiveFile: (fileId: string) => void;
  onWorkerError: (reason: unknown) => void;
}

interface ReviewAnnotationData {
  comments: ChatReviewCommentRead[];
}

const WORKER_IDLE_GRACE_MS = 30_000;
const CHAT_REVIEW_SYNTAX_THEME = "ganbaru-chat-syntax";
registerCustomCSSVariableTheme(CHAT_REVIEW_SYNTAX_THEME, {
  foreground: "var(--chat-syntax-foreground)",
  background: "var(--chat-syntax-background)",
  "token-comment": "var(--chat-syntax-comment)",
  "token-keyword": "var(--chat-syntax-keyword)",
  "token-constant": "var(--chat-syntax-constant)",
  "token-string": "var(--chat-syntax-string)",
  "token-parameter": "var(--chat-syntax-parameter)",
  "token-function": "var(--chat-syntax-function)",
  "token-string-expression": "var(--chat-syntax-string-expression)",
  "token-punctuation": "var(--chat-syntax-punctuation)",
  "token-link": "var(--chat-syntax-link)",
  "token-inserted": "var(--chat-syntax-inserted)",
  "token-deleted": "var(--chat-syntax-deleted)",
}, true);
const REVIEW_DIFF_UNSAFE_CSS = `
  .chat-review-inline-comments { display: grid; gap: .35rem; padding: .35rem .5rem; }
  .chat-review-inline-comment { display: grid; gap: .3rem; border: 1px solid var(--border); border-radius: .45rem; background: var(--background); padding: .5rem; color: var(--foreground); font-family: var(--diffs-header-font-family); font-size: calc(.7rem * var(--type-scale)); }
  .chat-review-inline-comment[data-state="resolved"] { opacity: .64; }
  .chat-review-inline-comment > div, .chat-review-inline-comment footer { display: flex; align-items: center; gap: .35rem; }
  .chat-review-inline-comment > div span { margin-left: auto; color: var(--status-tentative); }
  .chat-review-inline-comment p { margin: 0; white-space: pre-wrap; overflow-wrap: anywhere; }
  .chat-review-inline-comment footer { justify-content: flex-end; }
  .chat-review-inline-comment button { border: 0; border-radius: .3rem; background: transparent; padding: .2rem .35rem; color: var(--muted-foreground); font: inherit; }
  .chat-review-inline-comment button:hover { background: var(--accent); color: var(--foreground); }
  .chat-review-inline-comment button:focus-visible { outline: 2px solid var(--ring); outline-offset: 1px; }
`;
let activeRuntimes = 0;
let workerTerminationTimer: number | null = null;
const workerErrorListeners = new Set<(reason: unknown) => void>();

/** Owns one imperative Pierre CodeView instance for a Svelte host. */
export class ReviewDiffRuntime {
  private readonly viewer!: CodeView<ReviewAnnotationData>;
  private readonly workerErrorListener: (reason: unknown) => void;
  private options: ReviewDiffRuntimeOptions;
  private itemToFile = new Map<string, string>();
  private parsedPageCache = new Map<string, FileDiffMetadata[] | null>();
  private cleanedUp = false;

  public constructor(host: HTMLElement, options: ReviewDiffRuntimeOptions) {
    this.options = options;
    this.workerErrorListener = (reason) => this.options.onWorkerError(reason);
    workerErrorListeners.add(this.workerErrorListener);
    retainWorkerPool();
    let viewer: CodeView<ReviewAnnotationData> | null = null;
    try {
      const workerManager = getOrCreateWorkerPoolSingleton({
        poolOptions: {
          workerFactory: () => {
            const worker = new DiffsWorker();
            worker.addEventListener("error", (event) => notifyWorkerError(event.error ?? event.message));
            return worker;
          },
          poolSize: workerPoolSize(),
          totalASTLRUCacheSize: 48,
        },
        highlighterOptions: {
          theme: { light: CHAT_REVIEW_SYNTAX_THEME, dark: CHAT_REVIEW_SYNTAX_THEME },
          tokenizeMaxLineLength: 1_000,
          useTokenTransformer: false,
        },
      });
      viewer = new CodeView<ReviewAnnotationData>(this.codeViewOptions(), workerManager);
      viewer.setup(host);
      this.viewer = viewer;
    } catch (reason: unknown) {
      try {
        viewer?.cleanUp();
      } finally {
        workerErrorListeners.delete(this.workerErrorListener);
        releaseWorkerPool();
      }
      throw reason;
    }
  }

  public setOptions(options: ReviewDiffRuntimeOptions): void {
    this.options = options;
    this.viewer.setOptions(this.codeViewOptions());
  }

  /** Parses pages independently, then merges valid pages into one virtualized file item. */
  public setItems(items: readonly ReviewDiffRenderItem[]): ReviewDiffRenderItem[] {
    const grouped = new Map<string, { source: ReviewDiffRenderItem; files: FileDiffMetadata[] }>();
    const rejected: ReviewDiffRenderItem[] = [];
    const activeCacheKeys = new Set<string>();
    for (const item of items) {
      const cacheKey = `${item.key}:${item.patchVersion}`;
      activeCacheKeys.add(cacheKey);
      try {
        let parsedFiles = this.parsedPageCache.get(cacheKey);
        if (parsedFiles === undefined) {
          parsedFiles = parsePatchFiles(item.patch, cacheKey, true).flatMap((patch) => patch.files);
          this.parsedPageCache.set(cacheKey, parsedFiles.length > 0 ? parsedFiles : null);
        }
        if (!parsedFiles || parsedFiles.length === 0) {
          rejected.push(item);
          continue;
        }
        const entry = grouped.get(item.fileId) ?? { source: item, files: [] };
        entry.files.push(...parsedFiles.map((file) => ({
          ...file,
          name: item.fileName,
          ...(item.previousFileName ? { prevName: item.previousFileName } : {}),
        })));
        if (item.pageIndex < entry.source.pageIndex) entry.source = item;
        grouped.set(item.fileId, entry);
      } catch (reason: unknown) {
        void reason;
        this.parsedPageCache.set(cacheKey, null);
        rejected.push(item);
      }
    }
    for (const cacheKey of this.parsedPageCache.keys()) {
      if (!activeCacheKeys.has(cacheKey)) this.parsedPageCache.delete(cacheKey);
    }

    this.itemToFile = new Map();
    const codeItems: CodeViewDiffItem<ReviewAnnotationData>[] = [];
    for (const [fileId, group] of grouped) {
      const itemId = `review-file:${fileId}`;
      this.itemToFile.set(itemId, fileId);
      const comments = items
        .filter((item) => item.fileId === fileId)
        .flatMap((item) => item.comments)
        .filter((comment, index, all) => all.findIndex((candidate) => candidate.id === comment.id) === index);
      codeItems.push({
        id: itemId,
        type: "diff",
        fileDiff: mergeFileDiffs(group.files, group.source.fileName, group.source.previousFileName),
        annotations: commentAnnotations(comments),
        version: combinedItemVersion(items.filter((item) => item.fileId === fileId), comments),
      });
    }
    this.viewer.setItems(codeItems);
    return rejected;
  }

  public scrollToFile(fileId: string, behavior: "instant" | "smooth-auto" = "smooth-auto"): void {
    const itemId = `review-file:${fileId}`;
    if (!this.viewer.getItem(itemId)) return;
    this.viewer.scrollTo({ type: "item", id: itemId, align: "start", behavior });
  }

  public scrollToLine(
    fileId: string,
    lineNumber: number,
    side: "deletions" | "additions",
  ): void {
    const itemId = `review-file:${fileId}`;
    if (!this.viewer.getItem(itemId)) return;
    this.viewer.scrollTo({
      type: "line",
      id: itemId,
      lineNumber,
      side,
      align: "center",
      behavior: "smooth-auto",
    });
  }

  public clearSelection(): void {
    this.viewer.clearSelectedLines({ notify: false });
    this.options.onSelection(null);
  }

  public cleanUp(): void {
    if (this.cleanedUp) return;
    this.cleanedUp = true;
    workerErrorListeners.delete(this.workerErrorListener);
    this.parsedPageCache.clear();
    try {
      this.viewer.cleanUp();
    } finally {
      releaseWorkerPool();
    }
  }

  private codeViewOptions(): CodeViewOptions<ReviewAnnotationData> {
    return {
      theme: { light: CHAT_REVIEW_SYNTAX_THEME, dark: CHAT_REVIEW_SYNTAX_THEME },
      themeType: document.documentElement.classList.contains("dark") ? "dark" as const : "light" as const,
      diffStyle: this.options.diffStyle,
      overflow: this.options.wrap ? "wrap" as const : "scroll" as const,
      diffIndicators: "bars" as const,
      hunkSeparators: "line-info-basic" as const,
      lineDiffType: "word-alt" as const,
      maxLineDiffLength: 1_000,
      tokenizeMaxLineLength: 1_000,
      tokenizeMaxLength: 250_000,
      unsafeCSS: REVIEW_DIFF_UNSAFE_CSS,
      collapsedContextThreshold: 18,
      expansionLineCount: 20,
      stickyHeaders: true,
      enableLineSelection: true,
      controlledSelection: true,
      lineHoverHighlight: "both" as const,
      layout: { paddingTop: 8, paddingBottom: 12, gap: 8 },
      itemMetrics: { lineHeight: 20, diffHeaderHeight: 40, spacing: 8, hunkLineCount: 120 },
      onSelectedLinesChange: (selection) => {
        this.viewer.setSelectedLines(selection, { notify: false });
        const fileId = selection ? this.itemToFile.get(selection.id) : undefined;
        this.options.onSelection(selection && fileId ? { fileId, range: selection.range } : null);
      },
      onLineClick: (_line, context) => {
        const fileId = this.itemToFile.get(context.item.id);
        if (fileId) this.options.onActiveFile(fileId);
      },
      renderAnnotation: (annotation: LineAnnotation<ReviewAnnotationData> | DiffLineAnnotation<ReviewAnnotationData>) => this.renderAnnotation(annotation),
    };
  }

  private renderAnnotation(
    annotation: LineAnnotation<ReviewAnnotationData> | DiffLineAnnotation<ReviewAnnotationData>,
  ): HTMLElement {
    const list = document.createElement("section");
    list.className = "chat-review-inline-comments";
    for (const comment of annotation.metadata.comments) {
      const article = document.createElement("article");
      article.className = "chat-review-inline-comment";
      article.dataset.state = comment.state;
      const heading = document.createElement("div");
      const location = document.createElement("strong");
      location.textContent = `${comment.relativePath}:${comment.startLine}`;
      heading.appendChild(location);
      if (comment.applicability && comment.applicability !== "current") {
        const stale = document.createElement("span");
        stale.textContent = this.options.labels.outdated;
        heading.appendChild(stale);
      }
      const text = document.createElement("p");
      text.textContent = comment.commentText;
      const actions = document.createElement("footer");
      actions.appendChild(this.commentButton(this.options.labels.attach, () => this.options.onAttachComment(comment)));
      actions.appendChild(this.commentButton(
        comment.state === "open" ? this.options.labels.resolve : this.options.labels.reopen,
        () => this.options.onResolveComment(comment, comment.state === "open"),
      ));
      article.append(heading, text, actions);
      list.appendChild(article);
    }
    return list;
  }

  private commentButton(label: string, action: () => void): HTMLButtonElement {
    const button = document.createElement("button");
    button.type = "button";
    button.textContent = label;
    button.addEventListener("click", action);
    return button;
  }
}

function mergeFileDiffs(
  files: readonly FileDiffMetadata[],
  fileName: string,
  previousFileName: string | null,
): FileDiffMetadata {
  const first = files[0];
  if (!first) throw new Error("A review file needs at least one valid patch page");
  if (files.length === 1) return first;
  const deletionLines: string[] = [];
  const additionLines: string[] = [];
  const hunks = [] as FileDiffMetadata["hunks"];
  let splitOffset = 0;
  let unifiedOffset = 0;
  for (const file of files) {
    const deletionOffset = deletionLines.length;
    const additionOffset = additionLines.length;
    deletionLines.push(...file.deletionLines);
    additionLines.push(...file.additionLines);
    hunks.push(...file.hunks.map((hunk) => ({
      ...hunk,
      deletionLineIndex: hunk.deletionLineIndex + deletionOffset,
      additionLineIndex: hunk.additionLineIndex + additionOffset,
      splitLineStart: hunk.splitLineStart + splitOffset,
      unifiedLineStart: hunk.unifiedLineStart + unifiedOffset,
    })));
    splitOffset += file.splitLineCount;
    unifiedOffset += file.unifiedLineCount;
  }
  return {
    ...first,
    name: fileName,
    ...(previousFileName ? { prevName: previousFileName } : {}),
    hunks,
    deletionLines,
    additionLines,
    splitLineCount: splitOffset,
    unifiedLineCount: unifiedOffset,
    cacheKey: `${fileName}:pages:${stableVersion(files.map((file) => file.cacheKey ?? "").join("\0"))}`,
  };
}

function combinedItemVersion(
  items: readonly ReviewDiffRenderItem[],
  comments: readonly ChatReviewCommentRead[],
): number {
  return stableVersion([
    ...items
      .toSorted((left, right) => left.pageIndex - right.pageIndex)
      .map((item) => `${item.pageIndex}:${item.version}`),
    ...comments.map((comment) => `${comment.id}:${comment.updatedAt}:${comment.state}`),
  ].join("\0"));
}

function stableVersion(value: string): number {
  let hash = 2_166_136_261;
  for (let index = 0; index < value.length; index += 1) {
    hash ^= value.charCodeAt(index);
    hash = Math.imul(hash, 16_777_619);
  }
  return hash >>> 0;
}

function commentAnnotations(
  comments: readonly ChatReviewCommentRead[],
): DiffLineAnnotation<ReviewAnnotationData>[] {
  const grouped = new Map<string, { side: "deletions" | "additions"; lineNumber: number; comments: ChatReviewCommentRead[] }>();
  for (const comment of comments) {
    const side = comment.selectionSide === "old" ? "deletions" : "additions";
    const lineNumber = comment.selectionSide === "file" ? 0 : comment.endLine;
    const key = `${side}:${lineNumber}`;
    const group = grouped.get(key) ?? { side, lineNumber, comments: [] };
    group.comments.push(comment);
    grouped.set(key, group);
  }
  return [...grouped.values()].map((group) => ({
    side: group.side,
    lineNumber: group.lineNumber,
    metadata: { comments: group.comments },
  }));
}

function workerPoolSize(): number {
  const cores = Math.max(1, navigator.hardwareConcurrency || 2);
  if (cores <= 4) return 1;
  if (cores <= 8) return 2;
  return 3;
}

function notifyWorkerError(reason: unknown): void {
  for (const listener of [...workerErrorListeners]) listener(reason);
}

function retainWorkerPool(): void {
  activeRuntimes += 1;
  if (workerTerminationTimer !== null) window.clearTimeout(workerTerminationTimer);
  workerTerminationTimer = null;
}

function releaseWorkerPool(): void {
  activeRuntimes = Math.max(0, activeRuntimes - 1);
  if (activeRuntimes > 0) return;
  workerTerminationTimer = window.setTimeout(() => {
    workerTerminationTimer = null;
    if (activeRuntimes === 0) terminateWorkerPoolSingleton();
  }, WORKER_IDLE_GRACE_MS);
}
