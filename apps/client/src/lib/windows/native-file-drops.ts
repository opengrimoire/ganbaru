import { getCurrentWebview, type DragDropEvent } from "@tauri-apps/api/webview";

/**
 * A file drag over this page, as the operating system reports it. Desktop pages receive file drops natively instead
 * of as HTML5 drops; the dropped paths stay in Rust, which imports them through feature commands.
 */
export type NativeFileDrag =
  | { readonly kind: "hover" }
  | { readonly kind: "leave" }
  | { readonly kind: "drop"; readonly count: number };

/** Maps a native drag event to what a drop target needs, leaving the dropped paths out. */
export function nativeFileDrag(event: DragDropEvent): NativeFileDrag {
  switch (event.type) {
    case "enter":
    case "over":
      return { kind: "hover" };
    case "leave":
      return { kind: "leave" };
    case "drop":
      return { kind: "drop", count: event.paths.length };
  }
}

/**
 * Calls `handler` for every file drag over the current webview. Desktop windows host their pages as child webviews,
 * so drag events target the webview rather than the window.
 *
 * @returns A function that stops listening.
 */
export async function listenForNativeFileDrags(handler: (drag: NativeFileDrag) => void): Promise<() => void> {
  return getCurrentWebview().onDragDropEvent((event) => handler(nativeFileDrag(event.payload)));
}
