import type { NotesEquationBlockPayload } from "$lib/notes/types";

export function equationExpressionPlainText(equation: NotesEquationBlockPayload): string {
  return equation.expression;
}

export function equationPreviewText(expression: string): string {
  const normalized = expression.trim();
  return normalized.length > 0 ? normalized : "e=mc^2";
}
