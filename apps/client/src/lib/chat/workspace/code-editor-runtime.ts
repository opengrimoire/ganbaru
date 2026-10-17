import { LanguageDescription, syntaxHighlighting, type LanguageSupport } from "@codemirror/language";
import { languages } from "@codemirror/language-data";
import { Compartment } from "@codemirror/state";
import { classHighlighter } from "@lezer/highlight";
import { basicSetup, EditorView } from "codemirror";

export interface ChatCodeEditorSelection {
  text: string;
  startLine: number;
  startColumn: number;
  endLine: number;
  endColumn: number;
}

export interface ChatCodeEditorRuntimeOptions {
  text: string;
  relativePath: string;
  readOnly: boolean;
  onChange: (text: string) => void;
  onSelectionChange: (selection: ChatCodeEditorSelection) => void;
  onSave: () => void;
}

const svelteDescription = LanguageDescription.of({
  name: "Svelte",
  extensions: ["svelte"],
  load: () => import("@replit/codemirror-lang-svelte").then((module) => module.svelte()),
});

/** Resolves presentation language metadata from a workspace-relative filename. */
export function chatLanguageDescription(relativePath: string): LanguageDescription | null {
  const filename = relativePath.replaceAll("\\", "/").split("/").pop() ?? relativePath;
  return LanguageDescription.matchFilename([svelteDescription, ...languages], filename);
}

/** Owns one lazily loaded CodeMirror editor and its parser lifecycle. */
export class ChatCodeEditorRuntime {
  private readonly view: EditorView;
  private readonly language = new Compartment();
  private readonly editable = new Compartment();
  private languageRequest = 0;
  private applyingExternalText = false;
  private destroyed = false;

  public constructor(host: HTMLElement, private readonly options: ChatCodeEditorRuntimeOptions) {
    this.view = new EditorView({
      doc: options.text,
      parent: host,
      extensions: [
        basicSetup,
        this.language.of([]),
        this.editable.of(EditorView.editable.of(!options.readOnly)),
        syntaxHighlighting(classHighlighter),
        EditorView.lineWrapping,
        EditorView.updateListener.of((update) => {
          if (update.docChanged && !this.applyingExternalText) {
            this.options.onChange(update.state.doc.toString());
          }
          if (update.selectionSet || update.docChanged) this.emitSelection();
        }),
        editorTheme,
      ],
    });
    host.addEventListener("keydown", this.handleKeydown);
    void this.loadRelativePath(options.relativePath);
  }

  public destroy(): void {
    if (this.destroyed) return;
    this.destroyed = true;
    this.languageRequest += 1;
    this.view.dom.parentElement?.removeEventListener("keydown", this.handleKeydown);
    this.view.destroy();
  }

  public setText(nextText: string): void {
    if (this.destroyed || this.view.state.doc.toString() === nextText) return;
    const currentDocument = this.view.state.doc;
    const currentSelection = this.view.state.selection.main;
    const anchorLine = currentDocument.lineAt(currentSelection.anchor);
    const headLine = currentDocument.lineAt(currentSelection.head);
    const anchor = { line: anchorLine.number, column: currentSelection.anchor - anchorLine.from };
    const head = { line: headLine.number, column: currentSelection.head - headLine.from };
    const scrollTop = this.view.scrollDOM.scrollTop;
    const scrollLeft = this.view.scrollDOM.scrollLeft;
    this.applyingExternalText = true;
    this.view.dispatch({ changes: { from: 0, to: currentDocument.length, insert: nextText } });
    const nextDocument = this.view.state.doc;
    const position = (coordinate: { line: number; column: number }): number => {
      const line = nextDocument.line(Math.min(coordinate.line, nextDocument.lines));
      return Math.min(line.to, line.from + coordinate.column);
    };
    this.view.dispatch({ selection: { anchor: position(anchor), head: position(head) } });
    this.restoreScroll(scrollTop, scrollLeft);
    window.requestAnimationFrame(() => this.restoreScroll(scrollTop, scrollLeft));
    this.applyingExternalText = false;
  }

  public setReadOnly(readOnly: boolean): void {
    if (this.destroyed || this.options.readOnly === readOnly) return;
    this.options.readOnly = readOnly;
    this.view.dispatch({ effects: this.editable.reconfigure(EditorView.editable.of(!readOnly)) });
  }

  public setRelativePath(relativePath: string): void {
    if (this.destroyed || this.options.relativePath === relativePath) return;
    this.options.relativePath = relativePath;
    void this.loadRelativePath(relativePath);
  }

