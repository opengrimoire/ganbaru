//! Reads a copied image from the GTK clipboard. WebKitGTK leaves copied images out of paste
//! events, so a paste that carries no text asks GTK for the image instead.

use std::sync::mpsc;
use std::time::Duration;

use ganbaru_quick_notes::QuickNoteImageError;
use ganbaru_quick_notes::images::{MAX_IMAGE_BYTES, MAX_IMAGE_DIMENSION, MAX_IMAGE_PIXELS};
use gtk::gdk;
use gtk::gdk_pixbuf::Pixbuf;
use tauri::{AppHandle, Runtime};

/// Longest wait for the application that owns the clipboard to hand over its image.
const CLIPBOARD_TIMEOUT: Duration = Duration::from_secs(5);

type ClipboardImage = Result<Option<Vec<u8>>, QuickNoteImageError>;

/// The copied image as PNG bytes, or `None` when the clipboard holds no image.
///
/// GTK runs on the main thread, so this waits for it and must run on a blocking thread. A PNG
/// copy keeps its original bytes; other image types are converted to PNG.
pub(super) fn read_png<R: Runtime>(app: &AppHandle<R>) -> ClipboardImage {
    let (sender, receiver) = mpsc::sync_channel::<ClipboardImage>(1);
    app.run_on_main_thread(move || {
        let clipboard = gtk::Clipboard::get(&gdk::SELECTION_CLIPBOARD);
        clipboard.request_contents(&gdk::Atom::intern("image/png"), move |clipboard, data| {
            let png = data.data();
            if !png.is_empty() {
                let _ = sender.send(bounded_png(png));
                return;
            }
            clipboard.request_image(move |_, pixbuf| {
                let _ = sender.send(pixbuf.map_or(Ok(None), encode_png));
            });
        });
    })
    .map_err(|error| QuickNoteImageError::Failed(format!("clipboard read: {error}")))?;
    receiver
        .recv_timeout(CLIPBOARD_TIMEOUT)
        .map_err(|_| QuickNoteImageError::Failed("clipboard image did not arrive".to_string()))?
}

fn bounded_png(png: Vec<u8>) -> ClipboardImage {
    if u32::try_from(png.len()).map_or(true, |length| length > MAX_IMAGE_BYTES) {
        return Err(QuickNoteImageError::TooLarge);
    }
    Ok(Some(png))
}

/// Encodes a converted clipboard image, refusing sizes a Quick note would reject before
/// spending time on them.
fn encode_png(pixbuf: &Pixbuf) -> ClipboardImage {
    let (Ok(width), Ok(height)) = (
        u32::try_from(pixbuf.width()),
        u32::try_from(pixbuf.height()),
    ) else {
        return Err(QuickNoteImageError::Unsupported);
    };
    if !dimensions_allowed(width, height) {
        return Err(QuickNoteImageError::TooLarge);
    }
    let png = pixbuf
        .save_to_bufferv("png", &[])
        .map_err(|error| QuickNoteImageError::Failed(format!("encode clipboard image: {error}")))?;
    bounded_png(png)
}

fn dimensions_allowed(width: u32, height: u32) -> bool {
    width <= MAX_IMAGE_DIMENSION
        && height <= MAX_IMAGE_DIMENSION
        && u64::from(width) * u64::from(height) <= MAX_IMAGE_PIXELS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_converted_images_past_the_quick_note_limits() {
        assert!(dimensions_allowed(3840, 2160));
        assert!(dimensions_allowed(MAX_IMAGE_DIMENSION, 1));
        assert!(!dimensions_allowed(MAX_IMAGE_DIMENSION + 1, 1));
        assert!(!dimensions_allowed(
            MAX_IMAGE_DIMENSION,
            MAX_IMAGE_DIMENSION
        ));
    }

    #[test]
    fn refuses_copied_png_files_past_the_byte_limit() {
        assert!(matches!(bounded_png(vec![0; 16]), Ok(Some(bytes)) if bytes.len() == 16));
        let oversized = vec![0; usize::try_from(MAX_IMAGE_BYTES).unwrap_or(usize::MAX) + 1];
        assert!(matches!(
            bounded_png(oversized),
            Err(QuickNoteImageError::TooLarge)
        ));
    }
}
