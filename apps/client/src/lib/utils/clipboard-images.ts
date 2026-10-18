/** Returns deduplicated clipboard images from both file and item representations. */
export function clipboardImageFiles(
  clipboard: Pick<DataTransfer, "files" | "items"> | null,
): File[] {
  if (!clipboard) return [];
  const images: File[] = [];
  const add = (file: File | null, advertisedType?: string): void => {
    if (!file) return;
    const mimeType = file.type || advertisedType || "";
    if (!mimeType.startsWith("image/")) return;
    if (file.name.trim()) {
      images.push(file);
      return;
    }
    const extension = imageExtension(mimeType);
    images.push(new File([file], `pasted-image.${extension}`, {
      type: mimeType,
      lastModified: file.lastModified,
    }));
  };
  for (const file of Array.from(clipboard.files)) add(file);
  if (images.length === 0) {
    for (const item of Array.from(clipboard.items)) {
      if (item.kind === "file") add(item.getAsFile(), item.type);
    }
  }
  return images;
}

function imageExtension(mimeType: string): "png" | "jpg" | "gif" | "webp" {
  if (mimeType === "image/jpeg") return "jpg";
  if (mimeType === "image/gif") return "gif";
  if (mimeType === "image/webp") return "webp";
  return "png";
}

const SINGLE_URL_PATTERN = /^[a-z][a-z0-9+.-]*:\S+$/iu;

/**
 * Whether a paste that carries images is a copied image rather than copied text. Browsers add the image address or
 * an `<img>` tag next to a copied image, while office suites add a picture of copied text, so text that reads as
 * more than an address or an image tag wins.
 */
export function clipboardPrefersImages(clipboard: Pick<DataTransfer, "getData">, imageCount: number): boolean {
  if (imageCount === 0) return false;
  const plainText = clipboard.getData("text/plain").trim();
  if (!plainText || SINGLE_URL_PATTERN.test(plainText)) return true;
  const html = clipboard.getData("text/html");
  return /<img\b/iu.test(html) && htmlText(html) === "";
}

/**
 * Whether a paste that the webview handed over without files may still hold a copied image on the system clipboard.
 * WebKitGTK leaves copied images out of paste events, so only a paste with no text at all, or only an image tag,
 * asks the system clipboard; any copied text keeps pasting as text.
 */
export function clipboardMayHoldNativeImage(clipboard: Pick<DataTransfer, "getData">): boolean {
  if (clipboard.getData("text/plain") !== "") return false;
  const html = clipboard.getData("text/html");
  return html === "" || (/<img\b/iu.test(html) && htmlText(html) === "");
}

function htmlText(html: string): string {
  return html
    .replace(/<(script|style)\b[\s\S]*?<\/\1\s*>/giu, "")
    .replace(/<[^>]*>/gu, "")
    .replace(/&nbsp;|&#160;/giu, " ")
    .trim();
}

/** Whether this webview can put images on the clipboard. */
export function imageClipboardSupported(): boolean {
  return typeof ClipboardItem !== "undefined"
    && typeof navigator !== "undefined"
    && typeof navigator.clipboard?.write === "function";
}

/**
 * Puts a PNG image on the clipboard. Call it from the user gesture: the clipboard item is created right away and
 * waits for the image, which some webviews require.
 */
export async function writePngToClipboard(png: Promise<Blob>): Promise<void> {
  await navigator.clipboard.write([new ClipboardItem({ "image/png": png })]);
}

/** Converts an image to PNG, which is the image type every clipboard accepts. */
export async function imageBlobToPng(blob: Blob): Promise<Blob> {
  if (blob.type === "image/png") return blob;
  const url = URL.createObjectURL(blob);
  try {
    const element = new Image();
    element.src = url;
    await element.decode();
    const canvas = document.createElement("canvas");
    canvas.width = element.naturalWidth;
    canvas.height = element.naturalHeight;
    const context = canvas.getContext("2d");
    if (!context) throw new Error("image could not be drawn for the clipboard");
    context.drawImage(element, 0, 0);
    const png = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, "image/png"));
    if (!png) throw new Error("image could not be converted for the clipboard");
    return png;
  } finally {
    URL.revokeObjectURL(url);
  }
}