  private async loadRelativePath(relativePath: string): Promise<void> {
    const request = ++this.languageRequest;
    const description = chatLanguageDescription(relativePath);
    let support: LanguageSupport | null = null;
    try {
      support = description ? await description.load() : null;
    } catch {
      support = null;
    }
    if (this.destroyed || request !== this.languageRequest) return;
    this.view.dispatch({ effects: this.language.reconfigure(support ?? []) });
  }

  private readonly handleKeydown = (event: KeyboardEvent): void => {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "s") {
      event.preventDefault();
      this.options.onSave();
    }
  };

  private emitSelection(): void {
    const range = this.view.state.selection.main;
    const start = this.view.state.doc.lineAt(range.from);
    const end = this.view.state.doc.lineAt(range.to);
    this.options.onSelectionChange({
      text: this.view.state.sliceDoc(range.from, range.to),
      startLine: start.number,
      startColumn: range.from - start.from + 1,
      endLine: end.number,
      endColumn: range.to - end.from + 1,
    });
  }

  private restoreScroll(top: number, left: number): void {
    if (this.destroyed) return;
    this.view.scrollDOM.scrollTop = Math.min(
      top,
      Math.max(0, this.view.scrollDOM.scrollHeight - this.view.scrollDOM.clientHeight),
    );
    this.view.scrollDOM.scrollLeft = Math.min(
      left,
      Math.max(0, this.view.scrollDOM.scrollWidth - this.view.scrollDOM.clientWidth),
    );
  }
}

const editorTheme = EditorView.theme({
  "&": {
    height: "100%",
    color: "var(--chat-syntax-foreground)",
    backgroundColor: "var(--chat-syntax-background)",
    fontSize: "calc(0.75rem * var(--type-scale))",
  },
  ".cm-scroller": {
    fontFamily: '"SF Mono", "SFMono-Regular", "JetBrains Mono", "Cascadia Code", Consolas, "Liberation Mono", Menlo, monospace',
    lineHeight: "calc(1.25rem * var(--type-scale))",
  },
  ".cm-content": { caretColor: "var(--foreground)" },
  ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--foreground)" },
  ".cm-gutters": {
    color: "var(--muted-foreground)",
    backgroundColor: "var(--chat-syntax-background)",
    borderRight: "1px solid var(--border)",
  },
  ".cm-activeLine, .cm-activeLineGutter": {
    backgroundColor: "color-mix(in srgb, var(--accent) 58%, transparent)",
  },
  ".cm-selectionBackground, &.cm-focused .cm-selectionBackground, ::selection": {
    backgroundColor: "color-mix(in srgb, var(--primary) 28%, transparent) !important",
  },
  ".cm-panels": { color: "var(--foreground)", backgroundColor: "var(--background)" },
  ".cm-panels input": {
    color: "var(--foreground)",
    backgroundColor: "var(--chat-syntax-background)",
    border: "1px solid var(--border)",
  },
  ".tok-comment": { color: "var(--chat-syntax-comment)", fontStyle: "italic" },
  ".tok-keyword, .tok-operator, .tok-meta": { color: "var(--chat-syntax-keyword)" },
  ".tok-atom, .tok-bool, .tok-number, .tok-literal": { color: "var(--chat-syntax-constant)" },
  ".tok-string": { color: "var(--chat-syntax-string)" },
  ".tok-string2": { color: "var(--chat-syntax-string-expression)" },
  ".tok-variableName2, .tok-labelName": { color: "var(--chat-syntax-parameter)" },
  ".tok-definition, .tok-typeName, .tok-className, .tok-namespace, .tok-macroName, .tok-propertyName": {
    color: "var(--chat-syntax-function)",
  },
  ".tok-punctuation": { color: "var(--chat-syntax-punctuation)" },
  ".tok-link, .tok-url": { color: "var(--chat-syntax-link)", textDecoration: "underline" },
  ".tok-heading, .tok-strong": { color: "var(--chat-syntax-keyword)", fontWeight: "700" },
  ".tok-emphasis": { fontStyle: "italic" },
  ".tok-inserted": { color: "var(--chat-syntax-inserted)" },
  ".tok-deleted": { color: "var(--chat-syntax-deleted)" },
  ".tok-invalid": {
    color: "var(--chat-syntax-invalid)",
    textDecoration: "underline wavy var(--chat-syntax-invalid)",
  },
});
